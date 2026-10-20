//! Playbooks, the checklist and launchers through `EngineHandle`
//! (docs/PLAYBOOKS.md §5, MODULES §8): view models for the TUI and CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::Serialize;
use spektr_model::{Actor, ChecklistItemId, EntityId, PlaybookRunId};
use spektr_protocol::playbook::{From, On, StepKind};
use spektr_protocol::{Manifest, PlaybookFile};
use spektr_store::{ChecklistItemRow, EntityRow, EntitySummary, ItemState};

use crate::engine::{Shared, read};
use crate::error::{EngineError, EngineResult};
use crate::events::EventKind;
use crate::launcher::Launchers;
use crate::playbook::{CheckCtx, Problems, Source};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepInfo {
    pub title: String,
    /// `run`, `playbook` or `check`.
    pub kind: String,
    /// `run github bluesky`, `playbook person-from-email`, the check text.
    pub detail: String,
    /// `seed`, `account from run`, `url from run, max 25`, `—`.
    pub on: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaybookInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub seed: Vec<String>,
    pub requires_profile: String,
    /// `built-in`, or the user file's path.
    pub source: String,
    pub steps: Vec<StepInfo>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepView {
    pub step: u32,
    pub info: StepInfo,
    pub state: String,
    pub selected: u32,
    pub capped: u32,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunView {
    pub id: PlaybookRunId,
    pub playbook: String,
    pub seed: Option<EntitySummary>,
    pub parent: Option<PlaybookRunId>,
    pub state: String,
    pub current_step: u32,
    pub total_steps: u32,
    pub started_at: jiff::Timestamp,
    pub finished_at: Option<jiff::Timestamp>,
    pub steps: Vec<StepView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LauncherView {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    /// Why it can't open (a local-only image has no URL).
    pub disabled: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemView {
    pub id: ChecklistItemId,
    pub run: PlaybookRunId,
    pub playbook: String,
    pub step: u32,
    pub step_title: String,
    pub entity: Option<EntitySummary>,
    pub text: String,
    pub launchers: Vec<LauncherView>,
    pub screen: Option<String>,
    pub state: String,
    pub note: Option<String>,
    pub resolved_at: Option<jiff::Timestamp>,
    /// The image file to reveal when every launcher is disabled.
    pub local_image: Option<String>,
}

pub(crate) fn step_info(s: &spektr_protocol::playbook::StepDef) -> StepInfo {
    let (kind, detail) = match s.kind() {
        Some(StepKind::Run(m)) => ("run", format!("run {}", m.join(" "))),
        Some(StepKind::Playbook(p)) => ("playbook", format!("playbook {p}")),
        Some(StepKind::Check { text, .. }) => ("check", text.to_string()),
        None => ("?", String::new()),
    };
    let on = match &s.on {
        None => "—".to_string(),
        Some(On::Seed(_)) => "seed".to_string(),
        Some(On::Select(sel)) => {
            let from = match sel.from {
                From::Run => "run",
                From::SeedLinks => "seed links",
                From::Case => "case",
            };
            let mut t = format!("{} from {from}", sel.ty);
            if let Some(m) = sel.max {
                t.push_str(&format!(", max {m}"));
            }
            t
        }
    };
    StepInfo {
        title: s.title.clone(),
        kind: kind.into(),
        detail,
        on,
    }
}

/// PLAYBOOKS §2.9 against this engine's modules, launchers and playbooks.
pub(crate) fn check_playbook(shared: &Shared, file: &PlaybookFile) -> Problems {
    let modules: BTreeMap<String, Manifest> = read!(shared.modules)
        .iter()
        .map(|(k, m)| (k.clone(), m.manifest().clone()))
        .collect();
    let launchers = read!(shared.launchers).clone();
    let playbooks = read!(shared.playbooks).clone();
    let disabled = BTreeSet::new();
    playbooks.check(
        file,
        &CheckCtx {
            modules: &modules,
            disabled: &disabled,
            launchers: &launchers,
            playbooks: &playbooks,
        },
    )
}

pub(crate) fn playbooks(shared: &Shared) -> Vec<PlaybookInfo> {
    let pbs = read!(shared.playbooks).clone();
    pbs.iter()
        .map(|p| {
            let problems = check_playbook(shared, &p.file);
            PlaybookInfo {
                name: p.name().to_string(),
                version: p.file.playbook.version.clone(),
                description: p.file.playbook.description.clone(),
                seed: p.file.playbook.seed.clone(),
                requires_profile: p.file.playbook.requires_profile.clone(),
                source: match &p.source {
                    Source::Builtin => "built-in".into(),
                    Source::User(path) => path.display().to_string(),
                },
                steps: p.file.steps.iter().map(step_info).collect(),
                errors: problems.errors,
                warnings: problems.warnings,
            }
        })
        .collect()
}

fn launcher_views(l: &Launchers, ids: &[String], e: Option<&EntityRow>) -> Vec<LauncherView> {
    ids.iter()
        .map(|id| match (l.get(id), e) {
            (Some(x), Some(e)) => match Launchers::url(x, e) {
                Ok(u) => LauncherView {
                    id: id.clone(),
                    name: x.name.clone(),
                    url: Some(u),
                    disabled: None,
                },
                Err(why) => LauncherView {
                    id: id.clone(),
                    name: x.name.clone(),
                    url: None,
                    disabled: Some(why),
                },
            },
            (Some(x), None) => LauncherView {
                id: id.clone(),
                name: x.name.clone(),
                url: None,
                disabled: Some("no entity".into()),
            },
            (None, _) => LauncherView {
                id: id.clone(),
                name: id.clone(),
                url: None,
                disabled: Some("unknown launcher".into()),
            },
        })
        .collect()
}

pub(crate) async fn runs(shared: &Arc<Shared>) -> EngineResult<Vec<RunView>> {
    let pbs = read!(shared.playbooks).clone();
    Ok(shared
        .store
        .read(move |r| {
            let mut out = Vec::new();
            for run in r.playbook_runs()? {
                let pb = pbs.get(&run.playbook);
                let infos: Vec<StepInfo> = pb
                    .map(|p| p.file.steps.iter().map(step_info).collect())
                    .unwrap_or_default();
                let rows = r.playbook_steps(run.id)?;
                let steps = infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| {
                        let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
                        let row = rows.iter().find(|s| s.step == n);
                        StepView {
                            step: n,
                            info: info.clone(),
                            state: row.map_or("pending".into(), |s| s.state.as_str().to_string()),
                            selected: row.map_or(0, |s| s.selected),
                            capped: row.map_or(0, |s| s.capped),
                            result: row.map_or(serde_json::json!({}), |s| s.result.clone()),
                        }
                    })
                    .collect();
                out.push(RunView {
                    id: run.id,
                    playbook: run.playbook.clone(),
                    seed: r.summary(r.resolve(run.seed_entity)?)?,
                    parent: run.parent_run,
                    state: run.state.as_str().to_string(),
                    current_step: run.current_step,
                    total_steps: u32::try_from(infos.len()).unwrap_or(0),
                    started_at: run.started_at,
                    finished_at: run.finished_at,
                    steps,
                });
            }
            Ok(out)
        })
        .await?)
}

/// Items, optionally of one state, with their entity and launchers.
pub(crate) async fn checklist(
    shared: &Arc<Shared>,
    state: Option<ItemState>,
    entity: Option<EntityId>,
) -> EngineResult<Vec<ItemView>> {
    let launchers = read!(shared.launchers).clone();
    let pbs = read!(shared.playbooks).clone();
    Ok(shared
        .store
        .read(move |r| {
            let rows: Vec<ChecklistItemRow> = match entity {
                Some(e) => r.checklist_items_for_entity(r.resolve(e)?)?,
                None => r.checklist_items(None, None)?,
            };
            let runs: BTreeMap<PlaybookRunId, String> = r
                .playbook_runs()?
                .into_iter()
                .map(|x| (x.id, x.playbook))
                .collect();
            let mut out = Vec::new();
            for it in rows {
                if state.is_some_and(|s| s != it.state) {
                    continue;
                }
                let e = match it.entity {
                    Some(id) => r.entity(r.resolve(id)?)?,
                    None => None,
                };
                let playbook = runs.get(&it.run_id).cloned().unwrap_or_default();
                let step_title = pbs
                    .get(&playbook)
                    .and_then(|p| {
                        p.file
                            .steps
                            .get(usize::try_from(it.step).unwrap_or(1).saturating_sub(1))
                    })
                    .map(|s| s.title.clone())
                    .unwrap_or_default();
                let lv = launcher_views(&launchers, &it.launchers, e.as_ref());
                let local_image = e
                    .as_ref()
                    .filter(|e| {
                        e.ty.is(spektr_model::CoreType::Image)
                            && !lv.is_empty()
                            && lv.iter().all(|l| l.disabled.is_some())
                    })
                    .map(|e| e.key.clone());
                out.push(ItemView {
                    id: it.id,
                    run: it.run_id,
                    playbook,
                    step: it.step,
                    step_title,
                    entity: e.map(|e| e.summary()),
                    text: it.text,
                    launchers: lv,
                    screen: it.screen,
                    state: it.state.as_str().to_string(),
                    note: it.note,
                    resolved_at: it.resolved_at,
                    local_image,
                });
            }
            Ok(out)
        })
        .await?)
}

/// The launchers that apply to an entity, with their URLs.
pub(crate) async fn launchers_for(
    shared: &Arc<Shared>,
    entity: EntityId,
) -> EngineResult<Vec<LauncherView>> {
    let launchers = read!(shared.launchers).clone();
    Ok(shared
        .store
        .read(move |r| {
            let Some(e) = r.entity(r.resolve(entity)?)? else {
                return Ok(Vec::new());
            };
            let ids: Vec<String> = launchers.for_entity(&e).map(|l| l.id.clone()).collect();
            Ok(launcher_views(&launchers, &ids, Some(&e)))
        })
        .await?)
}

/// Open a launcher: the URL is returned for the caller to open, and the
/// opening is recorded in the audit log (MODULES §8, PLAYBOOKS §2.8).
pub(crate) async fn open_launcher(
    shared: &Arc<Shared>,
    id: String,
    entity: EntityId,
    item: Option<ChecklistItemId>,
) -> EngineResult<String> {
    let launchers = read!(shared.launchers).clone();
    let l = launchers
        .get(&id)
        .cloned()
        .ok_or_else(|| EngineError::Invalid(format!("unknown launcher `{id}`")))?;
    let (url, summary) = shared
        .store
        .write(move |tx| {
            let root = tx.reader().resolve(entity)?;
            let e = tx
                .reader()
                .entity(root)?
                .ok_or_else(|| spektr_store::StoreError::NotFound("entity".into()))?;
            let url = Launchers::url(&l, &e).map_err(spektr_store::StoreError::Invalid)?;
            tx.record(
                &Actor::Analyst,
                "launcher.open",
                &serde_json::json!({ "launcher": l.id, "entity": root, "url": url, "item": item }),
            )?;
            Ok((url, e.summary()))
        })
        .await?;
    shared.emit(EventKind::LauncherOpened {
        launcher: id,
        entity: summary,
        url: url.clone(),
    });
    Ok(url)
}

/// `:reload`: re-read exec modules, playbooks, profiles and launchers from
/// the config dir. Returns the warnings.
pub(crate) fn reload(shared: &Arc<Shared>) -> EngineResult<Vec<String>> {
    let Some(dir) = shared.config.config_dir.clone() else {
        return Ok(vec!["no config dir: nothing to reload".into()]);
    };
    let mut warnings = Vec::new();
    let mut pbs = crate::playbook::Playbooks::builtin().map_err(EngineError::Invalid)?;
    warnings.extend(pbs.load_dir(&dir.join("playbooks")));
    *crate::engine::write_lock(&shared.playbooks) = pbs;
    let mut ls = Launchers::builtin().map_err(EngineError::Invalid)?;
    warnings.extend(ls.load_file(&dir.join("launchers.toml")));
    *crate::engine::write_lock(&shared.launchers) = ls;
    let mut profiles = crate::profile::Profiles::builtin()?;
    warnings.extend(profiles.load_dir(&dir.join("profiles"))?);
    *crate::engine::write_lock(&shared.profiles) = profiles;
    // Exec modules not yet registered (new installs).
    let known: BTreeSet<String> = read!(shared.modules).keys().cloned().collect();
    let before = known.len();
    warnings.extend(
        crate::engine::load_exec_modules(shared, &dir.join("modules"))
            .into_iter()
            .filter(|w| !w.contains("already registered")),
    );
    let added = read!(shared.modules).len() - before;
    if added > 0 {
        warnings.push(format!(
            "{added} new module{}",
            if added == 1 { "" } else { "s" }
        ));
    }
    shared.emit(EventKind::CaseChanged {
        what: "reload".into(),
    });
    Ok(warnings)
}
