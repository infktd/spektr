//! `exif`: device, software, the time taken and GPS from an image's bytes.

use futures::future::BoxFuture;
use serde_json::json;
use spektr_engine::{JobInput, Module, ModuleCx, ModuleError};
use spektr_protocol::{AttrValue, ErrorCode, Manifest};

use super::{Out, manifest};

const MANIFEST: &str = r#"
[module]
name        = "exif"
version     = "1.0.0"
description = "EXIF metadata: device, software, time taken, GPS location"
runtime     = "native"
concurrency = 4
timeout     = "30s"
technique   = "passive"
auto        = true
consumes    = ["image"]
produces    = [["taken_at", "location"]]
cache_ttl   = "30d"
cost        = { class = "local" }
"#;

pub struct Exif {
    manifest: Manifest,
}

impl Exif {
    pub fn new() -> Self {
        Self {
            manifest: manifest(MANIFEST),
        }
    }
}

impl Module for Exif {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>> {
        Box::pin(async move {
            let out = Out(&cx.emit);
            let Some(bytes) = input.blob.clone() else {
                return Err(ModuleError::code(
                    ErrorCode::Internal,
                    "no image bytes in the job input",
                ));
            };
            let info = match crate::media::exif(&bytes) {
                Ok(i) => i,
                Err(e) => {
                    out.observation(
                        "o1",
                        None,
                        None,
                        None,
                        Some(json!({ "exif": false, "reason": e })),
                    )
                    .await?;
                    return out.progress(100, "no EXIF").await;
                }
            };
            let summary = json!({
                "device": info.device, "software": info.software, "taken_at": info.taken_at,
                "gps": info.gps.map(|(a, b)| [a, b]),
            });
            out.observation(
                "o1",
                None,
                Some("application/json"),
                Some(summary.to_string()),
                Some(summary),
            )
            .await?;
            if let Some(d) = &info.device {
                out.attr("$input", "device", AttrValue::Text(d.clone()), false, "o1")
                    .await?;
            }
            if let Some(s) = &info.software {
                out.attr(
                    "$input",
                    "software",
                    AttrValue::Text(s.clone()),
                    false,
                    "o1",
                )
                .await?;
            }
            if let Some(t) = &info.taken_at {
                out.attr("$input", "taken_at", AttrValue::Time(t.clone()), true, "o1")
                    .await?;
            }
            let mut note = info.device.clone().unwrap_or_else(|| "EXIF".into());
            if let Some((lat, lon)) = info.gps {
                out.attr("$input", "gps", AttrValue::Geo { lat, lon }, false, "o1")
                    .await?;
                out.entity("loc", "location", &format!("{lat:.5}, {lon:.5}"), "o1")
                    .await?;
                out.edge("$input", "taken_at", "loc", "o1", Default::default())
                    .await?;
                note = format!("{note} · {lat:.2}, {lon:.2}");
            }
            out.progress(100, &note).await
        })
    }
}
