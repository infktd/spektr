//! `mastodon`: WebFinger plus the public account lookup on each configured
//! instance (`instances`, default `["mastodon.social"]`).

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, hrefs, json_body, manifest, strip_html};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "mastodon"
version     = "1.0.0"
description = "Mastodon profile: display name, bio, created date, last status, avatar, profile-field links"
runtime     = "native"
concurrency = 2
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["username", "account:mastodon"]
produces    = [["has_account", "account"], ["links_to", "url"]]
cache_ttl   = "24h"
rate_limit  = "1/s"
cost        = { class = "free" }
hosts       = ["mastodon.social"]

[module.config]
instances = { type = "list", default = ["mastodon.social"] }
"#;

#[derive(Debug, Deserialize)]
struct Field {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Deserialize)]
struct Account {
    username: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    last_status_at: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    fields: Vec<Field>,
}

pub struct Mastodon {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Mastodon {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("mastodon", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// `(user, instance)` pairs to try.
fn targets(input: &JobInput, config: &serde_json::Value) -> Vec<(String, String)> {
    if input.entity.ty.as_str() == "account" {
        // `mastodon:user@instance`
        return match input.entity.handle().split_once('@') {
            Some((u, i)) => vec![(u.to_string(), i.to_string())],
            None => Vec::new(),
        };
    }
    let user = input.entity.handle().trim_start_matches('@').to_string();
    let instances: Vec<String> = config
        .get("instances")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| vec!["mastodon.social".into()]);
    instances.into_iter().map(|i| (user.clone(), i)).collect()
}

/// The `YYYY-MM-DD` Mastodon gives for `last_status_at`, as a timestamp.
fn day(s: &str) -> String {
    if s.len() == 10 {
        format!("{s}T00:00:00Z")
    } else {
        s.to_string()
    }
}

impl Module for Mastodon {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let mut found = 0;
            for (n, (user, inst)) in targets(&input, &cx.config).iter().enumerate() {
                let wf =
                    format!("https://{inst}/.well-known/webfinger?resource=acct:{user}@{inst}");
                let ow = format!("w{n}");
                if fetch(
                    &self.http,
                    &out,
                    &ow,
                    &wf,
                    &[("accept", "application/jrd+json")],
                )
                .await?
                .is_none()
                {
                    continue;
                }
                let lookup = format!("https://{inst}/api/v1/accounts/lookup?acct={user}");
                let ol = format!("l{n}");
                let Some(resp) = fetch(&self.http, &out, &ol, &lookup, &[]).await? else {
                    continue;
                };
                let a: Account = json_body(&resp, "Mastodon account")?;
                found += 1;
                let acct = if input.entity.ty.as_str() == "account" {
                    "$input".to_string()
                } else {
                    let r = format!("acct{n}");
                    out.entity(
                        &r,
                        "account",
                        &format!("mastodon:{}@{inst}", a.username),
                        &ol,
                    )
                    .await?;
                    out.edge("$input", "has_account", &r, &ol, Default::default())
                        .await?;
                    r
                };
                let text = |s: &str| AttrValue::Text(s.to_string());
                if let Some(d) = a
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    out.attr(&acct, "display_name", text(d), false, &ol).await?;
                }
                if let Some(note) = &a.note {
                    let bio = strip_html(note);
                    if !bio.is_empty() {
                        out.attr(&acct, "bio", text(&bio), false, &ol).await?;
                    }
                }
                if let Some(c) = &a.created_at {
                    out.attr(&acct, "created_at", AttrValue::Time(c.clone()), true, &ol)
                        .await?;
                }
                if let Some(l) = &a.last_status_at {
                    out.attr(&acct, "last_active", AttrValue::Time(day(l)), true, &ol)
                        .await?;
                }
                if let Some(av) = &a.avatar {
                    out.attr(&acct, "avatar_url", AttrValue::Url(av.clone()), false, &ol)
                        .await?;
                }
                if let Some(u) = &a.url {
                    out.attr(&acct, "profile_url", AttrValue::Url(u.clone()), false, &ol)
                        .await?;
                }
                let mut links: Vec<String> = Vec::new();
                for f in &a.fields {
                    for h in hrefs(&f.value) {
                        if !links.contains(&h) {
                            links.push(h);
                        }
                    }
                }
                for (j, l) in links.iter().enumerate() {
                    let r = format!("link{n}_{j}");
                    out.entity(&r, "url", l, &ol).await?;
                    out.edge(&acct, "links_to", &r, &ol, Default::default())
                        .await?;
                }
            }
            out.progress(
                100,
                &if found == 0 {
                    "no Mastodon account".to_string()
                } else {
                    format!(
                        "{found} Mastodon account{}",
                        if found == 1 { "" } else { "s" }
                    )
                },
            )
            .await
        })
    }
}
