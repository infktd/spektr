//! Exports (PRD §4): paste-ready Trace Labs submission text, a Markdown
//! report, and the whole case as JSON. Pure functions of a store reader,
//! so the CLI (read-only, no engine) and the TUI share them.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spektr_model::{EntityId, Strength};
use spektr_store::{Reader, Sha256, StoreResult};

use crate::findings::{CheckLevel, FindingDetail, category_name, detail_in};
use crate::profile::RulesProfile;

/// The JSON export's format tag.
pub const FORMAT_VERSION: &str = "spektr-case/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Tracelabs,
    Md,
    Json,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "tracelabs" => Format::Tracelabs,
            "md" | "markdown" => Format::Md,
            "json" => Format::Json,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Format::Tracelabs => "tracelabs",
            Format::Md => "md",
            Format::Json => "json",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Tracelabs => "txt",
            Format::Md => "md",
            Format::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Only these findings; all when empty.
    pub findings: Vec<i64>,
    /// JSON only: blob bytes inline (base64).
    pub with_blobs: bool,
}

pub fn render(
    r: &Reader<'_>,
    p: &RulesProfile,
    format: Format,
    o: &Options,
) -> StoreResult<String> {
    Ok(match format {
        Format::Tracelabs => tracelabs(r, p, o)?,
        Format::Md => markdown(r, p, o)?,
        Format::Json => {
            let mut t = serde_json::to_string_pretty(&json_doc(r, o)?)?;
            t.push('\n');
            t
        }
    })
}

/// The selected findings, in the profile's category order, then by id.
fn selected(r: &Reader<'_>, p: &RulesProfile, o: &Options) -> StoreResult<Vec<FindingDetail>> {
    let mut out = Vec::new();
    for f in r.findings()? {
        if !o.findings.is_empty() && !o.findings.contains(&f.id) {
            continue;
        }
        if let Some(d) = detail_in(r, p, f.id)? {
            out.push(d);
        }
    }
    let rank = |c: &str| {
        p.categories
            .iter()
            .position(|x| x.id == c)
            .unwrap_or(usize::MAX)
    };
    out.sort_by_key(|d| (rank(&d.category), d.id));
    Ok(out)
}

