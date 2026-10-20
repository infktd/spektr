//! The playbook runner (docs/PLAYBOOKS.md §2). One task per top-level run;
//! nested runs execute inline, one after another. Steps run in order: `run`
//! steps wait for every job they queued, `playbook` steps for every child
//! run, `check` steps finish at once. Rules always apply: jobs go through
//! the planner with the `playbook` trigger.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::json;
use spektr_model::{Actor, CoreType, EntityFlags, EntityId, JobId, PlaybookRunId, Strength};
use spektr_protocol::playbook::{From, On, StepKind as Kind};
use spektr_store::{ItemState, JobState, RunState, StepKind, StepState, Trigger};
use tokio_util::sync::CancellationToken;

use crate::engine::{Shared, read};
use crate::error::{EngineError, EngineResult};
use crate::events::EventKind;
use crate::plan::{PlanRequest, PlanResult, plan};
use crate::playbook::{Playbook, seed_matches};
use crate::scheduler::SchedMsg;

/// Polling interval while a step waits for its jobs.
const POLL: Duration = Duration::from_millis(40);

/// `(module, entity)` pairs already run in this top-level run, children
/// included: a module never runs twice on one entity in a run (§2.5).
type Dedup = Arc<Mutex<HashSet<(String, EntityId)>>>;

pub(crate) struct ActiveRun {
    pub run: PlaybookRunId,
    pub cancel: CancellationToken,
}

fn actor(pb: &Playbook) -> Actor {
    Actor::Playbook {
        name: pb.name().to_string(),
        version: pb.file.playbook.version.clone(),
    }
}

fn step_kind(k: &Kind<'_>) -> StepKind {
    match k {
        Kind::Run(_) => StepKind::Run,
        Kind::Playbook(_) => StepKind::Playbook,
        Kind::Check { .. } => StepKind::Check,
    }
}

/// Start a top-level run on `seed`. Refuses while another top-level run is
/// going (the caller asks the analyst to stop it first, §2.6).
pub(crate) async fn start(
    shared: &Arc<Shared>,
    name: &str,
    seed: EntityId,
) -> EngineResult<PlaybookRunId> {
    let pb = read!(shared.playbooks)
        .get(name)
        .cloned()
        .ok_or_else(|| EngineError::Invalid(format!("unknown playbook `{name}`")))?;
    if let Some(a) = shared
        .active_run
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        && !a.cancel.is_cancelled()
    {
        return Err(EngineError::Invalid(
            "a playbook is already running: stop it first (`:playbook stop`)".into(),
        ));
    }
    let need = &pb.file.playbook.requires_profile;
    if !need.is_empty() && shared.settings().profile != *need {
        return Err(EngineError::Invalid(format!(
            "`{name}` needs the `{need}` rules profile"
        )));
    }
    let problems = crate::workflow::check_playbook(shared, &pb.file);
    if !problems.errors.is_empty() {
        return Err(EngineError::Invalid(format!(
            "`{name}`: {}",
            problems.errors.join("; ")
        )));
    }
    let (root, ty, platform) = shared
        .store
        .read(move |r| {
            let root = r.resolve(seed)?;
            let e = r
                .entity(root)?
                .ok_or_else(|| spektr_store::StoreError::NotFound("seed entity".into()))?;
            let platform =
                e.ty.is(CoreType::Account)
                    .then(|| e.key.split_once(':').map(|(p, _)| p.to_string()))
                    .flatten();
            Ok((root, e.ty.as_str().to_string(), platform))
        })
        .await?;
    if !seed_matches(&pb.file.playbook.seed, &ty, platform.as_deref()) {
        return Err(EngineError::Invalid(format!(
            "`{name}` starts from {}, not {ty}",
            pb.file.playbook.seed.join(" or ")
        )));
    }
    let run = new_run(shared, &pb, root, None).await?;
    let cancel = CancellationToken::new();
    *shared.active_run.lock().unwrap_or_else(|e| e.into_inner()) = Some(ActiveRun {
        run,
        cancel: cancel.clone(),
    });
    spawn(Arc::clone(shared), pb, run, root, cancel, 0);
    Ok(run)
}

