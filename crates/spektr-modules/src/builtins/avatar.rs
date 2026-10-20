//! `avatar-hash`: fetch an account's `avatar_url`, store the image, and
//! record its 64-bit dHash as `avatar_phash` for the correlator.

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_model::AttrValue as ModelValue;
use spektr_protocol::{AttrValue, EntitySpec, Manifest};

use super::{Out, fetch, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "avatar-hash"
version     = "1.0.0"
description = "Fetch an account's avatar, store it, and record its perceptual hash (dHash)"
runtime     = "native"
concurrency = 4
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["account"]
produces    = [["posted", "image"]]
cache_ttl   = "7d"
rate_limit  = "2/s"
cost        = { class = "free" }
"#;

pub struct AvatarHash {
    manifest: Manifest,
    http: ModuleHttp,
}

impl AvatarHash {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("avatar-hash", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// The first avatar URL any source recorded on the account.
fn avatar_url(input: &JobInput) -> Option<String> {
    input
        .entity
        .attrs
        .get("avatar_url")?
        .iter()
        .find_map(|a| match &a.value {
            ModelValue::Url(u) | ModelValue::Text(u) => Some(u.clone()),
            _ => None,
        })
}

impl Module for AvatarHash {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let Some(url) = avatar_url(&input) else {
                return Ok(()); // no avatar recorded yet
            };
            let Some(resp) = fetch(&self.http, &out, "o1", &url, &[("accept", "image/*")]).await?
            else {
                return out.progress(100, "avatar gone").await;
            };
            let Some(hash) = crate::media::dhash(&resp.body) else {
                return out.progress(100, "avatar didn't decode").await;
            };
            let name = url
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("avatar")
                .to_string();
            out.entity_with(
                "img",
                EntitySpec {
                    ty: "image".into(),
                    value: name,
                    blob: Some("o1".into()),
                    dedup_id: None,
                    key: None,
                },
                "o1",
            )
            .await?;
            let mut attrs = serde_json::Map::new();
            attrs.insert("role".into(), json!("avatar"));
            out.edge("$input", "posted", "img", "o1", attrs).await?;
            out.attr(
                "$input",
                "avatar_phash",
                AttrValue::Text(hash.clone()),
                false,
                "o1",
            )
            .await?;
            out.attr(
                "img",
                "source_url",
                AttrValue::Url(url.clone()),
                false,
                "o1",
            )
            .await?;
            out.progress(100, &format!("dHash {hash}")).await
        })
    }
}
