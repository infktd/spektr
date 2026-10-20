//! `username-sweep`: check one handle against the public profile URLs in
//! `data/sites.json` (MODULES §6).
//!
//! - **found:** an account entity plus `has_account`, at strength single
//! - **unsure:** a `sweep_unsure` review item (login wall, blocked, rate limited)
//! - **not found:** counted in the summary only
//!
//! 16 sites in flight, 1 request per second per host. A site whose platform
//! rules can't accept the handle (e.g. a dot on GitHub) is skipped without
//! a request.

use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_model::PlatformRegistry;
use spektr_protocol::Manifest;

use super::{BuiltinOptions, Out, manifest, net_err};
use crate::net::{HttpResponse, ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "username-sweep"
version     = "1.0.0"
description = "Checks a handle against about 300 public profile URLs"
runtime     = "native"
concurrency = 2
timeout     = "600s"
technique   = "passive"
auto        = true
consumes    = ["username"]
produces    = [["has_account", "account"]]
cache_ttl   = "24h"
rate_limit  = "1/s"
cost        = { class = "free" }
"#;

const SITES: &str = include_str!("../../data/sites.json");
const IN_FLIGHT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// 200 found; 404/410 not found.
    Status,
    /// 200 and `text` present: not found.
    AbsentText,
    /// `text` present: found.
    PresentText,
    /// Redirected away from the profile URL: not found.
    Redirect,
    /// Can't be checked without logging in: always unsure.
    LoginWall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unsure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A platform id from `spektr-model`'s platform rules.
    pub platform: String,
    /// The URL to check; `{username}` is replaced.
    pub url: String,
    pub method: Method,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The human-facing profile URL when `url` is an API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The account handle when it isn't just the username
    /// (e.g. `{username}@mastodon.social`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsure: Vec<Unsure>,
}

impl Site {
    pub fn check_url(&self, username: &str) -> String {
        let enc: String = url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
        self.url.replace("{username}", &enc)
    }

    pub fn profile_url(&self, username: &str) -> String {
        let enc: String = url::form_urlencoded::byte_serialize(username.as_bytes()).collect();
        self.profile
            .as_deref()
            .unwrap_or(&self.url)
            .replace("{username}", &enc)
    }

    pub fn handle_for(&self, username: &str) -> String {
        self.handle
            .as_deref()
            .unwrap_or("{username}")
            .replace("{username}", username)
    }
}

/// The embedded list. It is parsed by a unit test, so failure here is
/// unreachable in a tested build; an empty list is the defensive fallback.
pub fn embedded_sites() -> Vec<Site> {
    serde_json::from_str(SITES).unwrap_or_default()
}

/// The embedded list, overridden and extended by `<config>/sites.json`
/// (merged by site id). Returns a warning if the user file is unusable.
pub fn load_sites(config_dir: Option<&Path>) -> (Vec<Site>, Option<String>) {
    let mut sites = embedded_sites();
    let Some(path) = config_dir
        .map(|d| d.join("sites.json"))
        .filter(|p| p.exists())
    else {
        return (sites, None);
    };
    let user: Vec<Site> = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(u) => u,
        Err(e) => {
            return (
                sites,
                Some(format!(
                    "{}: {e} (using the built-in site list)",
                    path.display()
                )),
            );
        }
    };
    for s in user {
        match sites.iter_mut().find(|x| x.id == s.id) {
            Some(existing) => *existing = s,
            None => sites.push(s),
        }
    }
    (sites, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Found,
    Unsure(String),
    NotFound,
}

/// Decide a site's outcome from its response.
pub fn classify(site: &Site, requested: &str, resp: &HttpResponse) -> Outcome {
    let body = resp.text();
    for u in &site.unsure {
        let status_ok = u.status.is_none_or(|s| s == resp.status);
        let text_ok = u.text.as_deref().is_none_or(|t| body.contains(t));
        if status_ok && text_ok && (u.status.is_some() || u.text.is_some()) {
            return Outcome::Unsure(u.reason.clone());
        }
    }
    if site.method == Method::LoginWall {
        return Outcome::Unsure("login wall".into());
    }
    match resp.status {
        429 => return Outcome::Unsure("rate limited (HTTP 429)".into()),
        403 => return Outcome::Unsure("blocked by the site (HTTP 403)".into()),
        s if s >= 500 => return Outcome::Unsure(format!("site error (HTTP {s})")),
        _ => {}
    }
    let text = site.text.as_deref().unwrap_or("");
    match site.method {
        Method::Status => match resp.status {
            200 => Outcome::Found,
            404 | 410 => Outcome::NotFound,
            s => Outcome::Unsure(format!("HTTP {s}")),
        },
        Method::AbsentText => match resp.status {
            200 if !text.is_empty() && body.contains(text) => Outcome::NotFound,
            200 => Outcome::Found,
            404 | 410 => Outcome::NotFound,
            s => Outcome::Unsure(format!("HTTP {s}")),
        },
        Method::PresentText => {
            if resp.status == 200 && !text.is_empty() && body.contains(text) {
                Outcome::Found
            } else if matches!(resp.status, 200 | 404 | 410) {
                Outcome::NotFound
            } else {
                Outcome::Unsure(format!("HTTP {}", resp.status))
            }
        }
        Method::Redirect => {
            let same =
                crate::net::normalize_url(&resp.final_url) == crate::net::normalize_url(requested);
            match resp.status {
                200 if same => Outcome::Found,
                200 | 404 | 410 => Outcome::NotFound,
                s => Outcome::Unsure(format!("HTTP {s}")),
            }
        }
        Method::LoginWall => Outcome::Unsure("login wall".into()),
    }
}