fn spawn(
    shared: Arc<Shared>,
    pb: Playbook,
    run: PlaybookRunId,
    seed: EntityId,
    cancel: CancellationToken,
    from_step: usize,
) {
    shared.work.add(1);
    tokio::spawn(async move {
        let dedup: Dedup = Arc::default();
        if let Err(e) = execute(Arc::clone(&shared), pb, run, seed, cancel, from_step, dedup).await
        {
            tracing::error!("playbook run {run} failed: {e}");
            let _ = shared
                .store
                .write(move |tx| tx.set_run_state(&Actor::Engine, run, RunState::Failed, 0))
                .await;
            shared.emit(EventKind::PlaybookFinished {
                run,
                playbook: String::new(),
                state: "failed".into(),
            });
        }
        let mut a = shared.active_run.lock().unwrap_or_else(|e| e.into_inner());
        if a.as_ref().is_some_and(|x| x.run == run) {
            *a = None;
        }
        drop(a);
        shared.work.done(1);
    });
}

async fn new_run(
    shared: &Arc<Shared>,
    pb: &Playbook,
    seed: EntityId,
    parent: Option<PlaybookRunId>,
) -> EngineResult<PlaybookRunId> {
    let (name, version, hash, act) = (
        pb.name().to_string(),
        pb.file.playbook.version.clone(),
        pb.hash.clone(),
        actor(pb),
    );
    let run = shared
        .store
        .write(move |tx| tx.add_playbook_run(&act, &name, &version, &hash, seed, parent))
        .await?;
    let summary = shared.store.read(move |r| r.summary(seed)).await?;
    if let Some(seed) = summary {
        shared.emit(EventKind::PlaybookStarted {
            run,
            playbook: pb.name().to_string(),
            seed,
            parent,
        });
    }
    Ok(run)
}

/// Stop the top-level run (and its children): remaining steps are
/// cancelled, in-flight jobs it started are cancelled (joined auto jobs
/// keep going), results already ingested stay (§2.6).
pub(crate) fn stop(shared: &Arc<Shared>) -> bool {
    let a = shared.active_run.lock().unwrap_or_else(|e| e.into_inner());
    match a.as_ref() {
        Some(r) if !r.cancel.is_cancelled() => {
            r.cancel.cancel();
            true
        }
        _ => false,
    }
}

/// This run and every run nested under it.
async fn family(shared: &Arc<Shared>, run: PlaybookRunId) -> EngineResult<Vec<PlaybookRunId>> {
    Ok(shared
        .store
        .read(move |r| {
            let mut out = vec![run];
            let mut i = 0;
            while i < out.len() {
                out.extend(r.child_runs(out[i])?.into_iter().map(|c| c.id));
                i += 1;
            }
            Ok(out)
        })
        .await?)
}

fn strength_rank(s: Option<Strength>) -> u8 {
    match s {
        None => 0,
        Some(Strength::Single) => 1,
        Some(Strength::Corroborated) => 2,
        Some(Strength::Confirmed) => 3,
    }
}

