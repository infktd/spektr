//! `gitlab`: the GitLab public users API on gitlab.com.

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, json_body, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "gitlab"
version     = "1.0.0"
description = "GitLab profile: display name, bio, created date, avatar, website"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:gitlab"]
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "2/s"
cost        = { class = "free" }
hosts       = ["gitlab.com"]
"#;

const API: &str = "https://gitlab.com/api/v4/users";

#[derive(Debug, Deserialize)]
struct Brief {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct User {
    username: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    bio: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    web_url: Option<String>,
    #[serde(default)]
    website_url: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    location: Option<String>,
}

pub struct Gitlab {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Gitlab {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("gitlab", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl Module for Gitlab {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let handle = input.entity.handle().to_string();
            let url = format!("{API}?username={handle}");
            let Some(resp) = fetch(&self.http, &out, "o1", &url, &[]).await? else {
                return out.progress(100, "no GitLab user").await;
            };
            let found: Vec<Brief> = json_body(&resp, "GitLab users")?;
            let Some(b) = found.first() else {
                return out.progress(100, "no GitLab user").await;
            };
            let detail = format!("{API}/{}", b.id);
            let Some(resp) = fetch(&self.http, &out, "o2", &detail, &[]).await? else {
                return out.progress(100, "no GitLab user").await;
            };
            let u: User = json_body(&resp, "GitLab user")?;
            let acct = if input.entity.ty.as_str() == "account" {
                "$input".to_string()
            } else {
                out.entity("acct", "account", &format!("gitlab:{}", u.username), "o2")
                    .await?;
                out.edge("$input", "has_account", "acct", "o2", Default::default())
                    .await?;
                "acct".to_string()
            };
            let text = |s: &str| AttrValue::Text(s.to_string());
            if let Some(n) = nonempty(&u.name) {
                out.attr(&acct, "display_name", text(n), false, "o2")
                    .await?;
            }
            if let Some(b) = nonempty(&u.bio) {
                out.attr(&acct, "bio", text(b), false, "o2").await?;
            }
            if let Some(c) = nonempty(&u.created_at) {
                out.attr(&acct, "created_at", AttrValue::Time(c.into()), true, "o2")
                    .await?;
            }
            if let Some(a) = nonempty(&u.avatar_url) {
                out.attr(&acct, "avatar_url", AttrValue::Url(a.into()), false, "o2")
                    .await?;
            }
            if let Some(w) = nonempty(&u.web_url) {
                out.attr(&acct, "profile_url", AttrValue::Url(w.into()), false, "o2")
                    .await?;
            }
            if let Some(l) = nonempty(&u.location) {
                out.attr(&acct, "stated_location", text(l), false, "o2")
                    .await?;
            }
            if let Some(site) = nonempty(&u.website_url) {
                let link = if site.contains("://") {
                    site.to_string()
                } else {
                    format!("https://{site}")
                };
                out.attr(&acct, "website", AttrValue::Url(link.clone()), false, "o2")
                    .await?;
                out.entity("site", "url", &link, "o2").await?;
                out.edge(&acct, "links_to", "site", "o2", Default::default())
                    .await?;
            }
            out.progress(100, &format!("gitlab · {}", u.username)).await
        })
    }
}
