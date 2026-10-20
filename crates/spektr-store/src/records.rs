//! Case records beyond the graph: jobs, findings, notes, scope, settings,
//! review items, merges, playbook runs, steps and checklist items.

use jiff::Timestamp;
use rusqlite::Row;
use spektr_model::{
    ChecklistItemId, EntityId, JobId, MergeId, NoteId, ObservationId, PlaybookRunId, ReviewItemId,
    ScopeRuleId,
};

use crate::rows::str_enum;
use crate::sql::{json_col, opt_ts_col, opt_uuid_col, parse_col, ts_col, uuid_col};

str_enum!(
    /// Why a job exists.
    Trigger { Auto => "auto", Manual => "manual", Playbook => "playbook", Tutor => "tutor" }
);

str_enum!(
    JobState {
        Queued => "queued",
        Running => "running",
        Done => "done",
        Failed => "failed",
        Blocked => "blocked",
        Cached => "cached",
    }
);

impl JobState {
    pub fn is_terminal(self) -> bool {
        !matches!(self, JobState::Queued | JobState::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: JobId,
    pub module: String,
    pub input_entity: EntityId,
    pub trigger: Trigger,
    pub run_id: Option<PlaybookRunId>,
    pub cached_observation: Option<ObservationId>,
    pub joined_job: Option<JobId>,
    pub depth: u32,
    pub state: JobState,
    pub reason: Option<String>,
    pub error: Option<String>,
    pub queued_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

pub(crate) const JOB_COLS: &str = "id, module, input_entity, trigger, run_id, cached_observation, joined_job, depth, state, reason, error, queued_at, started_at, finished_at";

impl JobRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: JobId(uuid_col(r, 0)?),
            module: r.get(1)?,
            input_entity: EntityId(uuid_col(r, 2)?),
            trigger: parse_col(r, 3)?,
            run_id: opt_uuid_col(r, 4)?.map(PlaybookRunId),
            cached_observation: opt_uuid_col(r, 5)?.map(ObservationId),
            joined_job: opt_uuid_col(r, 6)?.map(JobId),
            depth: r.get(7)?,
            state: parse_col(r, 8)?,
            reason: r.get(9)?,
            error: r.get(10)?,
            queued_at: ts_col(r, 11)?,
            started_at: opt_ts_col(r, 12)?,
            finished_at: opt_ts_col(r, 13)?,
        })
    }
}

str_enum!(
    FindingStatus { Draft => "draft", Ready => "ready", Sent => "sent" }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingRow {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub claim: String,
    pub relevance: String,
    pub status: FindingStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

pub(crate) const FINDING_COLS: &str =
    "id, title, category, claim, relevance, status, created_at, updated_at";

impl FindingRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            title: r.get(1)?,
            category: r.get(2)?,
            claim: r.get(3)?,
            relevance: r.get(4)?,
            status: parse_col(r, 5)?,
            created_at: ts_col(r, 6)?,
            updated_at: ts_col(r, 7)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteRow {
    pub id: NoteId,
    pub entity: Option<EntityId>,
    pub finding: Option<i64>,
    pub body: String,
    pub created_at: Timestamp,
}

impl NoteRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: NoteId(uuid_col(r, 0)?),
            entity: opt_uuid_col(r, 1)?.map(EntityId),
            finding: r.get(2)?,
            body: r.get(3)?,
            created_at: ts_col(r, 4)?,
        })
    }
}

str_enum!(
    ScopeKind { Allow => "allow", Deny => "deny" }
);