/// The entities a step's selector picks, and how many the cap dropped.
async fn select(
    shared: &Arc<Shared>,
    run: PlaybookRunId,
    seed: EntityId,
    on: Option<&On>,
) -> EngineResult<(Vec<EntityId>, u32)> {
    let sel = match on {
        None | Some(On::Seed(_)) => return Ok((vec![seed], 0)),
        Some(On::Select(s)) => s.clone(),
    };
    let runs = family(shared, run).await?;
    let min = match sel.min_strength.as_deref() {
        Some("corroborated") => 2,
        Some("confirmed") => 3,
        _ => 1,
    };
    let (base, platform) = match sel.ty.split_once(':') {
        Some((t, p)) => (t.to_string(), Some(p.to_string())),
        None => (sel.ty.clone(), None),
    };
    let cap = sel.cap() as usize;
    shared
        .store
        .read(move |r| {
            // Candidate roots by `from`.
            let candidates: BTreeSet<EntityId> = match sel.from {
                From::Case => r.entities()?.into_iter().map(|e| e.id).collect(),
                From::SeedLinks => r
                    .edges_touching(&r.members(seed)?)?
                    .into_iter()
                    .filter(|e| e.state == spektr_store::EdgeState::Active)
                    .flat_map(|e| [e.src, e.dst])
                    .map(|id| r.resolve(id))
                    .collect::<Result<_, _>>()?,
                From::Run => {
                    // Reachable from the seed over edges whose evidence the
                    // run's jobs produced, hit in the cache, or joined.
                    let obs = r.run_observations(&runs)?;
                    let mut seen: BTreeSet<EntityId> = BTreeSet::from([seed]);
                    let mut frontier = vec![seed];
                    while !frontier.is_empty() {
                        let mut ids = Vec::new();
                        for f in &frontier {
                            ids.extend(r.members(*f)?);
                        }
                        let mut next = Vec::new();
                        for e in r.edges_touching(&ids)? {
                            if e.state != spektr_store::EdgeState::Active {
                                continue;
                            }
                            if !r
                                .evidence(e.id)?
                                .iter()
                                .any(|ev| obs.contains(&ev.observation))
                            {
                                continue;
                            }
                            for end in [e.src, e.dst] {
                                let root = r.resolve(end)?;
                                if seen.insert(root) {
                                    next.push(root);
                                }
                            }
                        }
                        frontier = next;
                    }
                    seen
                }
            };
            let mut picked: Vec<(u8, jiff::Timestamp, EntityId)> = Vec::new();
            for id in candidates {
                if id == seed && sel.from != From::Case {
                    continue;
                }
                let Some(e) = r.entity(id)? else { continue };
                if e.merged_into.is_some() || e.ty.as_str() != base {
                    continue;
                }
                if let Some(p) = &platform
                    && e.key.split_once(':').map(|(x, _)| x) != Some(p.as_str())
                {
                    continue;
                }
                // Hubs and out-of-scope entities are never selected.
                if e.flags.contains(EntityFlags::HUB) || e.flags.contains(EntityFlags::OUT_OF_SCOPE)
                {
                    continue;
                }
                let st = strength_rank(r.entity_strength(id)?);
                // Entities without a strength (seeds, unlinked) pass the
                // default `single`; a stricter minimum needs one.
                if min > 1 && st < min {
                    continue;
                }
                picked.push((st, e.created_at, id));
            }
            // Strongest first, then newest.
            picked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
            let capped = picked.len().saturating_sub(cap);
            picked.truncate(cap);
            Ok((
                picked.into_iter().map(|p| p.2).collect(),
                u32::try_from(capped).unwrap_or(u32::MAX),
            ))
        })
        .await
        .map_err(Into::into)
}

/// Wait until every job is terminal (or the run is stopped). Joined
/// records follow their joined job's final state.
async fn wait_jobs(
    shared: &Arc<Shared>,
    jobs: &[JobId],
    joined: &[(JobId, JobId)],
    cancel: &CancellationToken,
) -> EngineResult<BTreeMap<String, u32>> {
    let mut all: Vec<JobId> = jobs.to_vec();
    all.extend(joined.iter().map(|(_, j)| *j));
    loop {
        let ids = all.clone();
        let states: Vec<JobState> = shared
            .store
            .read(move |r| {
                ids.iter()
                    .map(|j| r.job(*j).map(|x| x.map_or(JobState::Failed, |x| x.state)))
                    .collect()
            })
            .await?;
        if states.iter().all(|s| s.is_terminal()) || cancel.is_cancelled() {
            // Joined records take their joined job's result.
            let fix: Vec<(JobId, JobState)> = joined
                .iter()
                .zip(states.iter().skip(jobs.len()))
                .filter(|(_, s)| s.is_terminal())
                .map(|((mine, _), s)| (*mine, *s))
                .collect();
            if !fix.is_empty() {
                shared
                    .store
                    .write(move |tx| {
                        for (j, s) in fix {
                            tx.set_job_state(&Actor::Engine, j, s, Some("joined"), None)?;
                        }
                        Ok(())
                    })
                    .await?;
            }
            let mut counts = BTreeMap::new();
            for s in states {
                *counts.entry(s.as_str().to_string()).or_insert(0) += 1;
            }
            return Ok(counts);
        }
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = cancel.cancelled() => {}
        }
    }
}

