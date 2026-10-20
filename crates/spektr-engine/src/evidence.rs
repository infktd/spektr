//! Evidence through `EngineHandle`: web capture, attachments, review
//! decisions, raw observations and blobs.

use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use spektr_model::{
    Actor, CorePredicate, CoreType, EntityId, KeyHint, NormalizeCtx, ObservationId, Predicate,
    ReviewItemId, normalize,
};
use spektr_store::{
    EntitySummary, NewEdge, NewObservation, ObservationKind, ReviewKind, ReviewState,
};

use crate::capture::{CaptureRequest, MAX_CAPTURE};
use crate::engine::{Shared, read};
use crate::error::{EngineError, EngineResult};
use crate::events::EventKind;

/// A raw observation for `o` (the pager): its row and its bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RawView {
    pub observation: crate::handle::ObservationView,
    pub summary: serde_json::Value,
    pub bytes: Option<Vec<u8>>,
}

/// Image-ish magic bytes.
pub fn is_image(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG")
        || b.starts_with(b"\xff\xd8\xff")
        || b.starts_with(b"GIF8")
        || (b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
}

fn mime_for(name: &str, b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") {
        "image/png"
    } else if b.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if b.starts_with(b"GIF8") {
        "image/gif"
    } else if is_image(b) {
        "image/webp"
    } else if name.ends_with(".pdf") {
        "application/pdf"
    } else if name.ends_with(".html") || name.ends_with(".htm") {
        "text/html"
    } else if std::str::from_utf8(b).is_ok() {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}

/// Capture a page for an entity (`c`, `spektr capture`).
pub(crate) async fn capture(
    shared: &Arc<Shared>,
    entity: EntityId,
    url: String,
    screenshot: bool,
) -> EngineResult<ObservationId> {
    let svc = read!(shared.capture)
        .clone()
        .ok_or_else(|| EngineError::Invalid("web capture isn't available".into()))?;
    let got = svc
        .capture(CaptureRequest {
            url: url.clone(),
            screenshot,
        })
        .await
        .map_err(|e| EngineError::Invalid(format!("capture failed: {e}")))?;
    let size = (got.html.len() + got.screenshot.as_ref().map_or(0, Vec::len)) as u64;
    if size > MAX_CAPTURE {
        return Err(EngineError::Invalid(format!(
            "capture of {size} bytes exceeds the 25 MB limit"
        )));
    }
    let obs = shared
        .store
        .write(move |tx| {
            let act = Actor::Analyst;
            let root = tx.reader().resolve(entity)?;
            let html = tx.put_blob(&act, &got.html)?;
            let shot = match &got.screenshot {
                Some(png) => Some(tx.put_blob(&act, png)?),
                None => None,
            };
            let headers: serde_json::Map<String, serde_json::Value> = got
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            tx.add_observation(
                &act,
                NewObservation {
                    job: None,
                    module: "capture".into(),
                    module_version: env!("CARGO_PKG_VERSION").into(),
                    manifest_hash: "builtin".into(),
                    input_entity: Some(root),
                    source_url: Some(got.final_url.clone()),
                    content_type: Some("text/html".into()),
                    blob: Some(html),
                    summary: json!({
                        "url": url, "final_url": got.final_url, "status": got.status,
                        "headers": headers, "screenshot": shot.map(|s| s.hex()),
                        "screenshot_note": got.screenshot_note,
                    }),
                    kind: ObservationKind::Capture,
                },
            )
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "capture".into(),
    });
    Ok(obs)
}

/// Attach a local file as evidence (`spektr attach`, `:attach`). An image
/// also becomes an `image` entity (`posted` from an account, `depicts` to
/// a person: the analyst's assertion).
pub(crate) async fn attach(
    shared: &Arc<Shared>,
    entity: Option<EntityId>,
    name: String,
    bytes: Vec<u8>,
    note: Option<String>,
) -> EngineResult<(ObservationId, Option<EntitySummary>)> {
    let vocab = shared.vocab();
    let region = shared.settings().region;
    let out = shared
        .store
        .write(move |tx| {
            let act = Actor::Analyst;
            let target = match entity {
                Some(e) => Some(tx.reader().resolve(e)?),
                None => None,
            };
            let sha = tx.put_blob(&act, &bytes)?;
            let mime = mime_for(&name, &bytes);
            let obs = tx.add_observation(
                &act,
                NewObservation {
                    job: None,
                    module: "attachment".into(),
                    module_version: env!("CARGO_PKG_VERSION").into(),
                    manifest_hash: "builtin".into(),
                    input_entity: target,
                    source_url: None,
                    content_type: Some(mime.into()),
                    blob: Some(sha),
                    summary: json!({ "filename": name, "size": bytes.len(), "mime": mime, "note": note }),
                    kind: ObservationKind::Attachment,
                },
            )?;
            let mut image = None;
            if is_image(&bytes) {
                let hex = sha.hex();
                let n = normalize(
                    &CoreType::Image.into(),
                    &name,
                    KeyHint::Sha256(&hex),
                    NormalizeCtx { vocab: &vocab, default_region: Some(&region) },
                )?;
                let ing = tx.ingest_entity(&act, &n, 0, Some((obs, "attachment")))?;
                let img = tx.reader().resolve(ing.id)?;
                if let Some(t) = target {
                    let ty = tx.reader().entity(t)?.map(|e| e.ty);
                    let edge = match ty.and_then(|t| t.core()) {
                        Some(CoreType::Account) => Some((t, CorePredicate::Posted, img)),
                        Some(CoreType::Person) => Some((img, CorePredicate::Depicts, t)),
                        _ => None,
                    };
                    if let Some((src, pred, dst)) = edge {
                        let (e, _) = tx.upsert_edge(
                            &act,
                            NewEdge { src, predicate: Predicate::Core(pred), dst, discriminator: String::new(), attrs: Default::default(), origin: "analyst".into() },
                        )?;
                        tx.add_evidence(&act, e, obs, "analyst")?;
                    }
                }
                image = tx.reader().summary(img)?;
            }
            Ok((obs, image))
        })
        .await?;
    if let Some(img) = &out.1 {
        shared.emit(EventKind::EntityAdded {
            entity: img.clone(),
            by: "attachment".into(),
        });
        // A new image gets its auto modules (exif).
        let reqs = crate::plan::auto_requests(shared, &[(img.id, "image".into(), None)]);
        crate::plan::plan(shared, reqs).await?;
    }
    shared.emit(EventKind::CaseChanged {
        what: "attachment".into(),
    });
    Ok(out)
}

/// Decide a review item. Accepting a `sweep_unsure` hit adds the account
/// (`has_account` from the swept username, evidence: the sweep's response
/// and the analyst's decision). Accepting a `same_person` item only records
/// the decision; the merge is its own confirmed action.
pub(crate) async fn decide(
    shared: &Arc<Shared>,
    id: ReviewItemId,
    decision: ReviewState,
) -> EngineResult<Option<EntitySummary>> {
    let vocab = shared.vocab();
    let region = shared.settings().region;
    let added = shared
        .store
        .write(move |tx| {
            let act = Actor::Analyst;
            let item = tx.reader().review_item(id)?.ok_or_else(|| spektr_store::StoreError::NotFound(format!("review item {id}")))?;
            let mut added = None;
            if decision == ReviewState::Accepted && item.kind == ReviewKind::SweepUnsure {
                let d = &item.payload["detail"];
                let platform = d["platform"].as_str().unwrap_or_default();
                let handle = d["handle"].as_str().or(d["username"].as_str()).unwrap_or_default();
                let input: Option<EntityId> = serde_json::from_value(item.payload["input"].clone()).ok();
                let ev: Vec<ObservationId> = serde_json::from_value(item.payload["evidence"].clone()).unwrap_or_default();
                let obs = tx.add_observation(
                    &act,
                    NewObservation {
                        job: None,
                        module: "analyst".into(),
                        module_version: env!("CARGO_PKG_VERSION").into(),
                        manifest_hash: "builtin".into(),
                        input_entity: input,
                        source_url: d["url"].as_str().map(str::to_string),
                        content_type: None,
                        blob: None,
                        summary: json!({ "action": "review.accept", "review": id, "site": d["site"] }),
                        kind: ObservationKind::Analyst,
                    },
                )?;
                let n = normalize(
                    &CoreType::Account.into(),
                    &format!("{platform}:{handle}"),
                    KeyHint::None,
                    NormalizeCtx { vocab: &vocab, default_region: Some(&region) },
                )?;
                let depth = match input {
                    Some(i) => tx.reader().entity(i)?.map_or(1, |e| e.depth + 1),
                    None => 1,
                };
                let ing = tx.ingest_entity(&act, &n, depth, Some((obs, "analyst")))?;
                if let Some(src) = input {
                    let (e, _) = tx.upsert_edge(
                        &act,
                        NewEdge { src, predicate: Predicate::Core(CorePredicate::HasAccount), dst: ing.id, discriminator: String::new(), attrs: Default::default(), origin: "analyst".into() },
                    )?;
                    tx.add_evidence(&act, e, obs, "analyst")?;
                    for o in ev {
                        if let Some(row) = tx.reader().observation(o)? {
                            let key = row.source_url.as_deref().and_then(spektr_model::strength::source_key_for_url).unwrap_or_else(|| row.module.clone());
                            tx.add_evidence(&act, e, o, &key)?;
                        }
                    }
                }
                added = tx.reader().summary(tx.reader().resolve(ing.id)?)?;
            }
            tx.decide_review_item(&act, id, decision)?;
            Ok(added)
        })
        .await?;
    if let Some(a) = &added {
        shared.emit(EventKind::EntityAdded {
            entity: a.clone(),
            by: "analyst".into(),
        });
        let platform = a.key.split_once(':').map(|(p, _)| p.to_string());
        let reqs = crate::plan::auto_requests(shared, &[(a.id, "account".into(), platform)]);
        crate::plan::plan(shared, reqs).await?;
    }
    shared.emit(EventKind::CaseChanged {
        what: "review".into(),
    });
    Ok(added)
}

pub(crate) async fn raw(shared: &Arc<Shared>, id: ObservationId) -> EngineResult<Option<RawView>> {
    Ok(shared
        .store
        .read(move |r| {
            let Some(o) = r.observation(id)? else {
                return Ok(None);
            };
            let bytes = match o.blob {
                Some(sha) => r.blob(sha)?,
                None => None,
            };
            let summary = o.summary.clone();
            Ok(Some(RawView {
                observation: o.into(),
                summary,
                bytes,
            }))
        })
        .await?)
}
