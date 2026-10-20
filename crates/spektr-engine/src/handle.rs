//! `EngineHandle`: the one way the TUI and the headless CLI talk to the
//! engine (ARCHITECTURE §4). Cheap to clone.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use spektr_model::{
    Actor, ChecklistItemId, EdgeId, EntityId, EntityRef, JobId, MergeId, ObservationId,
    PlaybookRunId, ReviewItemId, ScopeRuleId,
};
use spektr_protocol::{CostClass, Manifest, Runtime, Technique};
use spektr_store::{
    EgoParams, EgoView, EntityRow, EntitySummary, FindingRow, ItemState, JobRow, JobState, NoteRow,
    ObservationRow, PathView, ReviewItemRow, ReviewState, ScopeRuleRow, Store, TimelineEvent,
    Trigger,
};
use tokio::sync::broadcast;

use crate::engine::{CaseSettings, Shared, read};
use crate::error::{EngineError, EngineResult};
use crate::events::{EngineEvent, EventKind};
use crate::ingest::seed;
use crate::plan::{PlanOutcome, PlanRequest, auto_requests, plan};
use crate::profile::RulesProfile;
use crate::scheduler::SchedMsg;
use crate::scope::parse_rule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineAction {
    /// Add an analyst seed and expand it automatically.
    Seed {
        entity: EntityRef,
    },
    /// Run one module on an entity (manual trigger).
    Run {
        module: String,
        entity: EntityId,
        rerun: bool,
    },
    /// Every eligible module for an entity (`A` in the Run module menu).
    RunAll {
        entity: EntityId,
    },
    /// Force automatic modules on an entity, even a hub (`:expand`).
    Expand {
        entity: EntityId,
    },
    Cancel {
        job: JobId,
    },
    ConfirmEdge(EdgeId),
    UnconfirmEdge(EdgeId),
    RejectEdge(EdgeId),
    RestoreEdge(EdgeId),
    Merge {
        survivor: EntityId,
        merged: EntityId,
    },
    Unmerge(MergeId),
    /// CLI/palette syntax, e.g. `deny domain:cdn.kestrel.example`.
    AddScope(String),
    RemoveScope(ScopeRuleId),
    SetDepth(u32),
    SetRegion(String),
    SetProfile(String),
    AddNote {
        entity: EntityId,
        body: String,
    },
    /// Start a playbook on an entity (PLAYBOOKS §5).
    StartPlaybook {
        name: String,
        entity: EntityId,
    },
    /// `:playbook stop`: the running top-level run and its children.
    StopPlaybook,
    /// Mark a checklist item done or skipped (or reopen it), with a note.
    ResolveItem {
        item: ChecklistItemId,
        state: ItemState,
        note: Option<String>,
    },
    ItemNote {
        item: ChecklistItemId,
        note: String,
    },
    /// Build a launcher's URL and record that it was opened.
    OpenLauncher {
        launcher: String,
        entity: EntityId,
        item: Option<ChecklistItemId>,
    },
    /// `:reload`.
    Reload,
    DecideReview {
        id: ReviewItemId,
        decision: ReviewState,
    },
    /// Web capture (`c`, `spektr capture`). Without an entity, the URL's
    /// own entity.
    Capture {
        entity: Option<EntityId>,
        url: String,
        screenshot: bool,
    },
    /// A local file as evidence (`spektr attach`).
    Attach {
        entity: Option<EntityId>,
        name: String,
        bytes: Vec<u8>,
        note: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ActionOutcome {
    Seeded {
        entity: EntitySummary,
        created: bool,
        jobs: Vec<PlanOutcome>,
    },
    Planned {
        jobs: Vec<PlanOutcome>,
    },
    Merged {
        merge: MergeId,
    },
    PlaybookStarted {
        run: PlaybookRunId,
    },
    /// A launcher's URL, for the caller to open.
    Launch {
        url: String,
    },
    Reloaded {
        warnings: Vec<String>,
    },
    Captured {
        observation: ObservationId,
    },
    Attached {
        observation: ObservationId,
        image: Option<EntitySummary>,
    },
    /// A review decision; an accepted sweep hit adds an account.
    Decided {
        added: Option<EntitySummary>,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    Ego {
        focus: EntityId,
        params: EgoParams,
    },
    Entities,
    Entity(EntityId),
    /// Everything the inspector shows.
    Detail(EntityId),
    Find(EntityRef),
    Timeline,
    Path {
        from: EntityId,
        to: EntityId,
        max_hops: usize,
    },
    Jobs(Vec<JobState>),
    Review(Option<ReviewState>),
    Scope,
    Status,
    Modules,
    Playbooks,
    /// Every playbook run, with its steps.
    Runs,
    /// Checklist items, optionally of one state.
    Checklist(Option<ItemState>),
    /// The launchers that apply to an entity.
    Launchers(EntityId),
    /// An observation's row and bytes (`o`, the raw response).
    Raw(ObservationId),
    /// Blob bytes by SHA-256 hex (image previews).
    Blob(String),
}

/// The inspector's view of one entity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityDetail {
    pub entity: EntityRowView,
    pub strength: Option<spektr_model::Strength>,
    pub degree: spektr_store::Degree,
    pub observations: Vec<ObservationView>,
    pub findings: Vec<FindingView>,
    pub notes: Vec<NoteView>,
    /// Open checklist items for the entity.
    pub checklist: Vec<crate::workflow::ItemView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityRowView {
    pub summary: EntitySummary,
    pub attrs: spektr_model::Attrs,
    pub created_at: jiff::Timestamp,
    pub created_by: String,
}

impl From<EntityRow> for EntityRowView {
    fn from(e: EntityRow) -> Self {
        Self {
            summary: e.summary(),
            attrs: e.attrs,
            created_at: e.created_at,
            created_by: e.created_by,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservationView {
    pub id: spektr_model::ObservationId,
    pub module: String,
    pub module_version: String,
    pub kind: String,
    pub source_url: Option<String>,
    pub content_type: Option<String>,
    pub blob: Option<String>,
    pub created_at: jiff::Timestamp,
}

impl From<ObservationRow> for ObservationView {
    fn from(o: ObservationRow) -> Self {
        Self {
            id: o.id,
            module: o.module,
            module_version: o.module_version,
            kind: o.kind.to_string(),
            source_url: o.source_url,
            content_type: o.content_type,
            blob: o.blob.map(|b| b.hex()),
            created_at: o.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingView {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub status: String,
}

impl From<FindingRow> for FindingView {
    fn from(f: FindingRow) -> Self {
        Self {
            id: f.id,
            title: f.title,
            category: f.category,
            status: f.status.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteView {
    pub body: String,
    pub created_at: jiff::Timestamp,
}

impl From<NoteRow> for NoteView {
    fn from(n: NoteRow) -> Self {
        Self {
            body: n.body,
            created_at: n.created_at,
        }
    }
}

/// The case at a glance: top bar, launcher, `run` summary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaseStatus {
    pub name: String,
    pub path: String,
    pub profile: String,
    pub profile_summary: String,
    pub depth: u32,
    pub region: String,
    pub last_seen: Option<String>,
    pub subject: Option<EntitySummary>,
    pub entities: u64,
    pub edges: u64,
    pub observations: u64,
    pub audit_rows: u64,
    pub findings: u64,
    pub review_pending: u64,
    pub jobs: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModuleInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime: Runtime,
    pub technique: Technique,
    pub auto: bool,
    pub consumes: Vec<String>,
    pub produces: Vec<(String, String)>,
    pub cost: CostClass,
    pub daily_budget: Option<u32>,
    pub missing_secrets: Vec<String>,
    pub hosts: Vec<String>,
    /// Why the active rules profile blocks it, if it does.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum QueryResult {
    Ego(EgoView),
    Entities(Vec<EntitySummary>),
    Entity(Option<EntityRowView>),
    Detail(Option<Box<EntityDetail>>),
    Found(Option<EntitySummary>),
    Timeline(Vec<TimelineEvent>),
    Path(Option<PathView>),
    Jobs(Vec<JobView>),
    Review(Vec<ReviewView>),
    Scope(Vec<String>),
    Status(Box<CaseStatus>),
    Modules(Vec<ModuleInfo>),
    Playbooks(Vec<crate::workflow::PlaybookInfo>),
    Runs(Vec<crate::workflow::RunView>),
    Checklist(Vec<crate::workflow::ItemView>),
    Launchers(Vec<crate::workflow::LauncherView>),
    Raw(Option<Box<crate::evidence::RawView>>),
    Blob(Option<Vec<u8>>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobView {
    pub id: JobId,
    pub module: String,
    pub input: EntityId,
    pub trigger: Trigger,
    pub state: JobState,
    pub reason: Option<String>,
    pub error: Option<String>,
    pub queued_at: jiff::Timestamp,
    pub finished_at: Option<jiff::Timestamp>,
}

impl From<JobRow> for JobView {
    fn from(j: JobRow) -> Self {
        Self {
            id: j.id,
            module: j.module,
            input: j.input_entity,
            trigger: j.trigger,
            state: j.state,
            reason: j.reason,
            error: j.error,
            queued_at: j.queued_at,
            finished_at: j.finished_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReviewView {
    pub id: spektr_model::ReviewItemId,
    pub kind: String,
    pub state: String,
    pub payload: serde_json::Value,
}

impl From<ReviewItemRow> for ReviewView {
    fn from(r: ReviewItemRow) -> Self {
        Self {
            id: r.id,
            kind: r.kind.to_string(),
            state: r.state.to_string(),
            payload: r.payload,
        }
    }
}

#[derive(Clone)]
pub struct EngineHandle {
    shared: Arc<Shared>,
}

impl EngineHandle {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Check a playbook against this engine's modules, launchers and
    /// playbooks (`spektr playbook check`).
    pub fn check_playbook(
        &self,
        file: &spektr_protocol::PlaybookFile,
    ) -> crate::playbook::Problems {
        crate::workflow::check_playbook(&self.shared, file)
    }

    /// Whether web capture is set up, and can take screenshots.
    pub fn capture_status(&self) -> (bool, bool) {
        let c = crate::engine::read!(self.shared.capture);
        (c.is_some(), c.as_ref().is_some_and(|c| c.screenshots()))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.shared.events.subscribe()
    }

    /// The case file, for read-only commands (`export`, `verify`).
    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn settings(&self) -> CaseSettings {
        self.shared.settings()
    }

    pub fn profile(&self) -> EngineResult<RulesProfile> {
        self.shared.profile()
    }

    pub fn warnings(&self) -> Vec<String> {
        self.shared
            .warnings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Wait until every queued job, ingestion batch and follow-up has been
    /// processed (`spektr run` "runs to quiescence").
    pub async fn wait_idle(&self, timeout: Duration) -> EngineResult<()> {
        if self.shared.work.idle(timeout).await {
            Ok(())
        } else {
            Err(EngineError::Timeout(timeout))
        }
    }

    pub async fn act(&self, a: EngineAction) -> EngineResult<ActionOutcome> {
        let shared = &self.shared;
        match a {
            EngineAction::Seed { entity } => {
                let ty = shared.vocab().entity_type(entity.ty.as_str())?;
                let (summary, created, new) = seed(shared, ty, entity.value).await?;
                let jobs = plan(shared, auto_requests(shared, &new)).await?;
                Ok(ActionOutcome::Seeded {
                    entity: summary,
                    created,
                    jobs,
                })
            }
            EngineAction::Run {
                module,
                entity,
                rerun,
            } => {
                if shared.module(&module).is_none() {
                    return Err(EngineError::UnknownModule(module));
                }
                let jobs = plan(
                    shared,
                    vec![PlanRequest {
                        module,
                        entity,
                        trigger: Trigger::Manual,
                        run_id: None,
                        rerun,
                        quiet: false,
                    }],
                )
                .await?;
                Ok(ActionOutcome::Planned { jobs })
            }
            EngineAction::RunAll { entity } => {
                let names: Vec<String> = read!(shared.modules).keys().cloned().collect();
                let reqs = names
                    .into_iter()
                    .map(|module| PlanRequest {
                        module,
                        entity,
                        trigger: Trigger::Manual,
                        run_id: None,
                        rerun: false,
                        quiet: false,
                    })
                    .collect();
                let jobs = plan(shared, reqs).await?;
                // A module that doesn't take this type isn't "eligible"; drop it.
                let jobs = jobs
                    .into_iter()
                    .filter(|j| !matches!(&j.result, crate::plan::PlanResult::Ineligible { reason } if reason.contains("does not accept")))
                    .collect();
                Ok(ActionOutcome::Planned { jobs })
            }
            EngineAction::Expand { entity } => {
                let e = shared
                    .store
                    .read(move |r| r.summary(r.resolve(entity)?))
                    .await?
                    .ok_or_else(|| EngineError::Invalid("no such entity".into()))?;
                let platform = (e.ty.as_str() == "account")
                    .then(|| e.key.split_once(':').map_or("", |p| p.0).to_string());
                let reqs = auto_requests(shared, &[(e.id, e.ty.to_string(), platform)])
                    .into_iter()
                    .map(|mut r| {
                        r.trigger = Trigger::Manual;
                        r
                    })
                    .collect();
                Ok(ActionOutcome::Planned {
                    jobs: plan(shared, reqs).await?,
                })
            }
            EngineAction::Cancel { job } => {
                shared
                    .sched
                    .send(SchedMsg::Cancel(job))
                    .map_err(|_| EngineError::Closed)?;
                Ok(ActionOutcome::Done)
            }
            EngineAction::ConfirmEdge(e) => {
                self.edge_op(e, "confirmed", |tx, e| tx.confirm_edge(&Actor::Analyst, e))
                    .await
            }
            EngineAction::UnconfirmEdge(e) => {
                self.edge_op(e, "unconfirmed", |tx, e| {
                    tx.unconfirm_edge(&Actor::Analyst, e)
                })
                .await
            }
            EngineAction::RejectEdge(e) => {
                self.edge_op(e, "rejected", |tx, e| tx.reject_edge(&Actor::Analyst, e))
                    .await
            }
            EngineAction::RestoreEdge(e) => {
                self.edge_op(e, "restored", |tx, e| tx.restore_edge(&Actor::Analyst, e))
                    .await
            }
            EngineAction::Merge { survivor, merged } => {
                let merge = shared
                    .store
                    .write(move |tx| tx.merge(&Actor::Analyst, survivor, merged))
                    .await?;
                shared.emit(EventKind::CaseChanged {
                    what: "merge".into(),
                });
                Ok(ActionOutcome::Merged { merge })
            }
            EngineAction::Unmerge(m) => {
                shared
                    .store
                    .write(move |tx| tx.unmerge(&Actor::Analyst, m))
                    .await?;
                shared.emit(EventKind::CaseChanged {
                    what: "unmerge".into(),
                });
                Ok(ActionOutcome::Done)
            }
            EngineAction::AddScope(rule) => {
                let (kind, matcher, pattern) = parse_rule(&rule)?;
                shared
                    .store
                    .write(move |tx| tx.add_scope_rule(&Actor::Analyst, kind, matcher, &pattern))
                    .await?;
                self.rescope().await?;
                Ok(ActionOutcome::Done)
            }
            EngineAction::RemoveScope(id) => {
                shared
                    .store
                    .write(move |tx| tx.remove_scope_rule(&Actor::Analyst, id))
                    .await?;
                self.rescope().await?;
                Ok(ActionOutcome::Done)
            }
            EngineAction::SetDepth(d) => self.set_meta("default_depth", serde_json::json!(d)).await,
            EngineAction::SetRegion(r) => {
                let r = r.trim().to_ascii_uppercase();
                if r.len() != 2 || !r.bytes().all(|b| b.is_ascii_uppercase()) {
                    return Err(EngineError::Invalid(format!(
                        "region `{r}` must be a two-letter country code"
                    )));
                }
                self.set_meta("default_region", serde_json::json!(r)).await
            }
            EngineAction::SetProfile(p) => {
                if read!(shared.profiles).get(&p).is_none() {
                    return Err(EngineError::Profile(format!("unknown rules profile `{p}`")));
                }
                self.set_meta("rules_profile", serde_json::json!(p)).await
            }
            EngineAction::AddNote { entity, body } => {
                shared
                    .store
                    .write(move |tx| tx.add_note(&Actor::Analyst, Some(entity), None, &body))
                    .await?;
                shared.emit(EventKind::CaseChanged {
                    what: "note".into(),
                });
                Ok(ActionOutcome::Done)
            }
            EngineAction::StartPlaybook { name, entity } => Ok(ActionOutcome::PlaybookStarted {
                run: crate::runner::start(shared, &name, entity).await?,
            }),
            EngineAction::StopPlaybook => {
                if !crate::runner::stop(shared) {
                    return Err(EngineError::Invalid("no playbook is running".into()));
                }
                Ok(ActionOutcome::Done)
            }
            EngineAction::ResolveItem { item, state, note } => {
                crate::runner::resolve_item(shared, item, state, note).await?;
                Ok(ActionOutcome::Done)
            }
            EngineAction::ItemNote { item, note } => {
                shared
                    .store
                    .write(move |tx| tx.set_checklist_note(&Actor::Analyst, item, &note))
                    .await?;
                shared.emit(EventKind::CaseChanged {
                    what: "checklist".into(),
                });
                Ok(ActionOutcome::Done)
            }
            EngineAction::OpenLauncher {
                launcher,
                entity,
                item,
            } => Ok(ActionOutcome::Launch {
                url: crate::workflow::open_launcher(shared, launcher, entity, item).await?,
            }),
            EngineAction::Reload => Ok(ActionOutcome::Reloaded {
                warnings: crate::workflow::reload(shared)?,
            }),
            EngineAction::DecideReview { id, decision } => Ok(ActionOutcome::Decided {
                added: crate::evidence::decide(shared, id, decision).await?,
            }),
            EngineAction::Capture {
                entity,
                url,
                screenshot,
            } => Ok(ActionOutcome::Captured {
                observation: crate::evidence::capture(shared, entity, url, screenshot).await?,
            }),
            EngineAction::Attach {
                entity,
                name,
                bytes,
                note,
            } => {
                let (observation, image) =
                    crate::evidence::attach(shared, entity, name, bytes, note).await?;
                Ok(ActionOutcome::Attached { observation, image })
            }
        }
    }

    async fn edge_op(
        &self,
        edge: EdgeId,
        what: &str,
        f: impl FnOnce(&mut spektr_store::Tx<'_>, EdgeId) -> spektr_store::StoreResult<()>
        + Send
        + 'static,
    ) -> EngineResult<ActionOutcome> {
        self.shared.store.write(move |tx| f(tx, edge)).await?;
        self.shared.emit(EventKind::EdgeChanged { edge });
        tracing::info!("edge {edge} {what}");
        Ok(ActionOutcome::Done)
    }

    async fn set_meta(
        &self,
        key: &'static str,
        value: serde_json::Value,
    ) -> EngineResult<ActionOutcome> {
        self.shared
            .store
            .write(move |tx| tx.set_meta(&Actor::Analyst, key, &value))
            .await?;
        self.shared.reload_settings().await?;
        self.shared
            .emit(EventKind::CaseChanged { what: key.into() });
        Ok(ActionOutcome::Done)
    }

    /// Recompute `out_of_scope` for every entity after the rules change.
    async fn rescope(&self) -> EngineResult<()> {
        let rows = self.shared.store.read(|r| r.scope_rules()).await?;
        let scope = crate::scope::Scope::from_rows(&rows);
        self.shared
            .store
            .write(move |tx| {
                use spektr_model::EntityFlags as F;
                for e in tx.reader().entities()? {
                    let out = !scope.in_scope(&e.ty, &e.key);
                    if out != e.flags.contains(F::OUT_OF_SCOPE) {
                        let (add, remove) = if out {
                            (F::OUT_OF_SCOPE, F::NONE)
                        } else {
                            (F::NONE, F::OUT_OF_SCOPE)
                        };
                        tx.set_flags(&Actor::Analyst, e.id, add, remove)?;
                    }
                }
                Ok(())
            })
            .await?;
        self.shared.emit(EventKind::CaseChanged {
            what: "scope".into(),
        });
        Ok(())
    }

    pub async fn query(&self, q: Query) -> EngineResult<QueryResult> {
        let store = &self.shared.store;
        Ok(match q {
            Query::Ego { focus, params } => {
                QueryResult::Ego(store.read(move |r| r.ego(focus, &params)).await?)
            }
            Query::Entities => QueryResult::Entities(store.read(|r| r.entities()).await?),
            Query::Entity(id) => QueryResult::Entity(
                store
                    .read(move |r| r.entity(r.resolve(id)?))
                    .await?
                    .map(Into::into),
            ),
            Query::Detail(id) => {
                let mut res = QueryResult::Detail(
                    store
                        .read(move |r| {
                            let root = r.resolve(id)?;
                            let Some(e) = r.entity(root)? else {
                                return Ok(None);
                            };
                            Ok(Some(Box::new(EntityDetail {
                                strength: r.entity_strength(root)?,
                                degree: r.degree(root)?,
                                observations: r
                                    .entity_observations(root)?
                                    .into_iter()
                                    .map(Into::into)
                                    .collect(),
                                findings: r
                                    .findings_for_entity(root)?
                                    .into_iter()
                                    .map(Into::into)
                                    .collect(),
                                notes: r
                                    .notes_for_entity(root)?
                                    .into_iter()
                                    .map(Into::into)
                                    .collect(),
                                checklist: Vec::new(),
                                entity: e.into(),
                            })))
                        })
                        .await?,
                );
                if let QueryResult::Detail(Some(d)) = &mut res {
                    d.checklist =
                        crate::workflow::checklist(&self.shared, Some(ItemState::Open), Some(id))
                            .await?;
                }
                res
            }
            Query::Find(er) => {
                let vocab = self.shared.vocab();
                let region = self.shared.settings().region;
                let n = spektr_model::normalize(
                    &vocab.entity_type(er.ty.as_str())?,
                    &er.value,
                    spektr_model::KeyHint::Key(&er.value),
                    spektr_model::NormalizeCtx {
                        vocab: &vocab,
                        default_region: Some(&region),
                    },
                )
                .or_else(|_| {
                    spektr_model::normalize(
                        &er.ty,
                        &er.value,
                        spektr_model::KeyHint::None,
                        spektr_model::NormalizeCtx {
                            vocab: &vocab,
                            default_region: Some(&region),
                        },
                    )
                })?;
                QueryResult::Found(
                    store
                        .read(move |r| match r.find(&n.ty, &n.key)? {
                            Some(id) => r.summary(id),
                            None => Ok(None),
                        })
                        .await?,
                )
            }
            Query::Timeline => QueryResult::Timeline(store.read(|r| r.timeline()).await?),
            Query::Path { from, to, max_hops } => QueryResult::Path(
                store
                    .read(move |r| r.shortest_path(from, to, max_hops))
                    .await?,
            ),
            Query::Jobs(states) => QueryResult::Jobs(
                store
                    .read(move |r| r.jobs(&states))
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            ),
            Query::Review(state) => QueryResult::Review(
                store
                    .read(move |r| r.review_items(state))
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            ),
            Query::Scope => QueryResult::Scope(
                store
                    .read(|r| r.scope_rules())
                    .await?
                    .iter()
                    .map(crate::scope::format_rule)
                    .collect(),
            ),
            Query::Status => QueryResult::Status(Box::new(self.status().await?)),
            Query::Modules => QueryResult::Modules(self.modules()),
            Query::Playbooks => QueryResult::Playbooks(crate::workflow::playbooks(&self.shared)),
            Query::Runs => QueryResult::Runs(crate::workflow::runs(&self.shared).await?),
            Query::Checklist(state) => {
                QueryResult::Checklist(crate::workflow::checklist(&self.shared, state, None).await?)
            }
            Query::Launchers(e) => {
                QueryResult::Launchers(crate::workflow::launchers_for(&self.shared, e).await?)
            }
            Query::Raw(id) => {
                QueryResult::Raw(crate::evidence::raw(&self.shared, id).await?.map(Box::new))
            }
            Query::Blob(hex) => QueryResult::Blob(match spektr_store::Sha256::from_hex(&hex) {
                Some(sha) => store.read(move |r| r.blob(sha)).await?,
                None => None,
            }),
        })
    }

    pub async fn status(&self) -> EngineResult<CaseStatus> {
        let s = self.shared.settings();
        let profile = self.shared.profile()?;
        let subject = s.subject;
        let path = self.shared.store.path().display().to_string();
        let counts = self
            .shared
            .store
            .read(move |r| {
                let subject = match subject {
                    Some(id) => r.summary(r.resolve(id)?)?,
                    None => None,
                };
                let jobs = r
                    .job_counts()?
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect();
                Ok((
                    subject,
                    r.entity_count()?,
                    r.edge_count()?,
                    r.observation_count()?,
                    r.audit_count()?,
                    r.findings()?.len() as u64,
                    r.review_items(Some(ReviewState::Pending))?.len() as u64,
                    jobs,
                ))
            })
            .await?;
        Ok(CaseStatus {
            name: s.name,
            path,
            profile: profile.name.clone(),
            profile_summary: profile.summary(),
            depth: self.shared.depth_limit(),
            region: s.region,
            last_seen: s.last_seen,
            subject: counts.0,
            entities: counts.1,
            edges: counts.2,
            observations: counts.3,
            audit_rows: counts.4,
            findings: counts.5,
            review_pending: counts.6,
            jobs: counts.7,
        })
    }

    /// Registered modules with their status under the active profile.
    pub fn modules(&self) -> Vec<ModuleInfo> {
        let profile = self.shared.profile().ok();
        read!(self.shared.modules)
            .values()
            .map(|m| {
                let man: &Manifest = m.manifest();
                ModuleInfo {
                    name: man.name.clone(),
                    version: man.version.clone(),
                    description: man.description.clone(),
                    runtime: man.runtime,
                    technique: man.technique,
                    auto: man.auto,
                    consumes: man.consumes.clone(),
                    produces: man.produces.clone(),
                    cost: man.cost.class,
                    daily_budget: man.cost.daily_budget,
                    missing_secrets: self.shared.missing_secrets(man),
                    hosts: man.hosts.clone(),
                    blocked: profile
                        .as_ref()
                        .and_then(|p| p.block_reason(&man.name, man.technique)),
                }
            })
            .collect()
    }

    /// The vocabulary, for callers that normalize input themselves.
    pub fn vocabulary(&self) -> Arc<spektr_model::Vocabulary> {
        self.shared.vocab()
    }

    /// Findings, notes and scope rules, used by `export`.
    pub async fn scope_rules(&self) -> EngineResult<Vec<ScopeRuleRow>> {
        Ok(self.shared.store.read(|r| r.scope_rules()).await?)
    }
}
