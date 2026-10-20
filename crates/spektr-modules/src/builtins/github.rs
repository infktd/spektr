//! `github`: the GitHub REST API's public user profile, plus the newest
//! public event for `last_active`.

use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_model::PlatformRegistry;
use spektr_protocol::{AttrValue, ErrorCode, Manifest};

use super::{Out, manifest, net_err, rate_limited};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "github"
version     = "1.0.0"
description = "GitHub profile: names, dates, links, public email, stated location, last activity"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:github"]
produces    = [["has_account", "account"], ["links_to", "url"], ["has_email", "email"], ["located_in", "location"]]
cache_ttl   = "24h"
rate_limit  = "1/s"
cost        = { class = "free" }
optional_secrets = ["GITHUB_TOKEN"]
hosts       = ["api.github.com"]
"#;

const API: &str = "https://api.github.com";

#[derive(Debug, Deserialize)]
struct User {
    login: String,
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    bio: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    blog: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    company: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Event {
    created_at: Option<String>,
}

pub struct Github {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Github {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("github", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl Module for Github {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let handle = input.entity.handle().to_string();
            // Not a possible GitHub login (e.g. `jmarlow.wav`): nothing to ask.
            let platforms = PlatformRegistry::builtin();
            if platforms
                .get("github")
                .is_some_and(|p| p.normalize_handle(&handle).is_err())
            {
                return Ok(());
            }
            let token = cx
                .secrets
                .get("GITHUB_TOKEN")
                .map(|t| format!("Bearer {t}"));
            let mut headers = vec![
                ("accept", "application/vnd.github+json"),
                ("x-github-api-version", "2022-11-28"),
            ];
            if let Some(t) = &token {
                headers.push(("authorization", t.as_str()));
            }
            let now = jiff::Timestamp::now().as_second();
            let url = format!("{API}/users/{handle}");
            let resp = self.http.get(&url, &headers).await.map_err(net_err)?;
            if let Some(e) = rate_limited(&resp, now) {
                return Err(e);
            }
            let out = Out(&cx.emit);
            out.response("o1", &url, &resp).await?;
            match resp.status {
                200 => {}
                404 => {
                    out.progress(100, "no GitHub user").await?;
                    return Ok(());
                }
                s if s >= 500 => {
                    return Err(ModuleError::code(
                        ErrorCode::Upstream,
                        format!("HTTP {s} from api.github.com"),
                    ));
                }
                s => {
                    return Err(ModuleError::code(
                        ErrorCode::Upstream,
                        format!("unexpected HTTP {s} from api.github.com"),
                    ));
                }
            }
            let user: User = resp.json().map_err(|e| {
                ModuleError::code(ErrorCode::Parse, format!("GitHub user JSON: {e}"))
            })?;

            // The account: the input itself, or a new entity found from a username.
            let acct = if input.entity.ty.as_str() == "account" {
                "$input".to_string()
            } else {
                out.entity("acct", "account", &format!("github:{}", user.login), "o1")
                    .await?;
                out.edge("$input", "has_account", "acct", "o1", Default::default())
                    .await?;
                "acct".to_string()
            };
            let text = |s: &str| AttrValue::Text(s.to_string());
            if let Some(n) = nonempty(&user.name) {
                out.attr(&acct, "display_name", text(n), false, "o1")
                    .await?;
            }
            if let Some(b) = nonempty(&user.bio) {
                out.attr(&acct, "bio", text(b), false, "o1").await?;
            }
            if let Some(c) = nonempty(&user.created_at) {
                out.attr(&acct, "created_at", AttrValue::Time(c.into()), true, "o1")
                    .await?;
            }
            if let Some(a) = nonempty(&user.avatar_url) {
                out.attr(&acct, "avatar_url", AttrValue::Url(a.into()), false, "o1")
                    .await?;
            }
            if let Some(h) = nonempty(&user.html_url) {
                out.attr(&acct, "profile_url", AttrValue::Url(h.into()), false, "o1")
                    .await?;
            }
            if let Some(c) = nonempty(&user.company) {
                out.attr(&acct, "company", text(c), false, "o1").await?;
            }
            if let Some(id) = user.id {
                out.attr(&acct, "github_id", AttrValue::Int(id), false, "o1")
                    .await?;
            }
            if let Some(blog) = nonempty(&user.blog) {
                let link = if blog.contains("://") {
                    blog.to_string()
                } else {
                    format!("https://{blog}")
                };
                out.attr(&acct, "website", AttrValue::Url(link.clone()), false, "o1")
                    .await?;
                out.entity("blog", "url", &link, "o1").await?;
                out.edge(&acct, "links_to", "blog", "o1", Default::default())
                    .await?;
            }
            if let Some(email) = nonempty(&user.email) {
                out.entity("email", "email", email, "o1").await?;
                out.edge(&acct, "has_email", "email", "o1", Default::default())
                    .await?;
            }
            if let Some(loc) = nonempty(&user.location) {
                out.attr(&acct, "stated_location", text(loc), false, "o1")
                    .await?;
                out.entity("loc", "location", loc, "o1").await?;
                let mut attrs = serde_json::Map::new();
                attrs.insert("kind".into(), json!("stated"));
                out.edge(&acct, "located_in", "loc", "o1", attrs).await?;
            }

            // Newest public event → last_active.
            let events_url = format!("{API}/users/{}/events/public?per_page=1", user.login);
            let ev = self
                .http
                .get(&events_url, &headers)
                .await
                .map_err(net_err)?;
            if let Some(e) = rate_limited(&ev, now) {
                return Err(e);
            }
            if ev.status == 200 {
                out.response("o2", &events_url, &ev).await?;
                let events: Vec<Event> = ev.json().unwrap_or_default();
                if let Some(t) = events.first().and_then(|e| e.created_at.clone()) {
                    out.attr(&acct, "last_active", AttrValue::Time(t), true, "o2")
                        .await?;
                }
            }
            out.progress(100, &format!("github · {}", user.login))
                .await?;
            Ok(())
        })
    }
}
