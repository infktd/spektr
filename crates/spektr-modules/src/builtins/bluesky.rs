//! `bluesky`: the public AT Protocol AppView profile
//! (`app.bsky.actor.getProfile`).

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, json_body, manifest, urls_in};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "bluesky"
version     = "1.0.0"
description = "Bluesky profile: display name, bio, created date, avatar, DID, links"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:bluesky"]
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "2/s"
cost        = { class = "free" }
hosts       = ["public.api.bsky.app"]
"#;

const API: &str = "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    did: String,
    handle: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

pub struct Bluesky {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Bluesky {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("bluesky", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// Handles to ask for: an account's own handle, or for a username the
/// default `<name>.bsky.social` (plus the name itself when it's already a
/// domain-shaped handle with a real public suffix).
fn actors(input: &JobInput) -> Vec<String> {
    let h = input.entity.handle().to_string();
    if input.entity.ty.as_str() == "account" {
        return vec![h];
    }
    let mut v = vec![format!("{h}.bsky.social")];
    if spektr_model::has_known_suffix(&h) {
        v.push(h);
    }
    v
}

impl Module for Bluesky {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            for (i, actor) in actors(&input).iter().enumerate() {
                let obs = format!("o{i}");
                let url = format!("{API}?actor={actor}");
                // The AppView answers 400 "Profile not found" for unknown handles.
                let Some(resp) = fetch(
                    &self.http,
                    &out,
                    &obs,
                    &url,
                    &[("accept", "application/json")],
                )
                .await
                .or_else(|e| match &e {
                    ModuleError::Code { message, .. } if message.starts_with("HTTP 400") => {
                        Ok(None)
                    }
                    _ => Err(e),
                })?
                else {
                    continue;
                };
                let p: Profile = json_body(&resp, "Bluesky profile")?;
                let acct = if input.entity.ty.as_str() == "account" {
                    "$input".to_string()
                } else {
                    out.entity("acct", "account", &format!("bluesky:{}", p.handle), &obs)
                        .await?;
                    out.edge("$input", "has_account", "acct", &obs, Default::default())
                        .await?;
                    "acct".to_string()
                };
                let text = |s: &str| AttrValue::Text(s.to_string());
                if let Some(n) = p.display_name.as_deref().filter(|s| !s.trim().is_empty()) {
                    out.attr(&acct, "display_name", text(n.trim()), false, &obs)
                        .await?;
                }
                if let Some(b) = p.description.as_deref().filter(|s| !s.trim().is_empty()) {
                    out.attr(&acct, "bio", text(b.trim()), false, &obs).await?;
                    for (j, u) in urls_in(b).iter().enumerate() {
                        let r = format!("link{j}");
                        out.entity(&r, "url", u, &obs).await?;
                        out.edge(&acct, "links_to", &r, &obs, Default::default())
                            .await?;
                    }
                }
                if let Some(c) = &p.created_at {
                    out.attr(&acct, "created_at", AttrValue::Time(c.clone()), true, &obs)
                        .await?;
                }
                if let Some(a) = &p.avatar {
                    out.attr(&acct, "avatar_url", AttrValue::Url(a.clone()), false, &obs)
                        .await?;
                }
                out.attr(&acct, "did", text(&p.did), false, &obs).await?;
                out.attr(
                    &acct,
                    "profile_url",
                    AttrValue::Url(format!("https://bsky.app/profile/{}", p.handle)),
                    false,
                    &obs,
                )
                .await?;
                out.progress(100, &format!("bluesky · {}", p.handle))
                    .await?;
                return Ok(());
            }
            out.progress(100, "no Bluesky profile").await?;
            Ok(())
        })
    }
}
