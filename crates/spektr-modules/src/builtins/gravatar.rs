//! `gravatar`: the public Gravatar profile for an email's SHA-256 hash.

use futures::future::BoxFuture;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, json_body, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "gravatar"
version     = "1.0.0"
description = "Gravatar profile for an email: avatar, display name, verified accounts"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["email"]
produces    = [["has_email", "email"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "2/s"
cost        = { class = "free" }
hosts       = ["api.gravatar.com"]
"#;

#[derive(Debug, Deserialize)]
struct Verified {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Profile {
    hash: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    profile_url: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    verified_accounts: Vec<Verified>,
}

pub struct Gravatar {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Gravatar {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("gravatar", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// Gravatar's hash: SHA-256 of the trimmed, lowercased address.
pub fn email_hash(email: &str) -> String {
    let d = Sha256::digest(email.trim().to_lowercase().as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

impl Module for Gravatar {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let hash = email_hash(&input.entity.key);
            let url = format!("https://api.gravatar.com/v3/profiles/{hash}");
            let Some(resp) = fetch(&self.http, &out, "o1", &url, &[]).await? else {
                return out.progress(100, "no Gravatar profile").await;
            };
            let p: Profile = json_body(&resp, "Gravatar profile")?;
            out.entity("acct", "account", &format!("gravatar:{}", p.hash), "o1")
                .await?;
            out.edge("acct", "has_email", "$input", "o1", Default::default())
                .await?;
            let text = |s: &str| AttrValue::Text(s.to_string());
            if let Some(n) = p.display_name.as_deref().filter(|s| !s.trim().is_empty()) {
                out.attr("acct", "display_name", text(n.trim()), false, "o1")
                    .await?;
            }
            if let Some(a) = &p.avatar_url {
                out.attr("acct", "avatar_url", AttrValue::Url(a.clone()), false, "o1")
                    .await?;
            }
            if let Some(u) = &p.profile_url {
                out.attr(
                    "acct",
                    "profile_url",
                    AttrValue::Url(u.clone()),
                    false,
                    "o1",
                )
                .await?;
            }
            if let Some(l) = p.location.as_deref().filter(|s| !s.trim().is_empty()) {
                out.attr("acct", "stated_location", text(l.trim()), false, "o1")
                    .await?;
            }
            for (i, v) in p.verified_accounts.iter().enumerate() {
                if let Some(u) = &v.url {
                    let r = format!("v{i}");
                    out.entity(&r, "url", u, "o1").await?;
                    out.edge("acct", "links_to", &r, "o1", Default::default())
                        .await?;
                }
            }
            out.progress(
                100,
                &format!("gravatar · {} verified", p.verified_accounts.len()),
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hash_is_sha256_of_the_normalized_address() {
        assert_eq!(
            super::email_hash(" Test@Example.com "),
            super::email_hash("test@example.com")
        );
        assert_eq!(super::email_hash("").len(), 64);
    }
}
