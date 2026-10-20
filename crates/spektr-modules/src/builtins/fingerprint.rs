//! `http-fingerprint`: one GET of a page plus its favicon: title, server,
//! favicon hash, analytics ids, outbound social links, mailto addresses.

use futures::future::BoxFuture;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_model::PlatformRegistry;
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, host_of, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "http-fingerprint"
version     = "1.0.0"
description = "Page title, server, favicon hash, analytics ids, outbound social links and mailto addresses"
runtime     = "native"
concurrency = 4
timeout     = "60s"
technique   = "passive"
auto        = true
consumes    = ["url", "domain", "subdomain"]
produces    = [["links_to", "url"], ["has_email", "email"]]
cache_ttl   = "24h"
rate_limit  = "1/s"
cost        = { class = "free" }
"#;

pub struct Fingerprint {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Fingerprint {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("http-fingerprint", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// GA (`UA-…`, `G-…`), GTM and AdSense ids in a page.
pub fn analytics_ids(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pat in [
        r"\bUA-\d{4,10}-\d{1,4}\b",
        r"\bG-[A-Z0-9]{6,12}\b",
        r"\bGTM-[A-Z0-9]{4,10}\b",
        r"\bca-pub-\d{10,20}\b",
    ] {
        if let Ok(re) = regex::Regex::new(pat) {
            for m in re.find_iter(html) {
                let s = m.as_str().to_string();
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

struct Page {
    title: Option<String>,
    icon: Option<String>,
    /// The page declares an icon that isn't fetchable (`data:,` means "no
    /// favicon"), so `/favicon.ico` isn't tried either.
    no_icon: bool,
    social: Vec<String>,
    mailto: Vec<String>,
}

/// Hosts of the known platforms' profile URLs.
fn platform_hosts() -> &'static std::collections::BTreeSet<String> {
    static HOSTS: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    HOSTS.get_or_init(|| {
        PlatformRegistry::builtin()
            .iter()
            .filter_map(|p| p.profile_url.as_deref().map(host_of))
            .filter(|h| !h.is_empty() && !h.contains('{'))
            .map(|h| h.trim_start_matches("www.").to_string())
            .collect()
    })
}

/// Whether a URL is a profile on a known platform (by its host).
fn is_social(url: &str) -> bool {
    let host = host_of(url);
    platform_hosts().contains(host.trim_start_matches("www."))
}

fn parse(html: &str, base: &str) -> Page {
    let doc = scraper::Html::parse_document(html);
    let sel = |s: &str| scraper::Selector::parse(s).ok();
    let title = sel("title")
        .and_then(|s| {
            doc.select(&s)
                .next()
                .map(|t| t.text().collect::<String>().trim().to_string())
        })
        .filter(|t| !t.is_empty());
    let base_url = url::Url::parse(base).ok();
    let abs = |h: &str| {
        base_url
            .as_ref()
            .and_then(|b| b.join(h).ok())
            .map(|u| u.to_string())
    };
    let declared = sel("link[rel~=icon]")
        .and_then(|s| {
            doc.select(&s)
                .find_map(|l| l.value().attr("href").map(str::to_string))
        })
        .and_then(|h| abs(&h));
    let fetchable = |u: &str| u.starts_with("https://") || u.starts_with("http://");
    let no_icon = declared.as_deref().is_some_and(|u| !fetchable(u));
    let icon = declared.filter(|u| fetchable(u));
    let mut social = Vec::new();
    let mut mailto = Vec::new();
    if let Some(s) = sel("a[href]") {
        for a in doc.select(&s) {
            let Some(h) = a.value().attr("href") else {
                continue;
            };
            if let Some(m) = h.strip_prefix("mailto:") {
                let addr = m.split('?').next().unwrap_or("").trim().to_string();
                if addr.contains('@') && !mailto.contains(&addr) {
                    mailto.push(addr);
                }
            } else if let Some(u) = abs(h)
                && host_of(&u) != host_of(base)
                && is_social(&u)
                && !social.contains(&u)
            {
                social.push(u);
            }
        }
    }
    Page {
        title,
        icon,
        no_icon,
        social,
        mailto,
    }
}

impl Module for Fingerprint {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let url = match input.entity.ty.as_str() {
                "url" => input.entity.key.clone(),
                _ => format!("https://{}/", input.entity.key),
            };
            let Some(resp) =
                fetch(&self.http, &out, "o1", &url, &[("accept", "text/html")]).await?
            else {
                return out.progress(100, "not found").await;
            };
            let html = resp.text();
            let page = parse(&html, &resp.final_url);
            let text = |s: &str| AttrValue::Text(s.to_string());
            if let Some(t) = &page.title {
                out.attr("$input", "title", text(t), false, "o1").await?;
            }
            if let Some(s) = resp.header("server") {
                out.attr("$input", "server", text(s), false, "o1").await?;
            }
            let ids = analytics_ids(&html);
            for id in &ids {
                out.attr("$input", "analytics_ids", text(id), false, "o1")
                    .await?;
            }
            for (i, s) in page.social.iter().enumerate() {
                let r = format!("s{i}");
                out.entity(&r, "url", s, "o1").await?;
                out.edge("$input", "links_to", &r, "o1", Default::default())
                    .await?;
            }
            for (i, m) in page.mailto.iter().enumerate() {
                let r = format!("m{i}");
                out.entity(&r, "email", m, "o1").await?;
                out.edge("$input", "has_email", &r, "o1", Default::default())
                    .await?;
            }
            let icon_url = page.icon.clone().filter(|_| !page.no_icon).or_else(|| {
                if page.no_icon {
                    return None;
                }
                url::Url::parse(&resp.final_url)
                    .ok()
                    .and_then(|u| u.join("/favicon.ico").ok())
                    .map(|u| u.to_string())
            });
            let mut fav = None;
            if let Some(iu) = icon_url
                && let Some(icon) =
                    fetch(&self.http, &out, "o2", &iu, &[("accept", "image/*")]).await?
                && !icon.body.is_empty()
            {
                let h = crate::media::favicon_hash(&icon.body);
                out.attr(
                    "$input",
                    "favicon_mmh3",
                    AttrValue::Int(i64::from(h)),
                    false,
                    "o2",
                )
                .await?;
                fav = Some(h);
            }
            let mut note = page
                .title
                .clone()
                .unwrap_or_else(|| format!("HTTP {}", resp.status));
            if !ids.is_empty() {
                note.push_str(&format!(
                    " · {} analytics id{}",
                    ids.len(),
                    if ids.len() == 1 { "" } else { "s" }
                ));
            }
            if fav.is_some() {
                note.push_str(" · favicon");
            }
            out.progress(100, &note).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_links() {
        let html = r#"<html><head><title> Jordan's notes </title><link rel="shortcut icon" href="/static/fav.png">
            <script>gtag('config','G-AB12CD34EF'); var x='UA-12345678-1';</script></head>
            <body><a href="https://github.com/jmarlow-wav">gh</a><a href="mailto:hello@jmarlow.example?subject=hi">mail</a>
            <a href="/about">about</a></body></html>"#;
        assert_eq!(analytics_ids(html), ["UA-12345678-1", "G-AB12CD34EF"]);
        let p = parse(html, "https://jmarlow.example/");
        assert_eq!(p.title.as_deref(), Some("Jordan's notes"));
        assert_eq!(
            p.icon.as_deref(),
            Some("https://jmarlow.example/static/fav.png")
        );
        assert_eq!(p.social, ["https://github.com/jmarlow-wav"]);
        assert_eq!(p.mailto, ["hello@jmarlow.example"]);
        assert!(!p.no_icon);
    }

    #[test]
    fn a_data_url_icon_means_no_favicon() {
        // example.com declares `<link rel="icon" href="data:,">` (seen live).
        let p = parse(
            r#"<html><head><link rel="icon" href="data:,"><title>Example</title></head></html>"#,
            "https://site.example/",
        );
        assert_eq!(p.icon, None);
        assert!(p.no_icon);
    }
}