/// `1,000`.
pub fn thousands(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn category_line(p: &RulesProfile, id: &str) -> String {
    let c = p.categories.iter().find(|c| c.id == id);
    let name = category_name(p, id);
    match c.and_then(|c| c.points) {
        Some(pts) => format!("{name} ({} pts)", thousands(pts)),
        None => name,
    }
}

fn utc(t: jiff::Timestamp) -> String {
    t.strftime("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Paste-ready Trace Labs submissions: one block per finding.
fn tracelabs(r: &Reader<'_>, p: &RulesProfile, o: &Options) -> StoreResult<String> {
    let mut out = String::new();
    for (i, d) in selected(r, p, o)?.iter().enumerate() {
        if i > 0 {
            out.push_str("\n---\n\n");
        }
        let _ = writeln!(out, "Category: {}", category_line(p, &d.category));
        let _ = writeln!(out, "Claim: {}", d.claim.trim());
        let _ = writeln!(out, "Relevance: {}", d.relevance.trim());
        let mut urls = BTreeSet::new();
        for e in &d.evidence {
            if let Some(u) = &e.url
                && urls.insert(u.clone())
            {
                let _ = writeln!(out, "URL: {u}");
            }
        }
        for e in d.evidence.iter().filter(|e| e.screenshot.is_some()) {
            let sha = e.screenshot.as_deref().unwrap_or_default();
            let _ = writeln!(
                out,
                "Screenshot: {} · sha256 {sha} · captured {}",
                e.url.as_deref().unwrap_or("capture"),
                utc(e.observation.created_at),
            );
        }
        for c in d.checks.iter().filter(|c| c.level == CheckLevel::Warn) {
            let _ = writeln!(out, "Note: {}", c.text);
        }
    }
    Ok(out)
}

fn strength_label(s: Option<Strength>) -> &'static str {
    s.map_or("—", |s| s.as_str())
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// A Markdown report of the findings, with their evidence and checks.
fn markdown(r: &Reader<'_>, p: &RulesProfile, o: &Options) -> StoreResult<String> {
    let meta = r.meta_all()?;
    let s = |k: &str| {
        meta.get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut out = String::new();
    let _ = writeln!(out, "# {}: findings\n", s("name"));
    let subject = match meta
        .get("subject_entity")
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<EntityId>().ok())
    {
        Some(id) => r.summary(r.resolve(id)?)?.map(|e| e.value),
        None => None,
    };
    let mut facts = vec![format!("Rules profile **{}**", p.name)];
    if let Some(sub) = subject {
        facts.push(format!("subject **{sub}**"));
    }
    if !s("last_seen").is_empty() {
        facts.push(format!("last seen {}", s("last_seen")));
    }
    facts.push(format!("audit log {} rows", r.audit_count()?));
    let _ = writeln!(out, "{}\n", facts.join(" · "));
    let list = selected(r, p, o)?;
    if list.is_empty() {
        out.push_str("No findings.\n");
        return Ok(out);
    }
    let mut last_cat: Option<String> = None;
    for d in &list {
        if last_cat.as_deref() != Some(&d.category) {
            let _ = writeln!(out, "## {}\n", category_line(p, &d.category));
            last_cat = Some(d.category.clone());
        }
        let _ = writeln!(out, "### F-{} · {} ({})\n", d.id, d.title, d.status);
        let _ = writeln!(out, "**Claim.** {}\n", d.claim.trim());
        let _ = writeln!(out, "**Relevance.** {}\n", d.relevance.trim());
        if !d.entities.is_empty() {
            out.push_str("**Supporting entities**\n\n");
            for (e, st) in &d.entities {
                let _ = writeln!(
                    out,
                    "- {} `{}` · {}",
                    e.ty.as_str(),
                    e.value,
                    strength_label(*st)
                );
            }
            out.push('\n');
        }
        if !d.evidence.is_empty() {
            out.push_str("**Evidence**\n\n| Source | URL | Recorded | SHA-256 | Audit row |\n|---|---|---|---|---|\n");
            for e in &d.evidence {
                let sha = e
                    .screenshot
                    .as_deref()
                    .or(e.observation.blob.as_deref())
                    .unwrap_or("—");
                let _ = writeln!(
                    out,
                    "| {} {} | {} | {} | `{}` | {} |",
                    md_cell(&e.observation.module),
                    md_cell(&e.observation.module_version),
                    md_cell(e.url.as_deref().unwrap_or("—")),
                    utc(e.observation.created_at),
                    sha,
                    e.audit_seq.map_or("—".to_string(), |s| format!("#{s}")),
                );
            }
            out.push('\n');
        }
        out.push_str("**Checks**\n\n");
        for c in &d.checks {
            let mark = match c.level {
                CheckLevel::Pass => "✓",
                CheckLevel::Warn => "!",
                CheckLevel::Fail => "✕",
            };
            let _ = writeln!(out, "- {mark} {}", c.text);
        }
        out.push('\n');
    }
    Ok(out)
}

/// The whole case as one JSON document (`spektr-case/1`).
pub fn json_doc(r: &Reader<'_>, opts: &Options) -> StoreResult<Value> {
    let meta = r.meta_all()?;
    let evidence = r.all_evidence()?;
    let entities: Vec<Value> = r
        .all_entities()?
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id, "type": e.ty, "key": e.key, "value": e.value, "attrs": e.attrs,
                "flags": e.flags, "depth": e.depth, "merged_into": e.merged_into,
                "created_at": e.created_at, "created_by": e.created_by,
            })
        })
        .collect();
    let mut edges = Vec::new();
    for e in r.all_edges()? {
        let ev: Vec<&spektr_store::EvidenceRow> =
            evidence.iter().filter(|x| x.edge == e.id).collect();
        let keys: std::collections::BTreeSet<&str> =
            ev.iter().map(|x| x.source_key.as_str()).collect();
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
        if opts.with_blobs
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
        .filter(|f| opts.findings.is_empty() || opts.findings.contains(&f.id))
        .map(|f| {
            let ents = r.finding_entities(f.id).unwrap_or_default();
            let ev = r.finding_evidence(f.id).unwrap_or_default();
            json!({ "id": f.id, "title": f.title, "category": f.category, "claim": f.claim, "relevance": f.relevance,
                    "status": f.status, "created_at": f.created_at, "updated_at": f.updated_at, "entities": ents, "evidence": ev })
        })
        .collect();
    let notes: Vec<Value> = r.all_notes()?.into_iter().map(|n| json!({ "id": n.id, "entity": n.entity, "finding": n.finding, "body": n.body, "created_at": n.created_at })).collect();
    let review: Vec<Value> = r.review_items(None)?.into_iter().map(|i| json!({ "id": i.id, "kind": i.kind, "state": i.state, "payload": i.payload, "created_at": i.created_at, "decided_at": i.decided_at })).collect();
    let scope: Vec<Value> = r
        .scope_rules()?
        .into_iter()
        .map(|s| json!({ "id": s.id, "kind": s.kind, "matcher": s.matcher, "pattern": s.pattern }))
        .collect();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separator() {
        assert_eq!(thousands(5), "5");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(5000), "5,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
