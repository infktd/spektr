//! `rdap`: registration data for domains, IP networks and ASNs, through
//! IANA's RDAP bootstrap. (`rdns` gives an IP's origin ASN; RDAP doesn't
//! reliably.)

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, EntitySpec, ErrorCode, Manifest};

use super::{Out, fetch, json_body, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "rdap"
version     = "1.0.0"
description = "RDAP registration: registrant, registered and expiry dates, network names, ASN operators"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["domain", "ip", "asn"]
produces    = [["registered_by", "organization"], ["registered_by", "person"], ["registered_by", "email"], ["operated_by", "organization"]]
cache_ttl   = "7d"
rate_limit  = "1/s"
cost        = { class = "free" }
hosts       = ["data.iana.org"]
"#;

const BOOTSTRAP: &str = "https://data.iana.org/rdap";

#[derive(Debug, Deserialize)]
struct Bootstrap {
    services: Vec<(Vec<String>, Vec<String>)>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    event_action: String,
    event_date: String,
}

#[derive(Debug, Deserialize)]
struct Entity {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default, rename = "vcardArray")]
    vcard: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Object {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    events: Vec<Event>,
    #[serde(default)]
    entities: Vec<Entity>,
}

/// `fn`, `org`, `email` and `kind` from a jCard.
fn vcard(v: &serde_json::Value, prop: &str) -> Option<String> {
    v.get(1)?.as_array()?.iter().find_map(|p| {
        let p = p.as_array()?;
        (p.first()?.as_str()? == prop).then(|| p.get(3)?.as_str().map(str::to_string))?
    })
}

