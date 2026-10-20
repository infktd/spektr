//! `hibp`: breaches an address appears in, from Have I Been Pwned API v3.
//! Names, dates and data classes only; never breach content.

use futures::future::BoxFuture;
use serde::Deserialize;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, EntitySpec, ErrorCode, Manifest};

use super::{Out, fetch, json_body, manifest};
use crate::net::{ModuleHttp, Net};

const MANIFEST: &str = r#"
[module]
name        = "hibp"
version     = "1.0.0"
description = "Breaches an email appears in (Have I Been Pwned): names, dates, data classes"
runtime     = "native"
concurrency = 1
timeout     = "60s"
technique   = "passive"
auto        = false
consumes    = ["email"]
produces    = [["appears_in_breach", "breach"]]
cache_ttl   = "7d"
rate_limit  = "10/min"
cost        = { class = "keyed", daily_budget = 100 }
secrets     = ["HIBP_KEY"]
hosts       = ["haveibeenpwned.com"]
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Breach {
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    breach_date: Option<String>,
    #[serde(default)]
    data_classes: Vec<String>,
}

pub struct Hibp {
    manifest: Manifest,
    http: ModuleHttp,
}

impl Hibp {
    pub fn new(net: Net) -> Self {
        let manifest = manifest(MANIFEST);
        let http = net.http.for_module("hibp", manifest.rate_limit.as_deref());
        Self { manifest, http }
    }
}

impl Module for Hibp {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let Some(key) = cx.secrets.get("HIBP_KEY") else {
                return Err(ModuleError::code(ErrorCode::Auth, "HIBP_KEY is not set"));
            };
            let out = Out(&cx.emit);
            let url = format!(
                "https://haveibeenpwned.com/api/v3/breachedaccount/{}?truncateResponse=false",
                input.entity.key
            );
            let headers = [("hibp-api-key", key), ("user-agent", "spektr")];
            let resp = match fetch(&self.http, &out, "o1", &url, &headers).await {
                Ok(r) => r,
                Err(ModuleError::Code { message, .. }) if message.starts_with("HTTP 401") => {
                    return Err(ModuleError::code(
                        ErrorCode::Auth,
                        "HIBP rejected the API key",
                    ));
                }
                Err(e) => return Err(e),
            };
            let Some(resp) = resp else {
                return out.progress(100, "no breaches").await;
            };
            let breaches: Vec<Breach> = json_body(&resp, "HIBP breaches")?;
            for (i, b) in breaches.iter().enumerate() {
                let r = format!("b{i}");
                out.entity_with(
                    &r,
                    EntitySpec {
                        ty: "breach".into(),
                        value: b.title.clone().unwrap_or_else(|| b.name.clone()),
                        blob: None,
                        dedup_id: None,
                        key: Some(format!("hibp:{}", b.name.to_ascii_lowercase())),
                    },
                    "o1",
                )
                .await?;
                out.edge("$input", "appears_in_breach", &r, "o1", Default::default())
                    .await?;
                if let Some(d) = &b.breach_date {
                    out.attr(&r, "breach_date", AttrValue::Date(d.clone()), true, "o1")
                        .await?;
                }
                if let Some(d) = &b.domain {
                    out.attr(&r, "domain", AttrValue::Text(d.clone()), false, "o1")
                        .await?;
                }
                if !b.data_classes.is_empty() {
                    out.attr(
                        &r,
                        "data_classes",
                        AttrValue::Text(b.data_classes.join(", ")),
                        false,
                        "o1",
                    )
                    .await?;
                }
            }
            out.progress(
                100,
                &format!(
                    "{} breach{}",
                    breaches.len(),
                    if breaches.len() == 1 { "" } else { "es" }
                ),
            )
            .await
        })
    }
}