fn execute(
    shared: Arc<Shared>,
    pb: Playbook,
    run: PlaybookRunId,
    seed: EntityId,
    cancel: CancellationToken,
    from_step: usize,
    dedup: Dedup,
) -> BoxFuture<'static, EngineResult<RunState>> {
    Box::pin(async move {
        let act = actor(&pb);
        let total = pb.file.steps.len();
        let name = pb.name().to_string();
        for (i, step) in pb.file.steps.iter().enumerate().skip(from_step) {
            let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let Some(kind) = step.kind() else { continue };
            let sk = step_kind(&kind);
            if cancel.is_cancelled() {
                return finish(&shared, &pb, run, i, RunState::Stopped).await;
            }
            {
                let a = act.clone();
                shared
                    .store
                    .write(move |tx| {
                        tx.set_run_state(&a, run, RunState::Running, n)?;
                        tx.set_step(&a, run, n, sk, StepState::Running, 0, 0, &json!({}))
                    })
                    .await?;
            }
            shared.emit(EventKind::PlaybookStep {
                run,
                playbook: name.clone(),
                step: n,
                of: u32::try_from(total).unwrap_or(0),
                title: step.title.clone(),
                state: "running".into(),
            });
            let (selected, capped) = select(&shared, run, seed, step.on.as_ref()).await?;
            let result = match kind {
                Kind::Run(mods) => {
                    let mut reqs = Vec::new();
                    {
                        let mut d = dedup.lock().unwrap_or_else(|e| e.into_inner());
                        for e in &selected {
                            for m in mods {
                                if d.insert((m.clone(), *e)) {
                                    reqs.push(PlanRequest {
                                        module: m.clone(),
                                        entity: *e,
                                        trigger: Trigger::Playbook,
                                        run_id: Some(run),
                                        rerun: false,
                                        quiet: false,
                                    });
                                }
                            }
                        }
                    }
                    let outcomes = plan(&shared, reqs).await?;
                    let mut jobs = Vec::new();
                    let mut joined_raw = Vec::new();
                    let mut decided: BTreeMap<String, u32> = BTreeMap::new();
                    for o in &outcomes {
                        match &o.result {
                            PlanResult::Queued { job } => jobs.push(*job),
                            PlanResult::Joined { job } => {
                                joined_raw.push((o.module.clone(), o.entity, *job))
                            }
                            PlanResult::Cached { .. } => {
                                *decided.entry("cached".into()).or_insert(0) += 1
                            }
                            PlanResult::Blocked { .. } => {
                                *decided.entry("blocked".into()).or_insert(0) += 1
                            }
                            PlanResult::Ineligible { .. } => {
                                *decided.entry("ineligible".into()).or_insert(0) += 1
                            }
                        }
                    }
                    // A joined request is recorded on the run, pointing at
                    // the job it joined (§2.3).
                    let joined: Vec<(JobId, JobId)> = if joined_raw.is_empty() {
                        Vec::new()
                    } else {
                        let a = act.clone();
                        shared
                            .store
                            .write(move |tx| {
                                let mut out = Vec::new();
                                for (m, e, other) in joined_raw {
                                    let depth = tx.reader().entity(e)?.map_or(0, |x| x.depth);
                                    let mine = tx.add_job(
                                        &a,
                                        &m,
                                        e,
                                        Trigger::Playbook,
                                        Some(run),
                                        depth,
                                        JobState::Queued,
                                        Some("joined"),
                                    )?;
                                    tx.link_job(&a, mine, None, Some(other))?;
                                    out.push((mine, other));
                                }
                                Ok(out)
                            })
                            .await?
                    };
                    let mut counts = wait_jobs(&shared, &jobs, &joined, &cancel).await?;
                    for (k, v) in decided {
                        *counts.entry(k).or_insert(0) += v;
                    }
                    json!({ "jobs": counts, "modules": mods })
                }
                Kind::Playbook(child) => {
                    let child_pb = read!(shared.playbooks).get(child).cloned();
                    let Some(child_pb) = child_pb else {
                        return Err(EngineError::Invalid(format!("unknown playbook `{child}`")));
                    };
                    let mut children = Vec::new();
                    for e in &selected {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let c = new_run(&shared, &child_pb, *e, Some(run)).await?;
                        let state = execute(
                            Arc::clone(&shared),
                            child_pb.clone(),
                            c,
                            *e,
                            cancel.clone(),
                            0,
                            Arc::clone(&dedup),
                        )
                        .await?;
                        children.push(json!({ "run": c, "state": state.as_str() }));
                    }
                    json!({ "children": children })
                }
                Kind::Check { text, open, screen } => {
                    let items: Vec<Option<EntityId>> = if step.on.is_none() {
                        vec![None]
                    } else {
                        selected.iter().copied().map(Some).collect()
                    };
                    let (a, text, open, screen) = (
                        act.clone(),
                        text.to_string(),
                        open.to_vec(),
                        screen.map(str::to_string),
                    );
                    let count = items.len();
                    shared
                        .store
                        .write(move |tx| {
                            for e in items {
                                tx.add_checklist_item(
                                    &a,
                                    run,
                                    n,
                                    e,
                                    &text,
                                    &open,
                                    screen.as_deref(),
                                )?;
                            }
                            Ok(())
                        })
                        .await?;
                    shared.emit(EventKind::ChecklistAdded {
                        run,
                        count: u32::try_from(count).unwrap_or(0),
                    });
                    json!({ "items": count })
                }
            };
            let stopped = cancel.is_cancelled();
            let (a, sel_n) = (
                act.clone(),
                u32::try_from(selected.len()).unwrap_or(u32::MAX),
            );
            let st = if stopped {
                StepState::Stopped
            } else {
                StepState::Done
            };
            let res = result.clone();
            shared
                .store
                .write(move |tx| tx.set_step(&a, run, n, sk, st, sel_n, capped, &res))
                .await?;
            shared.emit(EventKind::PlaybookStep {
                run,
                playbook: name.clone(),
                step: n,
                of: u32::try_from(total).unwrap_or(0),
                title: step.title.clone(),
                state: st.as_str().into(),
            });
            if stopped {
                return finish(&shared, &pb, run, i + 1, RunState::Stopped).await;
            }
        }
        finish(&shared, &pb, run, total, RunState::Done).await
    })
}

