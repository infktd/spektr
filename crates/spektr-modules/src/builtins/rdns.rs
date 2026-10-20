//! `rdns`: PTR lookup, plus the origin ASN from Team Cymru's IP-to-ASN DNS
//! service (`origin.asn.cymru.com` TXT). DNS only.

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, ErrorCode, Manifest};

use super::{Out, manifest, net_err};
use crate::net::{Net, RType, reverse_name};

const MANIFEST: &str = r#"
[module]
name        = "rdns"
version     = "1.0.0"
description = "Reverse DNS (PTR) and the origin ASN (Team Cymru DNS)"
runtime     = "native"
concurrency = 8
timeout     = "30s"
technique   = "passive"
auto        = true
consumes    = ["ip"]
produces    = [["reverse_resolves_to", "domain"], ["reverse_resolves_to", "subdomain"], ["hosted_in_asn", "asn"]]
cache_ttl   = "24h"
cost        = { class = "free" }
"#;

pub struct Rdns {
    manifest: Manifest,
    net: Net,
}

impl Rdns {
    pub fn new(net: Net) -> Self {
        Self {
            manifest: manifest(MANIFEST),
            net,
        }
    }
}

/// The Cymru query name: reversed octets (or nibbles) under the origin zone.
fn cymru_name(ip: std::net::IpAddr) -> String {
    let rev = reverse_name(ip);
    match ip {
        std::net::IpAddr::V4(_) => format!(
            "{}.origin.asn.cymru.com",
            rev.trim_end_matches(".in-addr.arpa")
        ),
        std::net::IpAddr::V6(_) => format!(
            "{}.origin6.asn.cymru.com",
            rev.trim_end_matches(".ip6.arpa")
        ),
    }
}

/// `"64500 | 203.0.113.0/24 | US | arin | 2010-01-01"` → (asn, prefix, cc).
fn parse_cymru(txt: &str) -> Option<(u32, String, String)> {
    let parts: Vec<&str> = txt.trim_matches('"').split('|').map(str::trim).collect();
    let asn = parts.first()?.split_whitespace().next()?.parse().ok()?;
    Some((
        asn,
        parts.get(1)?.to_string(),
        parts.get(2).unwrap_or(&"").to_string(),
    ))
}

impl Module for Rdns {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let ip: std::net::IpAddr = input.entity.key.parse().map_err(|_| {
                ModuleError::code(ErrorCode::Parse, format!("not an IP: {}", input.entity.key))
            })?;
            let dns = &self.net.dns;
            let ptr = dns
                .lookup("rdns", &reverse_name(ip), RType::Ptr)
                .await
                .map_err(net_err)?;
            let txt = dns
                .lookup("rdns", &cymru_name(ip), RType::Txt)
                .await
                .map_err(net_err)?;
            let raw = json!({ "PTR": ptr, "TXT": txt });
            out.observation(
                "o1",
                None,
                Some("application/json"),
                Some(raw.to_string()),
                Some(json!({
                    "ptr": ptr.answers.len(), "asn": txt.answers.first(),
                })),
            )
            .await?;
            for (i, host) in ptr.answers.iter().enumerate() {
                let r = format!("h{i}");
                out.entity(&r, "domain", host, "o1").await?;
                out.edge(
                    "$input",
                    "reverse_resolves_to",
                    &r,
                    "o1",
                    Default::default(),
                )
                .await?;
            }
            let mut note = format!("{} PTR", ptr.answers.len());
            if let Some((asn, prefix, cc)) = txt.answers.first().and_then(|t| parse_cymru(t)) {
                out.entity("asn", "asn", &format!("AS{asn}"), "o1").await?;
                out.edge("$input", "hosted_in_asn", "asn", "o1", Default::default())
                    .await?;
                out.attr(
                    "$input",
                    "bgp_prefix",
                    AttrValue::Text(prefix.clone()),
                    false,
                    "o1",
                )
                .await?;
                if !cc.is_empty() {
                    out.attr("$input", "country", AttrValue::Text(cc), false, "o1")
                        .await?;
                }
                note = format!("{note} · AS{asn} · {prefix}");
            }
            out.progress(100, &note).await
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cymru() {
        assert_eq!(
            super::cymru_name("203.0.113.24".parse().unwrap()),
            "24.113.0.203.origin.asn.cymru.com"
        );
        assert_eq!(
            super::parse_cymru("64500 | 203.0.113.0/24 | US | arin | 2010-01-01"),
            Some((64500, "203.0.113.0/24".into(), "US".into()))
        );
    }
}
