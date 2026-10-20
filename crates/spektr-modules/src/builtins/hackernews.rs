//! `hackernews`: the Hacker News public Firebase API.

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_model::PlatformRegistry;
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, hrefs, manifest, strip_html, urls_in};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "hackernews"
version     = "1.0.0"
description = "Hacker News user: created date, karma, about text and its links"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:hackernews"]
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "2/s"
cost        = { class = "free" }
hosts       = ["hacker-news.firebaseio.com"]
"#;

#[derive(Debug, Deserialize)]
struct User {
    id: String,
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    karma: Option<i64>,
    #[serde(default)]
    about: Option<String>,
}

pub struct Hackernews {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Hackernews {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("hackernews", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

impl Module for Hackernews {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let handle = input.entity.handle().to_string();
            // Not a possible HN username (e.g. a dot): nothing to ask.
            if PlatformRegistry::builtin()
                .get("hackernews")
                .is_some_and(|p| p.normalize_handle(&handle).is_err())
            {
                return Ok(());
            }
            let url = format!("https://hacker-news.firebaseio.com/v0/user/{handle}.json");
            let Some(resp) = fetch(&self.http, &out, "o1", &url, &[]).await? else {
                return out.progress(100, "no HN user").await;
            };
            // The API answers `null` for an unknown user.
            let Ok(Some(u)) = resp.json::<Option<User>>() else {
                return out.progress(100, "no HN user").await;
            };
            let acct = if input.entity.ty.as_str() == "account" {
                "$input".to_string()
            } else {
                out.entity("acct", "account", &format!("hackernews:{}", u.id), "o1")
                    .await?;
                out.edge("$input", "has_account", "acct", "o1", Default::default())
                    .await?;
                "acct".to_string()
            };
            if let Some(t) = u.created.and_then(|s| jiff::Timestamp::from_second(s).ok()) {
                out.attr(
                    &acct,
                    "created_at",
                    AttrValue::Time(t.to_string()),
                    true,
                    "o1",
                )
                .await?;
            }
            if let Some(k) = u.karma {
                out.attr(&acct, "karma", AttrValue::Int(k), false, "o1")
                    .await?;
            }
            if let Some(about) = &u.about {
                let text = strip_html(about);
                if !text.is_empty() {
                    out.attr(&acct, "bio", AttrValue::Text(text.clone()), false, "o1")
                        .await?;
                }
                let mut links = hrefs(about);
                for u in urls_in(&text) {
                    if !links.contains(&u) {
                        links.push(u);
                    }
                }
                for (j, l) in links.iter().enumerate() {
                    let r = format!("link{j}");
                    out.entity(&r, "url", l, "o1").await?;
                    out.edge(&acct, "links_to", &r, "o1", Default::default())
                        .await?;
                }
            }
            out.progress(100, &format!("hackernews · {}", u.id)).await
        })
    }
}