pub struct Rdap {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Rdap {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net.http.for_module("rdap", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

fn ipv4_in(ip: std::net::Ipv4Addr, cidr: &str) -> bool {
    let Some((net, len)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(net), Ok(len)) = (net.parse::<std::net::Ipv4Addr>(), len.parse::<u32>()) else {
        return false;
    };
    if len > 32 {
        return false;
    }
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    u32::from(ip) & mask == u32::from(net) & mask
}

fn ipv6_in(ip: std::net::Ipv6Addr, cidr: &str) -> bool {
    let Some((net, len)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(net), Ok(len)) = (net.parse::<std::net::Ipv6Addr>(), len.parse::<u32>()) else {
        return false;
    };
    if len > 128 {
        return false;
    }
    let mask = if len == 0 {
        0
    } else {
        u128::MAX << (128 - len)
    };
    u128::from(ip) & mask == u128::from(net) & mask
}

/// The RDAP base URL for a query, from a bootstrap registry.
fn base_for(b: &Bootstrap, matches: impl Fn(&str) -> bool) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for (keys, urls) in &b.services {
        for k in keys {
            if matches(k) {
                let url = urls
                    .iter()
                    .find(|u| u.starts_with("https://"))
                    .or(urls.first())?
                    .clone();
                // Prefer the most specific match (longest key).
                if best.as_ref().is_none_or(|(l, _)| k.len() > *l) {
                    best = Some((k.len(), url));
                }
            }
        }
    }
    best.map(|(_, u)| if u.ends_with('/') { u } else { format!("{u}/") })
}

impl Module for Rdap {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let key = input.entity.key.clone();
            let ty = input.entity.ty.as_str().to_string();
            let (registry, path) = match ty.as_str() {
                "domain" => ("dns.json", format!("domain/{key}")),
                "ip" if key.contains(':') => ("ipv6.json", format!("ip/{key}")),
                "ip" => ("ipv4.json", format!("ip/{key}")),
                _ => (
                    "asn.json",
                    format!("autnum/{}", key.trim_start_matches("AS")),
                ),
            };
            let burl = format!("{BOOTSTRAP}/{registry}");
            let Some(b) = fetch(&self.http, &out, "b", &burl, &[]).await? else {
                return Err(ModuleError::code(
                    ErrorCode::Upstream,
                    "RDAP bootstrap unavailable",
                ));
            };
            let b: Bootstrap = json_body(&b, "RDAP bootstrap")?;
            let base = match ty.as_str() {
                "domain" => {
                    let tld = key.rsplit('.').next().unwrap_or("").to_string();
                    base_for(&b, |k| k.eq_ignore_ascii_case(&tld))
                }
                "ip" => match key.parse::<std::net::IpAddr>() {
                    Ok(std::net::IpAddr::V4(v4)) => base_for(&b, |k| ipv4_in(v4, k)),
                    Ok(std::net::IpAddr::V6(v6)) => base_for(&b, |k| ipv6_in(v6, k)),
                    Err(_) => None,
                },
                _ => {
                    let n: u64 = key.trim_start_matches("AS").parse().unwrap_or(0);
                    base_for(&b, |k| {
                        let (lo, hi) = k.split_once('-').unwrap_or((k, k));
                        matches!((lo.parse::<u64>(), hi.parse::<u64>()), (Ok(lo), Ok(hi)) if (lo..=hi).contains(&n))
                    })
                }
            };
            let Some(base) = base else {
                return out.progress(100, "no RDAP service for this registry").await;
            };
            let url = format!("{base}{path}");
            let Some(resp) = fetch(
                &self.http,
                &out,
                "o1",
                &url,
                &[("accept", "application/rdap+json")],
            )
            .await?
            else {
                return out.progress(100, "not registered").await;
            };
            let obj: Object = json_body(&resp, "RDAP")?;
            for e in &obj.events {
                let name = match e.event_action.as_str() {
                    "registration" => "registered_at",
                    "expiration" => "expires_at",
                    _ => continue,
                };
                out.attr(
                    "$input",
                    name,
                    AttrValue::Time(e.event_date.clone()),
                    true,
                    "o1",
                )
                .await?;
            }
            if let Some(n) = &obj.name {
                out.attr(
                    "$input",
                    "network_name",
                    AttrValue::Text(n.clone()),
                    false,
                    "o1",
                )
                .await?;
            }
            if let Some(h) = &obj.handle {
                out.attr(
                    "$input",
                    "rdap_handle",
                    AttrValue::Text(h.clone()),
                    false,
                    "o1",
                )
                .await?;
            }
            let mut linked = 0;
            for (i, e) in obj.entities.iter().enumerate() {
                let wanted = match ty.as_str() {
                    "domain" => e.roles.iter().any(|r| r == "registrant"),
                    "asn" => e
                        .roles
                        .iter()
                        .any(|r| r == "registrant" || r == "administrative"),
                    _ => false,
                };
                let Some(v) = e.vcard.as_ref().filter(|_| wanted) else {
                    continue;
                };
                let predicate = if ty == "asn" {
                    "operated_by"
                } else {
                    "registered_by"
                };
                let name = vcard(v, "fn").or_else(|| vcard(v, "org"));
                let individual = vcard(v, "kind").is_some_and(|k| k == "individual");
                if let Some(n) = name.filter(|n| {
                    !n.trim().is_empty() && !n.to_ascii_lowercase().contains("redacted")
                }) {
                    let r = format!("e{i}");
                    out.entity_with(
                        &r,
                        EntitySpec {
                            ty: if individual && ty == "domain" {
                                "person"
                            } else {
                                "organization"
                            }
                            .into(),
                            value: n,
                            blob: None,
                            dedup_id: e.handle.clone(),
                            key: None,
                        },
                        "o1",
                    )
                    .await?;
                    out.edge("$input", predicate, &r, "o1", Default::default())
                        .await?;
                    linked += 1;
                }
                if ty == "domain"
                    && let Some(email) = vcard(v, "email").filter(|m| m.contains('@'))
                {
                    let r = format!("m{i}");
                    out.entity(&r, "email", &email, "o1").await?;
                    out.edge("$input", "registered_by", &r, "o1", Default::default())
                        .await?;
                    linked += 1;
                }
            }
            out.progress(
                100,
                &format!(
                    "{} · {linked} contact{}",
                    obj.handle.as_deref().unwrap_or("RDAP"),
                    if linked == 1 { "" } else { "s" }
                ),
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_matching() {
        let b: Bootstrap = serde_json::from_str(
            r#"{"services":[[["example"],["https://rdap.example/"]],[["203.0.0.0/8"],["http://x/","https://rdap.arin.example/registry"]],[["64496-64511"],["https://rdap.asn.example/"]]]}"#,
        )
        .unwrap();
        assert_eq!(
            base_for(&b, |k| k == "example").as_deref(),
            Some("https://rdap.example/")
        );
        let v4: std::net::Ipv4Addr = "203.0.113.24".parse().unwrap();
        assert_eq!(
            base_for(&b, |k| ipv4_in(v4, k)).as_deref(),
            Some("https://rdap.arin.example/registry/")
        );
        assert!(!ipv4_in("198.51.100.7".parse().unwrap(), "203.0.0.0/8"));
        let card = serde_json::json!([
            "vcard",
            [
                ["version", {}, "text", "4.0"],
                ["fn", {}, "text", "Kestrel Labs LLC"]
            ]
        ]);
        assert_eq!(vcard(&card, "fn").as_deref(), Some("Kestrel Labs LLC"));
    }
}
