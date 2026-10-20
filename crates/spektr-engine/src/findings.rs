//! Findings (PRD §4, TUI_SPEC §5): the analyst's claims, their supporting
//! entities and evidence, and the submission checks from the case's rules
//! profile.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde::Serialize;
use spektr_model::{Actor, EntityId, ObservationId, Strength};
use spektr_store::{EntitySummary, FindingRow, FindingStatus, FindingUpdate, Reader, StoreResult};

use crate::engine::Shared;
use crate::error::{EngineError, EngineResult};
use crate::events::EventKind;
use crate::handle::ObservationView;
use crate::profile::RulesProfile;

/// Titles start as the claim's first 60 characters.
pub const TITLE_LEN: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckLevel {
    Pass,
    /// Doesn't block `ready`: a reminder (approval needed).
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub level: CheckLevel,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CategoryView {
    pub id: String,
    pub name: String,
    pub points: Option<u32>,
    pub needs_approval: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingSummary {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub status: String,
    /// The strongest supporting entity's strength.
    pub strength: Option<Strength>,
    pub updated_at: jiff::Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvidenceItem {
    pub observation: ObservationView,
    /// A capture's or response's URL.
    pub url: Option<String>,
    /// A capture's screenshot blob (SHA-256 hex).
    pub screenshot: Option<String>,
    pub audit_seq: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingDetail {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub claim: String,
    pub relevance: String,
    pub status: String,
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
    pub entities: Vec<(EntitySummary, Option<Strength>)>,
    pub evidence: Vec<EvidenceItem>,
    pub checks: Vec<Check>,
}

impl FindingDetail {
    /// Nothing fails: the finding may be marked ready.
    pub fn passes(&self) -> bool {
        self.checks.iter().all(|c| c.level != CheckLevel::Fail)
    }
}

/// The findings list (Findings screen, `query --findings`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingsView {
    pub categories: Vec<CategoryView>,
    pub findings: Vec<FindingSummary>,
}

pub fn categories(p: &RulesProfile) -> Vec<CategoryView> {
    p.categories
        .iter()
        .map(|c| CategoryView {
            id: c.id.clone(),
            name: c.name.clone(),
            points: c.points,
            needs_approval: c.needs_approval.clone(),
        })
        .collect()
}

/// A category's display name, or its id when the profile doesn't know it.
pub fn category_name(p: &RulesProfile, id: &str) -> String {
    p.categories
        .iter()
        .find(|c| c.id == id)
        .map_or_else(|| id.to_string(), |c| c.name.clone())
}

/// A new finding's title: the claim's first 60 characters, or
/// `<category> · <entity value>` while the claim is empty.
pub fn auto_title(claim: &str, category_name: &str, entity: Option<&str>) -> String {
    let claim = claim.trim();
    if !claim.is_empty() {
        let mut t: String = claim.chars().take(TITLE_LEN).collect();
        if claim.chars().count() > TITLE_LEN {
            t = t.trim_end().to_string();
            t.push('…');
        }
        return t;
    }
    match entity {
        Some(v) => format!("{category_name} · {v}"),
        None => category_name.to_string(),
    }
}

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let h = rest.split(['/', '?', '#']).next()?;
    let h = h.rsplit_once('@').map_or(h, |(_, h)| h);
    let h = h.split(':').next()?;
    Some(h.to_ascii_lowercase())
}

/// Whether `url`'s host is, or is under, one of `domains`.
fn blocked_host(url: &str, domains: &[String]) -> Option<String> {
    let h = host(url)?;
    domains
        .iter()
        .find(|d| h == **d || h.ends_with(&format!(".{d}")))
        .map(|_| h)
}

/// The submission checks (PRD §4 "Findings").
pub fn checks(
    p: &RulesProfile,
    f: &FindingRow,
    evidence: &[EvidenceItem],
    others: &[(FindingRow, Vec<ObservationId>)],
) -> Vec<Check> {
    let mut out = Vec::new();
    let pass = |t: &str| Check {
        level: CheckLevel::Pass,
        text: t.to_string(),
    };
    let fail = |t: String| Check {
        level: CheckLevel::Fail,
        text: t,
    };
    let l = &p.lints;
    let claim = !f.claim.trim().is_empty();
    let relevance = !f.relevance.trim().is_empty();
    match (l.require_claim && !claim, l.require_relevance && !relevance) {
        (false, false) if claim && relevance => out.push(pass("claim and relevance written")),
        (false, false) if claim => out.push(pass("claim written")),
        (false, false) => {}
        (true, true) => out.push(fail("write the claim and the relevance".into())),
        (true, false) => out.push(fail("write the claim".into())),
        (false, true) => out.push(fail("write the relevance".into())),
    }
    let urls = evidence.iter().filter(|e| e.url.is_some()).count();
    let shots = evidence.iter().filter(|e| e.screenshot.is_some()).count();
    if l.require_url_or_screenshot {
        if urls + shots == 0 {
            out.push(fail("attach a capture or a response with a URL".into()));
        } else {
            let captures = evidence
                .iter()
                .filter(|e| e.url.is_some() && e.screenshot.is_some())
                .count();
            out.push(pass(&if captures > 0 {
                format!(
                    "{captures} capture{} with URL + screenshot",
                    if captures == 1 { "" } else { "s" }
                )
            } else {
                format!("{urls} source URL{}", if urls == 1 { "" } else { "s" })
            }));
        }
    }
    if !p.blocked_source_domains.is_empty() {
        let bad: BTreeSet<String> = evidence
            .iter()
            .filter_map(|e| e.url.as_deref())
            .filter_map(|u| blocked_host(u, &p.blocked_source_domains))
            .collect();
        if bad.is_empty() {
            out.push(pass("no news or aggregator sources"));
        } else {
            out.push(fail(format!(
                "{} is a news or aggregator source",
                bad.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    }
    if l.no_duplicate_across_categories {
        let mine: BTreeSet<ObservationId> = evidence.iter().map(|e| e.observation.id).collect();
        let clash = others.iter().find(|(o, ev)| {
            o.id != f.id
                && o.category != f.category
                && ((!f.claim.trim().is_empty() && o.claim.trim() == f.claim.trim())
                    || ev.iter().any(|x| mine.contains(x)))
        });
        match clash {
            Some((o, _)) => out.push(fail(format!(
                "also filed under {} (F-{})",
                category_name(p, &o.category),
                o.id
            ))),
            None => out.push(pass("not used in another category")),
        }
    }
    if let Some(who) = p
        .categories
        .iter()
        .find(|c| c.id == f.category)
        .and_then(|c| c.needs_approval.as_deref())
    {
        out.push(Check {
            level: CheckLevel::Warn,
            text: format!("needs {who} approval"),
        });
    }
    if !p.categories.is_empty() && !p.categories.iter().any(|c| c.id == f.category) {
        out.push(fail(format!(
            "`{}` isn't a category of the {} profile",
            f.category, p.name
        )));
    }
    out
}

fn evidence_item(r: &Reader<'_>, id: ObservationId) -> StoreResult<Option<EvidenceItem>> {
    let Some(o) = r.observation(id)? else {
        return Ok(None);
    };
    let url = o
        .summary
        .get("final_url")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| o.source_url.clone());
    let screenshot = o
        .summary
        .get("screenshot")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let audit_seq = r.observation_audit_seq(id)?;
    Ok(Some(EvidenceItem {
        observation: o.into(),
        url,
        screenshot,
        audit_seq,
    }))
}

/// Every finding with its evidence ids (for the duplicate check).
fn all_with_evidence(r: &Reader<'_>) -> StoreResult<Vec<(FindingRow, Vec<ObservationId>)>> {
    r.findings()?
        .into_iter()
        .map(|f| {
            let ev = r.finding_evidence(f.id)?;
            Ok((f, ev))
        })
        .collect()
}

pub(crate) fn detail_in(
    r: &Reader<'_>,
    p: &RulesProfile,
    id: i64,
) -> StoreResult<Option<FindingDetail>> {
    let Some(f) = r.finding(id)? else {
        return Ok(None);
    };
    let mut entities = Vec::new();
    for e in r.finding_entities(id)? {
        let root = r.resolve(e)?;
        if let Some(s) = r.summary(root)? {
            let st = r.entity_strength(root)?;
            entities.push((s, st));
        }
    }
    let mut evidence = Vec::new();
    for o in r.finding_evidence(id)? {
        if let Some(e) = evidence_item(r, o)? {
            evidence.push(e);
        }
    }
    evidence.sort_by_key(|e| e.observation.created_at);
    let others = all_with_evidence(r)?;
    let checks = checks(p, &f, &evidence, &others);
    Ok(Some(FindingDetail {
        id: f.id,
        title: f.title,
        category: f.category,
        claim: f.claim,
        relevance: f.relevance,
        status: f.status.as_str().to_string(),
        created_at: f.created_at,
        updated_at: f.updated_at,
        entities,
        evidence,
        checks,
    }))
}

pub(crate) fn list_in(r: &Reader<'_>, p: &RulesProfile) -> StoreResult<FindingsView> {
    let mut findings = Vec::new();
    for f in r.findings()? {
        let mut strength = None;
        for e in r.finding_entities(f.id)? {
            strength = strength.max(r.entity_strength(r.resolve(e)?)?);
        }
        findings.push(FindingSummary {
            id: f.id,
            title: f.title,
            category: f.category,
            status: f.status.as_str().to_string(),
            strength,
            updated_at: f.updated_at,
        });
    }
    Ok(FindingsView {
        categories: categories(p),
        findings,
    })
}

pub(crate) async fn list(shared: &Arc<Shared>) -> EngineResult<FindingsView> {
    let p = shared.profile()?;
    Ok(shared.store.read(move |r| list_in(r, &p)).await?)
}

pub(crate) async fn detail(shared: &Arc<Shared>, id: i64) -> EngineResult<Option<FindingDetail>> {
    let p = shared.profile()?;
    Ok(shared.store.read(move |r| detail_in(r, &p, id)).await?)
}

/// A new finding (`f` on an entity, `n` on the Findings screen, `f` on a
/// checklist item). The entities' captures are attached as evidence.
pub struct NewFinding {
    pub category: String,
    pub claim: String,
    pub relevance: String,
    pub title: Option<String>,
    pub entities: Vec<EntityId>,
    pub evidence: Vec<ObservationId>,
}

pub(crate) async fn create(shared: &Arc<Shared>, n: NewFinding) -> EngineResult<i64> {
    let p = shared.profile()?;
    if !p.categories.is_empty() && !p.categories.iter().any(|c| c.id == n.category) {
        return Err(EngineError::Invalid(format!(
            "`{}` isn't a category of the {} profile",
            n.category, p.name
        )));
    }
    let cat_name = category_name(&p, &n.category);
    let id = shared
        .store
        .write(move |tx| {
            let act = Actor::Analyst;
            let first = match n.entities.first() {
                Some(e) => tx
                    .reader()
                    .summary(tx.reader().resolve(*e)?)?
                    .map(|s| s.value),
                None => None,
            };
            let title = n
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| auto_title(&n.claim, &cat_name, first.as_deref()));
            let id = tx.add_finding(&act, &title, &n.category, &n.claim, &n.relevance)?;
            let mut evidence: BTreeSet<ObservationId> = n.evidence.iter().copied().collect();
            for e in &n.entities {
                let root = tx.reader().resolve(*e)?;
                tx.finding_entity(&act, id, root, true)?;
                for o in tx.reader().entity_observations(root)? {
                    if o.kind == spektr_store::ObservationKind::Capture {
                        evidence.insert(o.id);
                    }
                }
            }
            for o in evidence {
                tx.finding_evidence(&act, id, o, true)?;
            }
            Ok(id)
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(id)
}

/// A change to a finding. A title that still reads as the automatic one
/// follows the claim; once edited by hand it stays as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingEdit {
    pub title: Option<String>,
    pub category: Option<String>,
    pub claim: Option<String>,
    pub relevance: Option<String>,
}

pub(crate) async fn edit(shared: &Arc<Shared>, id: i64, e: FindingEdit) -> EngineResult<()> {
    let p = shared.profile()?;
    if let Some(c) = &e.category
        && !p.categories.is_empty()
        && !p.categories.iter().any(|x| &x.id == c)
    {
        return Err(EngineError::Invalid(format!(
            "`{c}` isn't a category of the {} profile",
            p.name
        )));
    }
    shared
        .store
        .write(move |tx| {
            let r = tx.reader();
            let f = r
                .finding(id)?
                .ok_or_else(|| spektr_store::StoreError::NotFound(format!("finding F-{id}")))?;
            let first = match r.finding_entities(id)?.first() {
                Some(x) => r.summary(r.resolve(*x)?)?.map(|s| s.value),
                None => None,
            };
            let auto_before =
                auto_title(&f.claim, &category_name(&p, &f.category), first.as_deref());
            let mut title = e.title.clone();
            if title.is_none()
                && f.title == auto_before
                && (e.claim.is_some() || e.category.is_some())
            {
                let claim = e.claim.as_deref().unwrap_or(&f.claim);
                let cat = e.category.as_deref().unwrap_or(&f.category);
                let t = auto_title(claim, &category_name(&p, cat), first.as_deref());
                if t != f.title {
                    title = Some(t);
                }
            }
            tx.update_finding(
                &Actor::Analyst,
                id,
                &FindingUpdate {
                    title,
                    category: e.category.clone(),
                    claim: e.claim.clone(),
                    relevance: e.relevance.clone(),
                    status: None,
                },
            )
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(())
}

/// `ready` needs every check to pass; `sent` needs `ready` first.
pub(crate) async fn set_status(
    shared: &Arc<Shared>,
    id: i64,
    status: FindingStatus,
) -> EngineResult<()> {
    let d = detail(shared, id)
        .await?
        .ok_or_else(|| EngineError::Invalid(format!("no finding F-{id}")))?;
    match status {
        FindingStatus::Ready if !d.passes() => {
            let first = d
                .checks
                .iter()
                .find(|c| c.level == CheckLevel::Fail)
                .map(|c| c.text.clone())
                .unwrap_or_default();
            return Err(EngineError::Invalid(format!("checks fail: {first}")));
        }
        FindingStatus::Sent if d.status == "draft" => {
            return Err(EngineError::Invalid("mark it ready first".into()));
        }
        _ => {}
    }
    shared
        .store
        .write(move |tx| {
            tx.update_finding(
                &Actor::Analyst,
                id,
                &FindingUpdate {
                    status: Some(status),
                    ..Default::default()
                },
            )
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(())
}

pub(crate) async fn delete(shared: &Arc<Shared>, id: i64) -> EngineResult<()> {
    shared
        .store
        .write(move |tx| tx.delete_finding(&Actor::Analyst, id))
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(())
}

/// Attach or detach an entity (its captures come along when attached).
pub(crate) async fn link_entity(
    shared: &Arc<Shared>,
    id: i64,
    entity: EntityId,
    attach: bool,
) -> EngineResult<()> {
    shared
        .store
        .write(move |tx| {
            let root = tx.reader().resolve(entity)?;
            tx.finding_entity(&Actor::Analyst, id, root, attach)?;
            if attach {
                for o in tx.reader().entity_observations(root)? {
                    if o.kind == spektr_store::ObservationKind::Capture {
                        tx.finding_evidence(&Actor::Analyst, id, o.id, true)?;
                    }
                }
            }
            Ok(())
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(())
}

pub(crate) async fn link_evidence(
    shared: &Arc<Shared>,
    id: i64,
    obs: ObservationId,
    attach: bool,
) -> EngineResult<()> {
    shared
        .store
        .write(move |tx| {
            tx.finding_evidence(&Actor::Analyst, id, obs, attach)
                .map(|_| ())
        })
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "finding".into(),
    });
    Ok(())
}

/// Evidence `a` can attach: the supporting entities' observations not yet
/// attached, captures first, then newest first.
pub(crate) async fn candidates(
    shared: &Arc<Shared>,
    id: i64,
) -> EngineResult<Vec<ObservationView>> {
    Ok(shared
        .store
        .read(move |r| {
            let have: BTreeSet<ObservationId> = r.finding_evidence(id)?.into_iter().collect();
            let mut out: Vec<spektr_store::ObservationRow> = Vec::new();
            for e in r.finding_entities(id)? {
                for o in r.entity_observations(r.resolve(e)?)? {
                    if !have.contains(&o.id) && !out.iter().any(|x| x.id == o.id) {
                        out.push(o);
                    }
                }
            }
            out.sort_by_key(|o| {
                (
                    o.kind != spektr_store::ObservationKind::Capture,
                    std::cmp::Reverse(o.created_at),
                )
            });
            Ok(out.into_iter().map(Into::into).collect())
        })
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            auto_title("", "Advancing the Timeline", Some("github:jmarlow-wav")),
            "Advancing the Timeline · github:jmarlow-wav"
        );
        assert_eq!(auto_title("  short claim ", "X", None), "short claim");
        let long = "Subject's GitHub account jmarlow-wav was active on 2026-10-16, 18 days after the last-seen date.";
        let t = auto_title(long, "X", None);
        assert!(
            t.ends_with('…') && t.chars().count() <= TITLE_LEN + 1,
            "{t}"
        );
    }

    #[test]
    fn hosts() {
        assert_eq!(
            host("https://News.Example.com:443/a?b").as_deref(),
            Some("news.example.com")
        );
        let d = vec!["example.com".to_string()];
        assert!(blocked_host("https://news.example.com/x", &d).is_some());
        assert!(blocked_host("https://notexample.com/x", &d).is_none());
    }
}
