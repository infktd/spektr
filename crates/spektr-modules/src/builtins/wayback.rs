//! `wayback`: first and last archive dates, and archived URLs (capped at
//! 50), from the Wayback Machine's CDX API.

use futures::future::BoxFuture;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, Manifest};

use super::{Out, fetch, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "wayback"
version     = "1.0.0"
description = "Wayback Machine: first and last archive dates, archived URLs"
runtime     = "native"
concurrency = 2
timeout     = "90s"
technique   = "passive"
auto        = false
consumes    = ["url", "domain"]
produces    = [["links_to", "url"]]
cache_ttl   = "7d"
rate_limit  = "1/s"
cost        = { class = "free" }
hosts       = ["web.archive.org"]
"#;

const CDX: &str = "https://web.archive.org/cdx/search/cdx";
const MAX_LINKS: usize = 50;

pub struct Wayback {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Wayback {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net
            .http
            .for_module("wayback", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

/// `20190302174011` → RFC 3339.
fn cdx_time(ts: &str) -> Option<String> {
    if ts.len() < 14 || !ts.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let s = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &ts[0..4],
        &ts[4..6],
        &ts[6..8],
        &ts[8..10],
        &ts[10..12],
        &ts[12..14]
    );
    s.parse::<jiff::Timestamp>().ok().map(|t| t.to_string())
}

/// CDX JSON rows (`[["timestamp","original"], [..], ..]`), header dropped.
fn rows(body: &str) -> Vec<(String, String)> {
    let v: Vec<Vec<String>> = serde_json::from_str(body).unwrap_or_default();
    v.into_iter()
        .skip(1)
        .filter_map(|r| Some((r.first()?.clone(), r.get(1)?.clone())))
        .collect()
}

impl Module for Wayback {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let target = match input.entity.ty.as_str() {
                "url" => input.entity.key.clone(),
                _ => format!("{}/*", input.entity.key),
            };
            let q = |extra: &str| {
                format!("{CDX}?url={target}&output=json&fl=timestamp,original{extra}")
            };
            let Some(first) = fetch(&self.http, &out, "o1", &q("&limit=1"), &[]).await? else {
                return out.progress(100, "not archived").await;
            };
            let first = rows(&first.text());
            let Some((t0, _)) = first.first() else {
                return out.progress(100, "not archived").await;
            };
            if let Some(t) = cdx_time(t0) {
                out.attr("$input", "archived_first", AttrValue::Time(t), true, "o1")
                    .await?;
            }
            if let Some(last) = fetch(&self.http, &out, "o2", &q("&limit=-1"), &[]).await?
                && let Some((t1, _)) = rows(&last.text()).first()
                && let Some(t) = cdx_time(t1)
            {
                out.attr("$input", "archived_last", AttrValue::Time(t), true, "o2")
                    .await?;
            }
            let mut n = 0;
            if let Some(list) = fetch(
                &self.http,
                &out,
                "o3",
                &q(&format!("&collapse=urlkey&limit={MAX_LINKS}")),
                &[],
            )
            .await?
            {
                for (i, (ts, orig)) in rows(&list.text()).iter().take(MAX_LINKS).enumerate() {
                    let r = format!("a{i}");
                    out.entity(
                        &r,
                        "url",
                        &format!("https://web.archive.org/web/{ts}/{orig}"),
                        "o3",
                    )
                    .await?;
                    out.edge("$input", "links_to", &r, "o3", Default::default())
                        .await?;
                    n += 1;
                }
            }
            out.progress(
                100,
                &format!("archived since {} · {n} URLs", &t0[..4.min(t0.len())]),
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cdx_parsing() {
        assert_eq!(
            super::cdx_time("20190302174011").as_deref(),
            Some("2019-03-02T17:40:11Z")
        );
        assert_eq!(super::cdx_time("2019"), None);
        let r = super::rows(
            r#"[["timestamp","original"],["20190302174011","https://jmarlow.example/"]]"#,
        );
        assert_eq!(
            r,
            [(
                "20190302174011".to_string(),
                "https://jmarlow.example/".to_string()
            )]
        );
    }
}
