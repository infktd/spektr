//! Built-in modules (MODULES §6). All native, all `technique = passive`,
//! all recorded as fixtures for tests and the tutor.

mod avatar;
mod bluesky;
mod crtsh;
mod dns;
mod email_check;
mod exif;
mod fingerprint;
mod github;
mod gitlab;
pub mod gravatar;
mod hackernews;
mod hibp;
mod mastodon;
mod phone;
mod rdap;
mod rdns;
pub mod sweep;
mod wayback;

use std::path::PathBuf;

use serde_json::json;
use spektr_engine::{Emitter, Module, ModuleError};
use spektr_protocol::{AttrValue, EntitySpec, ErrorCode, Manifest, ModuleMessage};

use crate::net::{HttpResponse, Net, NetError};

/// Where built-ins find user overrides (`<config>/sites.json`).
#[derive(Debug, Clone, Default)]
pub struct BuiltinOptions {
    pub config_dir: Option<PathBuf>,
}

/// Every built-in module, each holding a clone of `net`.
pub fn builtins(net: Net) -> Vec<Box<dyn Module>> {
    builtins_with(net, BuiltinOptions::default())
}

pub fn builtins_with(net: Net, opts: BuiltinOptions) -> Vec<Box<dyn Module>> {
    vec![
        Box::new(sweep::Sweep::new(net.clone(), &opts)),
        Box::new(github::Github::new(net.clone())),
        Box::new(bluesky::Bluesky::new(net.clone())),
        Box::new(gitlab::Gitlab::new(net.clone())),
        Box::new(mastodon::Mastodon::new(net.clone())),
        Box::new(hackernews::Hackernews::new(net.clone())),
        Box::new(gravatar::Gravatar::new(net.clone())),
        Box::new(email_check::EmailCheck::new(net.clone())),
        Box::new(hibp::Hibp::new(net.clone())),
        Box::new(phone::PhoneParse::new()),
        Box::new(exif::Exif::new()),
        Box::new(avatar::AvatarHash::new(net.clone())),
        Box::new(fingerprint::Fingerprint::new(net.clone())),
        Box::new(wayback::Wayback::new(net.clone())),
        Box::new(dns::DnsModule::new(net.clone())),
        Box::new(rdns::Rdns::new(net.clone())),
        Box::new(rdap::Rdap::new(net.clone())),
        Box::new(crtsh::Crtsh::new(net)),
    ]
}

/// Parse a built-in's embedded manifest. Every built-in manifest is parsed
/// by a unit test (`manifests_parse`), so failure here is unreachable in a
/// tested build.
pub(crate) fn manifest(src: &str) -> Manifest {
    match Manifest::from_toml(src) {
        Ok(m) => m,
        // Unreachable: covered by the `manifests_parse` test.
        Err(e) => panic!("built-in manifest is invalid: {e}"),
    }
}

/// Network errors → module errors. Missing fixtures are never retried.
pub(crate) fn net_err(e: NetError) -> ModuleError {
    match e {
        NetError::Timeout => ModuleError::code(ErrorCode::Timeout, "request timed out"),
        NetError::NoFixture { .. }
        | NetError::NoDnsFixture { .. }
        | NetError::BadUrl(_)
        | NetError::Record(_) => ModuleError::Internal(e.to_string()),
        NetError::TooLarge(_) => ModuleError::Code {
            code: ErrorCode::Upstream,
            message: e.to_string(),
            retryable: false,
            retry_after: None,
        },
        NetError::Http(_) | NetError::Dns(_) | NetError::NoNameservers => {
            ModuleError::code(ErrorCode::Upstream, e.to_string())
        }
    }
}

/// 429, or 403 with an exhausted rate-limit header: `rate_limited`, with
/// `Retry-After` (seconds) or the reset time when given.
pub(crate) fn rate_limited(resp: &HttpResponse, now_unix: i64) -> Option<ModuleError> {
    let exhausted = resp.header("x-ratelimit-remaining") == Some("0");
    if resp.status != 429 && !(resp.status == 403 && exhausted) {
        return None;
    }
    let after = resp
        .header("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .or_else(|| {
            resp.header("x-ratelimit-reset")
                .and_then(|v| v.trim().parse::<i64>().ok())
                .map(|reset| u64::try_from((reset - now_unix).max(1)).unwrap_or(60))
        });
    Some(ModuleError::Code {
        code: ErrorCode::RateLimited,
        message: format!("HTTP {} from {}", resp.status, host_of(&resp.final_url)),
        retryable: true,
        retry_after: after.map(std::time::Duration::from_secs),
    })
}

pub(crate) fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

