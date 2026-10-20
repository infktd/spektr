//! `spektr export <case> --format json`: the whole case as one JSON object.
//! `md` and `tracelabs` arrive with findings (Phase 5).

use std::io::Write;

use anyhow::{Result, bail};
use base64::Engine as _;
use serde_json::{Value, json};
use spektr_model::Strength;
use spektr_store::{Db, Sha256};

use crate::cli::{ExportArgs, ExportFormat};
use crate::wiring::Ctx;

pub const FORMAT_VERSION: &str = "spektr-case/1";

pub fn run(ctx: &Ctx, a: ExportArgs) -> Result<()> {
    if a.format != ExportFormat::Json {
        let name = if a.format == ExportFormat::Md {
            "md"
        } else {
            "tracelabs"
        };
        bail!("--format {name} arrives with findings (Phase 5); use --format json");
    }
    if a.with_blobs && a.format != ExportFormat::Json {
        bail!("--with-blobs is for --format json");
    }
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let mut db = Db::open_read_only(&a.case, ctx.store_options()?)?;
    let doc = db.read(|r| {
        let meta = r.meta_all()?;
        let evidence = r.all_evidence()?;
        let entities: Vec<Value> = r
            .all_entities()?
            .into_iter()
            .map(|e| json!({
                "id": e.id, "type": e.ty, "key": e.key, "value": e.value, "attrs": e.attrs,
                "flags": e.flags, "depth": e.depth, "merged_into": e.merged_into,
                "created_at": e.created_at, "created_by": e.created_by,
            }))
            .collect();
        let mut edges = Vec::new();
        for e in r.all_edges()? {
            let ev: Vec<&spektr_store::EvidenceRow> = evidence.iter().filter(|x| x.edge == e.id).collect();
            let keys: std::collections::BTreeSet<&str> = ev.iter().map(|x| x.source_key.as_str()).collect();
            let strength = (e.state == spektr_store::EdgeState::Active)
                .then(|| Strength::compute(keys.len(), e.confirmed_at.is_some()))
                .flatten();
            edges.push(json!({
                "id": e.id, "src": e.src, "dst": e.dst, "predicate": e.predicate, "discriminator": e.discriminator,
                "attrs": e.attrs, "state": e.state, "confirmed_at": e.confirmed_at, "origin": e.origin,
                "created_at": e.created_at, "strength": strength,
                "evidence": ev.iter().map(|x| json!({ "observation": x.observation, "source_key": x.source_key })).collect::<Vec<_>>(),
            }));
        }
        let mut observations = Vec::new();
        for o in r.all_observations()? {
            let mut v = json!({
                "id": o.id, "job": o.job_id, "module": o.module, "module_version": o.module_version,
                "manifest_hash": o.manifest_hash, "input_entity": o.input_entity, "source_url": o.source_url,
                "content_type": o.content_type, "blob_sha256": o.blob.map(|b: Sha256| b.hex()), "summary": o.summary,
                "kind": o.kind, "created_at": o.created_at,
            });
            if a.with_blobs
                && let Some(sha) = o.blob
                && let Some(bytes) = r.blob(sha)?
            {
                v["blob_b64"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
            }
            observations.push(v);
        }
        let findings: Vec<Value> = r
            .findings()?
            .into_iter()
            .filter(|f| a.findings.is_empty() || a.findings.contains(&f.id))
            .map(|f| {
                let ents = r.finding_entities(f.id).unwrap_or_default();
                let ev = r.finding_evidence(f.id).unwrap_or_default();
                json!({ "id": f.id, "title": f.title, "category": f.category, "claim": f.claim, "relevance": f.relevance,
                        "status": f.status, "created_at": f.created_at, "updated_at": f.updated_at, "entities": ents, "evidence": ev })
            })
            .collect();
        let notes: Vec<Value> = r.all_notes()?.into_iter().map(|n| json!({ "id": n.id, "entity": n.entity, "finding": n.finding, "body": n.body, "created_at": n.created_at })).collect();
        let review: Vec<Value> = r.review_items(None)?.into_iter().map(|i| json!({ "id": i.id, "kind": i.kind, "state": i.state, "payload": i.payload, "created_at": i.created_at, "decided_at": i.decided_at })).collect();
        let scope: Vec<Value> = r.scope_rules()?.into_iter().map(|s| json!({ "id": s.id, "kind": s.kind, "matcher": s.matcher, "pattern": s.pattern })).collect();
        let jobs: Vec<Value> = r.jobs(&[])?.into_iter().map(|j| json!({ "id": j.id, "module": j.module, "input": j.input_entity, "trigger": j.trigger, "state": j.state, "reason": j.reason, "error": j.error, "queued_at": j.queued_at, "finished_at": j.finished_at })).collect();
        let merges: Vec<Value> = r.merges()?.into_iter().map(|m| json!({ "id": m.id, "survivor": m.survivor, "merged": m.merged, "repointed": m.repointed, "created_at": m.created_at, "undone_at": m.undone_at })).collect();
        Ok(json!({
            "format": FORMAT_VERSION,
            "spektr": env!("CARGO_PKG_VERSION"),
            "meta": meta,
            "entities": entities,
            "edges": edges,
            "observations": observations,
            "findings": findings,
            "notes": notes,
            "review_items": review,
            "scope_rules": scope,
            "jobs": jobs,
            "merges": merges,
            "audit": { "rows": r.audit_count()? },
        }))
    })?;
    let text = serde_json::to_string_pretty(&doc)?;
    match a.output {
        Some(p) => std::fs::write(&p, format!("{text}\n"))?,
        None => {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{text}")?;
        }
    }
    Ok(())
}
