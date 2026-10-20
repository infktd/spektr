//! `crtsh`: certificates logged for a domain and its subdomains (crt.sh's
//! JSON over certificate transparency).

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, EntitySpec, Manifest};

use super::{Out, fetch, json_body, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "crtsh"
version     = "1.0.0"
description = "Certificates from CT logs (crt.sh): issued_for edges and new subdomains from SANs"
runtime     = "native"
concurrency = 1
timeout     = "120s"
technique   = "passive"
auto        = true
consumes    = ["domain"]
produces    = [["issued_for", "domain"], ["issued_for", "subdomain"]]
cache_ttl   = "24h"
rate_limit  = "6/min"
cost        = { class = "free" }
hosts       = ["crt.sh"]
"#;

/// Certificates kept per run, newest first.
const MAX_CERTS: usize = 100;

#[derive(Debug, Deserialize)]
struct Row {
    id: i64,
    #[serde(default)]
    issuer_name: String,
    #[serde(default)]
    common_name: String,
    #[serde(default)]
    name_value: String,
    #[serde(default)]
    not_before: Option<String>,
    #[serde(default)]
    not_after: Option<String>,
}

pub struct Crtsh {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Crtsh {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net.http.for_module("crtsh", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// `C=US, O=Let's Encrypt, CN=R11` → `R11`.
fn issuer_cn(issuer: &str) -> &str {
    issuer
        .split(',')
        .map(str::trim)
        .find_map(|p| p.strip_prefix("CN="))
        .unwrap_or(issuer)
}

/// crt.sh's timestamps lack a zone; they're UTC.
fn utc(t: &str) -> String {
    if t.ends_with('Z') || t.contains('+') {
        t.to_string()
    } else {
        format!("{t}Z")
    }
}

impl Module for Crtsh {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let domain = input.entity.key.clone();
            let url = format!("https://crt.sh/?q=%25.{domain}&output=json");
            let Some(resp) = fetch(
                &self.http,
                &out,
                "o1",
                &url,
                &[("accept", "application/json")],
            )
            .await?
            else {
                return out.progress(100, "no certificates").await;
            };
            let mut rows: Vec<Row> = json_body(&resp, "crt.sh")?;
            rows.sort_by_key(|r| std::cmp::Reverse(r.id));
            rows.dedup_by_key(|r| r.id);
            let total = rows.len();
            let mut hosts: Vec<String> = Vec::new();
            for (i, row) in rows.iter().take(MAX_CERTS).enumerate() {
                let r = format!("c{i}");
                out.entity_with(
                    &r,
                    EntitySpec {
                        ty: "certificate".into(),
                        value: format!("{} · {}", row.common_name, issuer_cn(&row.issuer_name)),
                        blob: None,
                        dedup_id: None,
                        key: Some(format!("crtsh:{}", row.id)),
                    },
                    "o1",
                )
                .await?;
                if let Some(t) = &row.not_before {
                    out.attr(&r, "valid_from", AttrValue::Time(utc(t)), false, "o1")
                        .await?;
                }
                if let Some(t) = &row.not_after {
                    out.attr(&r, "valid_to", AttrValue::Time(utc(t)), false, "o1")
                        .await?;
                }
                out.attr(
                    &r,
                    "issuer",
                    AttrValue::Text(issuer_cn(&row.issuer_name).to_string()),
                    false,
                    "o1",
                )
                .await?;
                let mut names: Vec<String> = row
                    .name_value
                    .split('\n')
                    .map(|n| n.trim().trim_start_matches("*.").to_ascii_lowercase())
                    .filter(|n| !n.is_empty())
                    .collect();
                names.sort();
                names.dedup();
                for n in names {
                    let target = if n == domain {
                        "$input".to_string()
                    } else {
                        let idx = match hosts.iter().position(|h| *h == n) {
                            Some(i) => i,
                            None => {
                                hosts.push(n.clone());
                                let i = hosts.len() - 1;
                                out.entity(&format!("h{i}"), "domain", &n, "o1").await?;
                                i
                            }
                        };
                        format!("h{idx}")
                    };
                    out.edge(&r, "issued_for", &target, "o1", Default::default())
                        .await?;
                }
            }
            out.progress(
                100,
                &format!(
                    "{total} certificate{} · {} names",
                    if total == 1 { "" } else { "s" },
                    hosts.len()
                ),
            )
            .await
        })
    }
}
