//! `dns`: A, AAAA and MX for domains and subdomains, through spektr's stub
//! resolver (`net::Dns`).

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::Manifest;

use super::{Out, manifest, net_err};
use crate::net::{Net, RType};

const MANIFEST: &str = r#"
[module]
name        = "dns"
version     = "1.0.0"
description = "A, AAAA and MX records from the system's nameservers"
runtime     = "native"
concurrency = 8
timeout     = "30s"
technique   = "passive"
auto        = true
consumes    = ["domain", "subdomain"]
produces    = [["resolves_to", "ip"], ["mail_handled_by", "domain"], ["mail_handled_by", "subdomain"]]
cache_ttl   = "24h"
cost        = { class = "free" }
"#;

pub struct DnsModule {
    manifest: Manifest,
    net: Net,
}

impl DnsModule {
    pub fn new(net: Net) -> Self {
        Self {
            manifest: manifest(MANIFEST),
            net,
        }
    }
}

impl Module for DnsModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let name = input.entity.key.clone();
            let dns = &self.net.dns;
            let a = dns.lookup("dns", &name, RType::A).await.map_err(net_err)?;
            let aaaa = dns
                .lookup("dns", &name, RType::Aaaa)
                .await
                .map_err(net_err)?;
            let mx = dns.lookup("dns", &name, RType::Mx).await.map_err(net_err)?;
            let out = Out(&cx.emit);
            let raw = json!({ "name": name, "A": a, "AAAA": aaaa, "MX": mx });
            out.observation("o1", None, Some("application/json"), Some(raw.to_string()), Some(json!({
                "A": a.answers.len(), "AAAA": aaaa.answers.len(), "MX": mx.answers.len(), "rcode": a.rcode,
            })))
            .await?;
            for (i, ip) in a.answers.iter().chain(&aaaa.answers).enumerate() {
                let r = format!("ip{i}");
                out.entity(&r, "ip", ip, "o1").await?;
                out.edge("$input", "resolves_to", &r, "o1", Default::default())
                    .await?;
            }
            for (i, rec) in mx.answers.iter().enumerate() {
                let Some((_, host)) = rec.split_once(' ') else {
                    continue;
                };
                if host.is_empty() || host == "." {
                    continue; // null MX (RFC 7505)
                }
                let r = format!("mx{i}");
                out.entity(&r, "domain", host, "o1").await?;
                out.edge("$input", "mail_handled_by", &r, "o1", Default::default())
                    .await?;
            }
            let found = a.answers.len() + aaaa.answers.len();
            out.progress(
                100,
                &format!(
                    "{found} address{} · {} MX",
                    if found == 1 { "" } else { "es" },
                    mx.answers.len()
                ),
            )
            .await?;
            Ok(())
        })
    }
}