str_enum!(
    ScopeMatcher {
        DomainSuffix => "domain_suffix",
        Cidr => "cidr",
        Platform => "platform",
        Regex => "regex",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRuleRow {
    pub id: ScopeRuleId,
    pub kind: ScopeKind,
    pub matcher: ScopeMatcher,
    pub pattern: String,
    pub created_at: Timestamp,
}

impl ScopeRuleRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: ScopeRuleId(uuid_col(r, 0)?),
            kind: parse_col(r, 1)?,
            matcher: parse_col(r, 2)?,
            pattern: r.get(3)?,
            created_at: ts_col(r, 4)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSettingRow {
    pub module: String,
    pub enabled: bool,
    pub config: serde_json::Value,
}

str_enum!(
    ReviewKind { SweepUnsure => "sweep_unsure", SamePerson => "same_person" }
);

str_enum!(
    ReviewState { Pending => "pending", Accepted => "accepted", Rejected => "rejected", Later => "later" }
);

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewItemRow {
    pub id: ReviewItemId,
    pub kind: ReviewKind,
    pub payload: serde_json::Value,
    pub state: ReviewState,
    pub created_at: Timestamp,
    pub decided_at: Option<Timestamp>,
}

impl ReviewItemRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: ReviewItemId(uuid_col(r, 0)?),
            kind: parse_col(r, 1)?,
            payload: json_col(r, 2)?,
            state: parse_col(r, 3)?,
            created_at: ts_col(r, 4)?,
            decided_at: opt_ts_col(r, 5)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRow {
    pub id: MergeId,
    pub survivor: EntityId,
    pub merged: EntityId,
    /// `(entity, its previous merged_into)` for every entity re-pointed.
    pub repointed: Vec<(EntityId, EntityId)>,
    pub created_at: Timestamp,
    pub undone_at: Option<Timestamp>,
}

impl MergeRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: MergeId(uuid_col(r, 0)?),
            survivor: EntityId(uuid_col(r, 1)?),
            merged: EntityId(uuid_col(r, 2)?),
            repointed: json_col(r, 3)?,
            created_at: ts_col(r, 4)?,
            undone_at: opt_ts_col(r, 5)?,
        })
    }
}

str_enum!(
    RunState { Running => "running", Done => "done", Stopped => "stopped", Failed => "failed" }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookRunRow {
    pub id: PlaybookRunId,
    pub playbook: String,
    pub playbook_version: String,
    pub playbook_hash: String,
    pub seed_entity: EntityId,
    pub parent_run: Option<PlaybookRunId>,
    pub state: RunState,
    pub current_step: u32,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

pub(crate) const RUN_COLS: &str = "id, playbook, playbook_version, playbook_hash, seed_entity, parent_run, state, current_step, started_at, finished_at";

impl PlaybookRunRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: PlaybookRunId(uuid_col(r, 0)?),
            playbook: r.get(1)?,
            playbook_version: r.get(2)?,
            playbook_hash: r.get(3)?,
            seed_entity: EntityId(uuid_col(r, 4)?),
            parent_run: opt_uuid_col(r, 5)?.map(PlaybookRunId),
            state: parse_col(r, 6)?,
            current_step: r.get(7)?,
            started_at: ts_col(r, 8)?,
            finished_at: opt_ts_col(r, 9)?,
        })
    }
}

str_enum!(
    StepKind { Run => "run", Playbook => "playbook", Check => "check" }
);

str_enum!(
    StepState { Pending => "pending", Running => "running", Done => "done", Stopped => "stopped" }
);

#[derive(Debug, Clone, PartialEq)]
pub struct PlaybookStepRow {
    pub run_id: PlaybookRunId,
    pub step: u32,
    pub kind: StepKind,
    pub state: StepState,
    pub selected: u32,
    pub capped: u32,
    pub result: serde_json::Value,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

impl PlaybookStepRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            run_id: PlaybookRunId(uuid_col(r, 0)?),
            step: r.get(1)?,
            kind: parse_col(r, 2)?,
            state: parse_col(r, 3)?,
            selected: r.get(4)?,
            capped: r.get(5)?,
            result: json_col(r, 6)?,
            started_at: opt_ts_col(r, 7)?,
            finished_at: opt_ts_col(r, 8)?,
        })
    }
}

str_enum!(
    ItemState { Open => "open", Done => "done", Skipped => "skipped" }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistItemRow {
    pub id: ChecklistItemId,
    pub run_id: PlaybookRunId,
    pub step: u32,
    pub entity: Option<EntityId>,
    pub text: String,
    pub launchers: Vec<String>,
    pub screen: Option<String>,
    pub state: ItemState,
    pub note: Option<String>,
    pub resolved_at: Option<Timestamp>,
}

pub(crate) const ITEM_COLS: &str =
    "id, run_id, step, entity_id, text, launchers, screen, state, note, resolved_at";

impl ChecklistItemRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: ChecklistItemId(uuid_col(r, 0)?),
            run_id: PlaybookRunId(uuid_col(r, 1)?),
            step: r.get(2)?,
            entity: opt_uuid_col(r, 3)?.map(EntityId),
            text: r.get(4)?,
            launchers: json_col(r, 5)?,
            screen: r.get(6)?,
            state: parse_col(r, 7)?,
            note: r.get(8)?,
            resolved_at: opt_ts_col(r, 9)?,
        })
    }
}