/// GET `url` and record the response as observation `obs`. `Ok(None)` for
/// 404 and 410 (nothing there); rate limits and server errors are errors.
pub(crate) async fn fetch(
    http: &crate::net::ModuleHttp,
    out: &Out<'_>,
    obs: &str,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Option<HttpResponse>, ModuleError> {
    let now = jiff::Timestamp::now().as_second();
    let resp = http.get(url, headers).await.map_err(net_err)?;
    if let Some(e) = rate_limited(&resp, now) {
        return Err(e);
    }
    out.response(obs, url, &resp).await?;
    match resp.status {
        200..=299 => Ok(Some(resp)),
        404 | 410 => Ok(None),
        s => Err(ModuleError::code(
            ErrorCode::Upstream,
            format!("HTTP {s} from {}", host_of(url)),
        )),
    }
}

/// Parse a JSON body, or fail the job with `parse`.
pub(crate) fn json_body<T: serde::de::DeserializeOwned>(
    resp: &HttpResponse,
    what: &str,
) -> Result<T, ModuleError> {
    resp.json()
        .map_err(|e| ModuleError::code(ErrorCode::Parse, format!("{what} JSON: {e}")))
}

/// http(s) URLs in free text (bios, about fields), deduplicated, in order.
pub(crate) fn urls_in(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let Some(re) = RE
        .get_or_init(|| regex::Regex::new(r#"https?://[^\s<>"'()\[\]{}]+"#).ok())
        .as_ref()
    else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for m in re.find_iter(text) {
        let u = m
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .to_string();
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

/// Text content of a small HTML fragment (bios, profile fields): tags
/// dropped, `<br>`/`</p>` as spaces, common entities decoded.
pub(crate) fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut tag = String::new();
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.to_ascii_lowercase();
                if t.starts_with("br") || t.starts_with("/p") {
                    out.push(' ');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `href` values of `<a>` tags in an HTML fragment.
pub(crate) fn hrefs(html: &str) -> Vec<String> {
    let doc = scraper::Html::parse_fragment(html);
    let Ok(sel) = scraper::Selector::parse("a[href]") else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for a in doc.select(&sel) {
        if let Some(h) = a.value().attr("href")
            && (h.starts_with("http://") || h.starts_with("https://"))
            && !out.iter().any(|o| o == h)
        {
            out.push(h.to_string());
        }
    }
    out
}

/// Builder-style helpers over the emitter.
pub(crate) struct Out<'a>(pub &'a Emitter);

impl Out<'_> {
    pub async fn observation(
        &self,
        id: &str,
        url: Option<&str>,
        content_type: Option<&str>,
        raw: Option<String>,
        summary: Option<serde_json::Value>,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Observation {
                job: None,
                id: id.into(),
                source_url: url.map(str::to_string),
                content_type: content_type.map(str::to_string),
                raw,
                raw_b64: None,
                summary,
            })
            .await
    }

    pub async fn response(
        &self,
        id: &str,
        url: &str,
        resp: &HttpResponse,
    ) -> Result<(), ModuleError> {
        let summary = json!({ "status": resp.status, "final_url": resp.final_url });
        if std::str::from_utf8(&resp.body).is_ok() {
            self.observation(
                id,
                Some(url),
                resp.content_type(),
                Some(resp.text()),
                Some(summary),
            )
            .await
        } else {
            use base64::Engine as _;
            self.0
                .emit(ModuleMessage::Observation {
                    job: None,
                    id: id.into(),
                    source_url: Some(url.into()),
                    content_type: resp.content_type().map(str::to_string),
                    raw: None,
                    raw_b64: Some(base64::engine::general_purpose::STANDARD.encode(&resp.body)),
                    summary: Some(summary),
                })
                .await
        }
    }

    pub async fn entity(
        &self,
        r: &str,
        ty: &str,
        value: &str,
        ev: &str,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Entity {
                job: None,
                reference: r.into(),
                entity: EntitySpec {
                    ty: ty.into(),
                    value: value.into(),
                    blob: None,
                    dedup_id: None,
                    key: None,
                },
                evidence: vec![ev.into()],
            })
            .await
    }

    /// An entity with a display label, a stable dedup id, or image bytes
    /// held by an observation (`blob`).
    pub async fn entity_with(
        &self,
        r: &str,
        spec: EntitySpec,
        ev: &str,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Entity {
                job: None,
                reference: r.into(),
                entity: spec,
                evidence: vec![ev.into()],
            })
            .await
    }

    pub async fn edge(
        &self,
        src: &str,
        predicate: &str,
        dst: &str,
        ev: &str,
        attrs: serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Edge {
                job: None,
                src: src.into(),
                predicate: predicate.into(),
                dst: dst.into(),
                attrs,
                evidence: vec![ev.into()],
            })
            .await
    }

    pub async fn attr(
        &self,
        target: &str,
        name: &str,
        value: AttrValue,
        event: bool,
        ev: &str,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Attr {
                job: None,
                target: target.into(),
                name: name.into(),
                value,
                event,
                evidence: vec![ev.into()],
            })
            .await
    }

    pub async fn review(
        &self,
        kind: &str,
        payload: serde_json::Value,
        ev: &str,
    ) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Review {
                job: None,
                kind: kind.into(),
                payload,
                evidence: vec![ev.into()],
            })
            .await
    }

    pub async fn progress(&self, pct: u8, note: &str) -> Result<(), ModuleError> {
        self.0
            .emit(ModuleMessage::Progress {
                job: None,
                pct,
                note: Some(note.into()),
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_parse() {
        let net = Net::replay(crate::net::Fixtures::default());
        let mods = builtins(net);
        let names: Vec<&str> = mods.iter().map(|m| m.manifest().name.as_str()).collect();
        assert_eq!(
            names,
            [
                "username-sweep",
                "github",
                "bluesky",
                "gitlab",
                "mastodon",
                "hackernews",
                "gravatar",
                "email-check",
                "hibp",
                "phone-parse",
                "exif",
                "avatar-hash",
                "http-fingerprint",
                "wayback",
                "dns",
                "rdns",
                "rdap",
                "crtsh",
            ]
        );
        for m in &mods {
            assert_eq!(
                m.manifest().technique,
                spektr_protocol::Technique::Passive,
                "{}",
                m.manifest().name
            );
            assert!(m.manifest().check().is_ok());
        }
    }
}