async fn finish(
    shared: &Arc<Shared>,
    pb: &Playbook,
    run: PlaybookRunId,
    from: usize,
    state: RunState,
) -> EngineResult<RunState> {
    let a = actor(pb);
    let total = pb.file.steps.len();
    let kinds: Vec<StepKind> = pb
        .file
        .steps
        .iter()
        .filter_map(|s| s.kind().map(|k| step_kind(&k)))
        .collect();
    if state == RunState::Stopped {
        // In-flight jobs this run started are cancelled; joined ones aren't
        // its to cancel.
        for r in family(shared, run).await? {
            let _ = shared.sched.send(SchedMsg::CancelRun(r));
        }
    }
    shared
        .store
        .write(move |tx| {
            if state == RunState::Stopped {
                for (i, k) in kinds.iter().enumerate().skip(from) {
                    let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
                    tx.set_step(
                        &a,
                        run,
                        n,
                        *k,
                        StepState::Stopped,
                        0,
                        0,
                        &json!({ "stopped": true }),
                    )?;
                }
            }
            tx.set_run_state(&a, run, state, u32::try_from(total).unwrap_or(0))
        })
        .await?;
    shared.emit(EventKind::PlaybookFinished {
        run,
        playbook: pb.name().to_string(),
        state: state.as_str().into(),
    });
    Ok(state)
}

/// Runs in progress when spektr exited resume on open (§2.7). A playbook
/// that no longer validates fails the run.
pub(crate) async fn resume(shared: &Arc<Shared>) -> EngineResult<()> {
    let runs = shared.store.read(|r| r.playbook_runs()).await?;
    for row in runs.into_iter().filter(|r| r.state == RunState::Running) {
        if row.parent_run.is_some() {
            // Children restart as part of their parent's step.
            let id = row.id;
            shared
                .store
                .write(move |tx| tx.set_run_state(&Actor::Engine, id, RunState::Stopped, 0))
                .await?;
            continue;
        }
        let pb = read!(shared.playbooks).get(&row.playbook).cloned();
        let valid = pb.as_ref().is_some_and(|p| {
            crate::workflow::check_playbook(shared, &p.file)
                .errors
                .is_empty()
        });
        let Some(pb) = pb.filter(|_| valid) else {
            let id = row.id;
            shared
                .store
                .write(move |tx| {
                    tx.set_run_state(&Actor::Engine, id, RunState::Failed, row.current_step)
                })
                .await?;
            shared.warn(format!("playbook run {} ({}) failed on resume: the playbook is missing or no longer validates", row.id, row.playbook));
            continue;
        };
        let cancel = CancellationToken::new();
        *shared.active_run.lock().unwrap_or_else(|e| e.into_inner()) = Some(ActiveRun {
            run: row.id,
            cancel: cancel.clone(),
        });
        let from = usize::try_from(row.current_step.saturating_sub(1)).unwrap_or(0);
        spawn(
            Arc::clone(shared),
            pb,
            row.id,
            row.seed_entity,
            cancel,
            from,
        );
    }
    Ok(())
}

/// Resolve a checklist item (done / skipped / reopened).
pub(crate) async fn resolve_item(
    shared: &Arc<Shared>,
    item: spektr_model::ChecklistItemId,
    state: ItemState,
    note: Option<String>,
) -> EngineResult<()> {
    shared
        .store
        .write(move |tx| tx.resolve_checklist_item(&Actor::Analyst, item, state, note.as_deref()))
        .await?;
    shared.emit(EventKind::CaseChanged {
        what: "checklist".into(),
    });
    Ok(())
}