pub struct Sweep {
    manifest: Manifest,
    http: ModuleHttp,
    sites: Arc<Vec<Site>>,
}

impl Sweep {
    pub fn new(net: Net, opts: &BuiltinOptions) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("username-sweep", manifest.rate_limit.as_deref());
        let (sites, warning) = load_sites(opts.config_dir.as_deref());
        if let Some(w) = warning {
            tracing::warn!("{w}");
        }
        Self {
            manifest,
            http,
            sites: Arc::new(sites),
        }
    }

    pub fn sites(&self) -> &[Site] {
        &self.sites
    }
}

enum Checked {
    Skipped,
    Done(Box<HttpResponse>, String, Outcome),
    Failed(String),
}

impl Module for Sweep {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let username = input.entity.key.clone();
            let platforms = PlatformRegistry::builtin();
            let total = self.sites.len();
            let out = Out(&cx.emit);
            let checks = futures::stream::iter(self.sites.iter().cloned().enumerate())
                .map(|(i, site)| {
                    let handle = site.handle_for(&username);
                    let valid = platforms
                        .get(&site.platform)
                        .is_none_or(|p| p.normalize_handle(&handle).is_ok());
                    let http = self.http.clone();
                    let username = username.clone();
                    async move {
                        if !valid {
                            return (i, site, Ok(Checked::Skipped));
                        }
                        let url = site.check_url(&username);
                        let r = match http.get(&url, &[]).await {
                            Ok(resp) => {
                                let o = classify(&site, &url, &resp);
                                Ok(Checked::Done(Box::new(resp), url, o))
                            }
                            Err(e) if e.is_fixture_miss() => Err(net_err(e)),
                            Err(e) => Ok(Checked::Failed(e.to_string())),
                        };
                        (i, site, r)
                    }
                })
                .buffer_unordered(IN_FLIGHT);
            tokio::pin!(checks);
            let (mut found, mut unsure, mut none, mut skipped, mut done) =
                (0u32, 0u32, 0u32, 0u32, 0usize);
            while let Some((i, site, r)) = checks.next().await {
                if cx.cancel.is_cancelled() {
                    return Err(ModuleError::Cancelled);
                }
                let name = site
                    .name
                    .clone()
                    .or_else(|| platforms.get(&site.platform).map(|p| p.name.to_string()))
                    .unwrap_or_else(|| site.id.clone());
                match r? {
                    Checked::Skipped => skipped += 1,
                    Checked::Failed(err) => {
                        unsure += 1;
                        let id = format!("s{i}");
                        out.observation(
                            &id,
                            Some(&site.check_url(&username)),
                            None,
                            None,
                            Some(json!({ "error": err })),
                        )
                        .await?;
                        out.review("sweep_unsure", json!({
                            "site": site.id, "name": name, "username": username, "platform": site.platform,
                            "url": site.profile_url(&username), "reason": format!("network error: {err}"),
                        }), &id).await?;
                    }
                    Checked::Done(resp, url, outcome) => match outcome {
                        Outcome::NotFound => none += 1,
                        Outcome::Found => {
                            found += 1;
                            let id = format!("s{i}");
                            out.response(&id, &url, &resp).await?;
                            let r = format!("a{i}");
                            out.entity(
                                &r,
                                "account",
                                &format!("{}:{}", site.platform, site.handle_for(&username)),
                                &id,
                            )
                            .await?;
                            out.edge("$input", "has_account", &r, &id, Default::default())
                                .await?;
                        }
                        Outcome::Unsure(reason) => {
                            unsure += 1;
                            let id = format!("s{i}");
                            out.response(&id, &url, &resp).await?;
                            out.review("sweep_unsure", json!({
                                "site": site.id, "name": name, "username": username, "platform": site.platform,
                                "url": site.profile_url(&username), "reason": reason,
                            }), &id).await?;
                        }
                    },
                }
                done += 1;
                if done % 25 == 0 && done < total {
                    let pct = u8::try_from(done * 100 / total.max(1)).unwrap_or(99);
                    out.progress(pct, &format!("{done}/{total} sites")).await?;
                }
            }
            let not_found = none + skipped;
            out.observation("summary", None, Some("application/json"), None, Some(json!({
                "sites": total, "found": found, "unsure": unsure, "none": none, "skipped": skipped,
            })))
            .await?;
            out.progress(
                100,
                &format!("{total} sites · {found} found · {unsure} unsure · {not_found} none"),
            )
            .await?;
            Ok(())
        })
    }
}

