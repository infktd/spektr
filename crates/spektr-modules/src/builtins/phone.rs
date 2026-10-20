//! `phone-parse`: country, line type and validity from libphonenumber's
//! metadata, offline.

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, ErrorCode, Manifest};

use super::{Out, manifest};

const MANIFEST: &str = r#"
[module]
name        = "phone-parse"
version     = "1.0.0"
description = "Phone number country, line type and validity (offline libphonenumber metadata)"
runtime     = "native"
concurrency = 8
timeout     = "10s"
technique   = "passive"
auto        = true
consumes    = ["phone"]
produces    = []
cache_ttl   = "30d"
cost        = { class = "local" }
"#;

pub struct PhoneParse {
    manifest: Manifest,
}

impl PhoneParse {
    pub fn new() -> Self {
        Self {
            manifest: manifest(MANIFEST),
        }
    }
}

fn line_type(t: phonenumber::Type) -> &'static str {
    use phonenumber::Type as T;
    match t {
        T::FixedLine => "fixed_line",
        T::Mobile => "mobile",
        T::FixedLineOrMobile => "fixed_line_or_mobile",
        T::TollFree => "toll_free",
        T::PremiumRate => "premium_rate",
        T::SharedCost => "shared_cost",
        T::PersonalNumber => "personal",
        T::Voip => "voip",
        T::Pager => "pager",
        T::Uan => "uan",
        T::Emergency => "emergency",
        T::Voicemail => "voicemail",
        T::ShortCode => "short_code",
        T::StandardRate => "standard_rate",
        T::Carrier => "carrier",
        T::NoInternational => "no_international",
        T::Unknown => "unknown",
    }
}

impl Module for PhoneParse {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            // The key is E.164 already (DATA_MODEL §1.1).
            let n = phonenumber::parse(None, &input.entity.key)
                .map_err(|e| ModuleError::code(ErrorCode::Parse, format!("phone number: {e}")))?;
            let valid = phonenumber::is_valid(&n);
            let region = n.country().id().map(|c| c.as_ref().to_string());
            let lt = line_type(n.number_type(&phonenumber::metadata::DATABASE));
            let summary = json!({ "valid": valid, "country": region, "line_type": lt });
            out.observation(
                "o1",
                None,
                Some("application/json"),
                Some(summary.to_string()),
                Some(summary.clone()),
            )
            .await?;
            out.attr("$input", "valid", AttrValue::Bool(valid), false, "o1")
                .await?;
            if let Some(r) = &region {
                out.attr("$input", "country", AttrValue::Text(r.clone()), false, "o1")
                    .await?;
            }
            out.attr(
                "$input",
                "line_type",
                AttrValue::Text(lt.into()),
                false,
                "o1",
            )
            .await?;
            out.progress(
                100,
                &format!(
                    "{} · {lt}{}",
                    region.as_deref().unwrap_or("??"),
                    if valid { "" } else { " · invalid" }
                ),
            )
            .await
        })
    }
}
