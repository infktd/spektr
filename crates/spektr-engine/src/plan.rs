//! Planning: turn requests ("run module M on entity E") into job rows.
//!
//! Eligibility runs inside one store write transaction, together with the
//! duplicate check and the job inserts, so two requests for the same
//! `(module, entity)` can't both be queued (the writer thread serializes).

use std::sync::Arc;

use jiff::{Timestamp, ToSpan};
use serde::Serialize;
use spektr_model::{Actor, EntityFlags, EntityId, JobId, ObservationId, PlaybookRunId};
use spektr_protocol::Manifest;
use spektr_store::{EntitySummary, JobState, Trigger};

use crate::eligibility::{BudgetFacts, InputFacts, ModuleFacts, Request, Verdict, check};
use crate::engine::Shared;
use crate::error::EngineResult;
use crate::events::EventKind;
use crate::profile::RulesProfile;
use crate::scheduler::{QueuedJob, SchedMsg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRequest {
    pub module: String,
    pub entity: EntityId,
    pub trigger: Trigger,
    pub run_id: Option<PlaybookRunId>,
    pub rerun: bool,
    /// A re-plan (an entity gained an attribute): record nothing when the
    /// cache or the rules already decided, only queue what can now run.
    pub quiet: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PlanResult {
    Queued {
        job: JobId,
    },
    Blocked {
        job: JobId,
        reason: String,
    },
    Cached {
        job: JobId,
        observation: ObservationId,
    },
    /// Already queued or running: the request joins it.
    Joined {
        job: JobId,
    },
    Ineligible {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanOutcome {
    pub module: String,
    pub entity: EntityId,
    #[serde(flatten)]
    pub result: PlanResult,
}

fn actor_for(trigger: Trigger) -> Actor {
    match trigger {
        Trigger::Auto => Actor::Engine,
        Trigger::Manual => Actor::Analyst,
        Trigger::Tutor => Actor::Tutor,
        // Playbook jobs are written by the runner with its own actor
        // (Phase 4); planning on its behalf is the engine's.
        Trigger::Playbook => Actor::Engine,
    }
}

/// One request's outcome, the job to enqueue (if any), and the input.
type Planned = (
    PlanOutcome,
    Option<(QueuedJob, EntitySummary)>,
    Option<EntitySummary>,
);

struct ModuleSnapshot {
    manifest: Manifest,
    missing_secrets: Vec<String>,
}

/// Plan and enqueue. Returns one outcome per request, in order.
pub(crate) async fn plan(
    shared: &Arc<Shared>,
    reqs: Vec<PlanRequest>,
) -> EngineResult<Vec<PlanOutcome>> {
    if reqs.is_empty() {
        return Ok(Vec::new());
    }
    let profile: RulesProfile = shared.profile()?;
    let case_depth = shared.depth_limit();
    let snapshots: Vec<Option<ModuleSnapshot>> = reqs
        .iter()
        .map(|r| {
            shared.module(&r.module).map(|m| {
                let manifest = m.manifest().clone();
                let missing_secrets = shared.missing_secrets(&manifest);
                ModuleSnapshot {
                    manifest,
                    missing_secrets,
                }
            })
        })
        .collect();
    let budget_overrides: Vec<Option<u32>> = reqs
        .iter()
        .map(|r| {
            shared
                .config
                .module_config
                .get(&r.module)
                .and_then(|c| c.get("daily_budget"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
        })
        .collect();
    let now = shared.clock.now();
    let today: Timestamp = now
        .to_zoned(jiff::tz::TimeZone::UTC)
        .start_of_day()
        .map(|z| z.timestamp())
        .unwrap_or(now);

    let reqs_in = reqs.clone();
    let planned: Vec<Planned> = shared
        .store
        .write(move |tx| {
            let mut out = Vec::new();
            for ((req, snap), budget_override) in
                reqs_in.into_iter().zip(snapshots).zip(budget_overrides)
            {
                let outcome = |result| PlanOutcome {
                    module: req.module.clone(),
                    entity: req.entity,
                    result,
                };
                let Some(snap) = snap else {
                    out.push((
                        outcome(PlanResult::Ineligible {
                            reason: format!("unknown module `{}`", req.module),
                        }),
                        None,
                        None,
                    ));
                    continue;
                };
                let r = tx.reader();
                let root = r.resolve(req.entity)?;
                let Some(entity) = r.entity(root)? else {
                    out.push((
                        outcome(PlanResult::Ineligible {
                            reason: "no such entity".into(),
                        }),
                        None,
                        None,
                    ));
                    continue;
                };
                let summary = entity.summary();
                if let Some(job) = r.active_job(&req.module, root)? {
                    out.push((outcome(PlanResult::Joined { job }), None, Some(summary)));
                    continue;
                }
                let enabled = r
                    .module_settings()?
                    .into_iter()
                    .find(|s| s.module == req.module)
                    .is_none_or(|s| s.enabled);
                let platform = (entity.ty.as_str() == "account")
                    .then(|| entity.key.split_once(':').map_or("", |p| p.0));
                let ttl = snap.manifest.cache_ttl_duration();
                let not_before = now
                    - i64::try_from(ttl.as_secs())
                        .unwrap_or(i64::MAX / 2)
                        .seconds();
                let cached = r.cached_observation(
                    &snap.manifest.name,
                    &snap.manifest.version,
                    root,
                    not_before,
                )?;
                let limit = budget_override.or(snap.manifest.cost.daily_budget);
                let used = if limit.is_some() {
                    u32::try_from(r.jobs_since(&req.module, today)?).unwrap_or(u32::MAX)
                } else {
                    0
                };
                let verdict = check(&Request {
                    module: ModuleFacts {
                        manifest: &snap.manifest,
                        enabled,
                        missing_secrets: snap.missing_secrets.clone(),
                    },
                    input: InputFacts {
                        ty: entity.ty.as_str(),
                        platform,
                        depth: entity.depth,
                        hub: entity.flags.contains(EntityFlags::HUB),
                        in_scope: !entity.flags.contains(EntityFlags::OUT_OF_SCOPE),
                    },
                    trigger: req.trigger,
                    rerun: req.rerun,
                    profile: &profile,
                    case_depth,
                    cached,
                    budget: BudgetFacts { used, limit },
                });
                let actor = actor_for(req.trigger);
                match verdict {
                    Verdict::Run => {
                        let job = tx.add_job(
                            &actor,
                            &req.module,
                            root,
                            req.trigger,
                            req.run_id,
                            entity.depth,
                            JobState::Queued,
                            None,
                        )?;
                        let q = QueuedJob {
                            job,
                            module: req.module.clone(),
                            entity: root,
                            depth: entity.depth,
                            trigger: req.trigger,
                            run_id: req.run_id,
                            attempt: 0,
                            not_before: None,
                        };
                        out.push((
                            outcome(PlanResult::Queued { job }),
                            Some((q, summary.clone())),
                            Some(summary),
                        ));
                    }
                    Verdict::Blocked(_) | Verdict::Cached(_) if req.quiet => {}
                    Verdict::Blocked(reason) => {
                        let job = tx.add_job(
                            &actor,
                            &req.module,
                            root,
                            req.trigger,
                            req.run_id,
                            entity.depth,
                            JobState::Blocked,
                            Some(&reason),
                        )?;
                        out.push((
                            outcome(PlanResult::Blocked { job, reason }),
                            None,
                            Some(summary),
                        ));
                    }
                    Verdict::Cached(observation) => {
                        let job = tx.add_job(
                            &actor,
                            &req.module,
                            root,
                            req.trigger,
                            req.run_id,
                            entity.depth,
                            JobState::Cached,
                            Some("cached"),
                        )?;
                        tx.link_job(&actor, job, Some(observation), None)?;
                        out.push((
                            outcome(PlanResult::Cached { job, observation }),
                            None,
                            Some(summary),
                        ));
                    }
                    Verdict::Ineligible(reason) => {
                        out.push((
                            outcome(PlanResult::Ineligible { reason }),
                            None,
                            Some(summary),
                        ));
                    }
                }
            }
            Ok(out)
        })
        .await?;

    let mut outcomes = Vec::with_capacity(planned.len());
    for (outcome, queued, summary) in planned {
        match (&outcome.result, summary) {
            (PlanResult::Queued { job }, Some(entity)) => {
                shared.emit(EventKind::JobQueued {
                    job: *job,
                    module: outcome.module.clone(),
                    entity,
                    trigger: queued.as_ref().map_or(Trigger::Auto, |q| q.0.trigger),
                });
            }
            (PlanResult::Blocked { job, reason }, Some(entity)) => {
                shared.emit(EventKind::JobBlocked {
                    job: Some(*job),
                    module: outcome.module.clone(),
                    entity,
                    reason: reason.clone(),
                })
            }
            (PlanResult::Cached { job, .. }, Some(entity)) => shared.emit(EventKind::JobCached {
                job: *job,
                module: outcome.module.clone(),
                entity,
            }),
            (PlanResult::Ineligible { reason }, Some(entity)) if reason.starts_with("hub") => {
                let first = shared
                    .hub_notified
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(entity.id);
                if first {
                    let degree = shared
                        .store
                        .read(move |r| r.degree(entity.id))
                        .await
                        .map(|d| d.total)
                        .unwrap_or(0);
                    shared.emit(EventKind::HubSuppressed { entity, degree });
                }
            }
            _ => {}
        }
        if let Some((q, _)) = queued {
            shared.work.add(1);
            if shared.sched.send(SchedMsg::Enqueue(q)).is_err() {
                shared.work.done(1);
            }
        }
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Automatic candidates for new entities: every `auto` module that consumes
/// the entity's type.
pub(crate) fn auto_requests(
    shared: &Shared,
    entities: &[(EntityId, String, Option<String>)],
) -> Vec<PlanRequest> {
    let modules = crate::engine::read!(shared.modules);
    let mut out = Vec::new();
    for (id, ty, platform) in entities {
        for (name, m) in modules.iter() {
            let man = m.manifest();
            if man.auto && man.consumes_entity(ty, platform.as_deref()) {
                out.push(PlanRequest {
                    module: name.clone(),
                    entity: *id,
                    trigger: Trigger::Auto,
                    run_id: None,
                    rerun: false,
                    quiet: false,
                });
            }
        }
    }
    out
}