/// Build replay fixtures for `username` where the given sites are found and
/// the login-walled sites are unsure; every other site answers "not found"
/// in the way its method expects. All bodies are fictional.
/// The tutor handle's sweep: found on these sites…
#[doc(hidden)]
pub const TUTOR_FOUND: &[&str] = &[
    "bluesky",
    "gitlab",
    "instagram",
    "tiktok",
    "medium",
    "mastodon-mastodon.social",
    "aboutme",
];
/// …and blocked (HTTP 403, so unsure) on these.
#[doc(hidden)]
pub const TUTOR_BLOCKED: &[&str] = &["vk", "okru"];

#[doc(hidden)]
/// `shared`: sites whose check URL another module's fixture already answers
/// (the sweep's Bluesky check is the AppView call `bluesky` makes); they're
/// left out so each request has one recorded response.
/// Sites the tutor fixtures answer from another module's fixture.
pub const TUTOR_SHARED: &[&str] = &["bluesky", "mastodon-mastodon.social"];

pub fn fixture_lines(
    username: &str,
    found: &[&str],
    blocked: &[&str],
    shared: &[&str],
) -> Vec<String> {
    use base64::Engine as _;
    let platforms = PlatformRegistry::builtin();
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    let mut lines = Vec::new();
    for site in embedded_sites() {
        if shared.contains(&site.id.as_str()) {
            continue;
        }
        let handle = site.handle_for(username);
        if platforms
            .get(&site.platform)
            .is_some_and(|p| p.normalize_handle(&handle).is_err())
        {
            continue;
        }
        let url = site.check_url(username);
        let is_found = found.contains(&site.id.as_str());
        let is_blocked = blocked.contains(&site.id.as_str());
        let text = site.text.clone().unwrap_or_default();
        let page = |body: &str| format!("<!doctype html><title>{}</title><p>{body}</p>", site.id);
        let (status, body, final_url) = match (site.method, is_found) {
            _ if is_blocked => (403, page("Access denied."), None),
            (Method::LoginWall, _) => (200, page("Log in to continue."), None),
            (Method::Status, true) | (Method::Redirect, true) => (
                200,
                page(&format!("Profile of {handle}. Fictional fixture.")),
                None,
            ),
            (Method::Status, false) => (404, page("Not found."), None),
            (Method::AbsentText, true) => (200, page(&format!("Profile of {handle}.")), None),
            (Method::AbsentText, false) => (200, page(&text), None),
            (Method::PresentText, true) => (200, page(&format!("{text} {handle}")), None),
            (Method::PresentText, false) => (200, page("Nothing here."), None),
            (Method::Redirect, false) => (
                200,
                page("Home"),
                Some(format!("https://{}/", super::host_of(&url))),
            ),
        };
        let mut ex = json!({
            "kind": "http", "method": "GET", "url": url, "status": status,
            "headers": { "content-type": "text/html; charset=utf-8" }, "body_b64": b64(&body),
        });
        if let Some(f) = final_url {
            ex["final_url"] = json!(f);
        }
        lines.push(ex.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed replay fixture for the tutor handle matches the
    /// generator. `SPEKTR_REGEN_FIXTURES=1` rewrites it.
    #[test]
    fn sweep_fixture_is_current() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/username-sweep/jmarlow.wav.jsonl");
        let mut text = String::from(
            "// Generated by username-sweep's fixture_lines(); regenerate with SPEKTR_REGEN_FIXTURES=1. Fictional.\n",
        );
        for line in fixture_lines("jmarlow.wav", TUTOR_FOUND, TUTOR_BLOCKED, TUTOR_SHARED) {
            text.push_str(&line);
            text.push('\n');
        }
        if std::env::var_os("SPEKTR_REGEN_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            committed == text,
            "{} is stale; run SPEKTR_REGEN_FIXTURES=1 cargo nextest run -p spektr-modules sweep_fixture_is_current",
            path.display()
        );
    }

    #[test]
    fn site_list_is_valid_and_matches_platform_rules() {
        let sites = embedded_sites();
        assert!(sites.len() >= 300, "{} sites", sites.len());
        let platforms = PlatformRegistry::builtin();
        let mut ids = std::collections::BTreeSet::new();
        for s in &sites {
            assert!(ids.insert(&s.id), "duplicate site {}", s.id);
            assert!(
                platforms.get(&s.platform).is_some(),
                "site {} uses platform {} missing from platforms.rs",
                s.id,
                s.platform
            );
            assert!(s.url.contains("{username}"), "{}", s.id);
            assert!(s.url.starts_with("https://"), "{}", s.id);
            if matches!(s.method, Method::AbsentText | Method::PresentText) {
                assert!(
                    s.text.as_deref().is_some_and(|t| !t.is_empty()),
                    "{} needs text",
                    s.id
                );
            }
            if let Some(p) = &s.profile {
                assert!(p.contains("{username}"), "{}", s.id);
            }
        }
    }

    fn resp(status: u16, body: &str, final_url: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: Default::default(),
            body: body.as_bytes().to_vec(),
            final_url: final_url.into(),
        }
    }

    #[test]
    fn classification() {
        let site = |m: Method, text: Option<&str>| Site {
            id: "x".into(),
            name: None,
            platform: "github".into(),
            url: "https://x.example/{username}".into(),
            method: m,
            text: text.map(str::to_string),
            profile: None,
            handle: None,
            unsure: vec![],
        };
        let u = "https://x.example/jm";
        assert_eq!(
            classify(&site(Method::Status, None), u, &resp(200, "", u)),
            Outcome::Found
        );
        assert_eq!(
            classify(&site(Method::Status, None), u, &resp(404, "", u)),
            Outcome::NotFound
        );
        assert_eq!(
            classify(&site(Method::Status, None), u, &resp(403, "", u)),
            Outcome::Unsure("blocked by the site (HTTP 403)".into())
        );
        assert_eq!(
            classify(&site(Method::Status, None), u, &resp(429, "", u)),
            Outcome::Unsure("rate limited (HTTP 429)".into())
        );
        assert_eq!(
            classify(
                &site(Method::AbsentText, Some("No such user")),
                u,
                &resp(200, "No such user.", u)
            ),
            Outcome::NotFound
        );
        assert_eq!(
            classify(
                &site(Method::AbsentText, Some("No such user")),
                u,
                &resp(200, "jm's page", u)
            ),
            Outcome::Found
        );
        assert_eq!(
            classify(
                &site(Method::PresentText, Some("\"ok\":true")),
                u,
                &resp(200, "{\"ok\":true}", u)
            ),
            Outcome::Found
        );
        assert_eq!(
            classify(
                &site(Method::PresentText, Some("\"ok\":true")),
                u,
                &resp(200, "{}", u)
            ),
            Outcome::NotFound
        );
        assert_eq!(
            classify(
                &site(Method::Redirect, None),
                u,
                &resp(200, "", "https://x.example/")
            ),
            Outcome::NotFound
        );
        assert_eq!(
            classify(&site(Method::Redirect, None), u, &resp(200, "", u)),
            Outcome::Found
        );
        assert_eq!(
            classify(&site(Method::LoginWall, None), u, &resp(200, "", u)),
            Outcome::Unsure("login wall".into())
        );
        let mut custom = site(Method::Status, None);
        custom.unsure.push(Unsure {
            status: Some(200),
            text: Some("Please verify you are human".into()),
            reason: "captcha".into(),
        });
        assert_eq!(
            classify(&custom, u, &resp(200, "Please verify you are human", u)),
            Outcome::Unsure("captcha".into())
        );
    }

    #[test]
    fn user_sites_override_by_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("sites.json"),
            r#"[{"id":"github","platform":"github","url":"https://gh.example/{username}","method":"status"},
                {"id":"forum-example","name":"Example Forum","platform":"github","url":"https://forum.example/u/{username}","method":"status"}]"#,
        )
        .unwrap();
        let (sites, warn) = load_sites(Some(dir.path()));
        assert!(warn.is_none());
        assert_eq!(
            sites.iter().find(|s| s.id == "github").unwrap().url,
            "https://gh.example/{username}"
        );
        assert!(sites.iter().any(|s| s.id == "forum-example"));
        std::fs::write(dir.path().join("sites.json"), "not json").unwrap();
        let (sites, warn) = load_sites(Some(dir.path()));
        assert!(warn.is_some());
        assert!(sites.len() >= 300);
    }
}
