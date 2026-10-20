//! Engine events, broadcast to the TUI and the headless CLI
//! (ARCHITECTURE §3). A lagging subscriber resyncs by re-querying.

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::Serialize;
use spektr_model::{ArtifactKind, EdgeId, JobId, Predicate, ReviewItemId};
use spektr_protocol::LogLevel;
use spektr_store::{EntitySummary, JobState, ReviewKind, Trigger};

#[derive(Debug, Clone, Serialize)]
pub struct EngineEvent {
    pub at: Timestamp,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    JobQueued {
        job: JobId,
        module: String,
        entity: EntitySummary,
        trigger: Trigger,
    },
    JobStarted {
        job: JobId,
        module: String,
        entity: EntitySummary,
    },
    JobProgress {
        job: JobId,
        module: String,
        pct: u8,
        note: Option<String>,
    },
    JobFinished {
        job: JobId,
        module: String,
        entity: EntitySummary,
        state: JobState,
        /// New entities by type.
        found: BTreeMap<String, u32>,
        /// The module's last progress note, e.g. `300 sites · 7 found`.
        note: Option<String>,
        error: Option<String>,
        /// Set when the job will be retried.
        retry_in_secs: Option<u64>,
        attempts_left: Option<u32>,
    },
    JobBlocked {
        job: Option<JobId>,
        module: String,
        entity: EntitySummary,
        reason: String,
    },
    JobCached {
        job: JobId,
        module: String,
        entity: EntitySummary,
    },
    EntityAdded {
        entity: EntitySummary,
        by: String,
    },
    EdgeAdded {
        edge: EdgeId,
        src: EntitySummary,
        predicate: Predicate,
        dst: EntitySummary,
    },
    EdgeChanged {
        edge: EdgeId,
    },
    Correlation {
        rule: String,
        artifact: ArtifactKind,
        value: String,
        a: EntitySummary,
        b: EntitySummary,
    },
    ReviewAdded {
        id: ReviewItemId,
        kind: ReviewKind,
        module: String,
    },
    HubSuppressed {
        entity: EntitySummary,
        degree: u64,
    },
    Budget {
        module: String,
        used: u32,
        limit: u32,
    },
    ModuleLog {
        module: String,
        level: LogLevel,
        msg: String,
    },
    /// A module message failed validation; the job continues.
    MessageRejected {
        module: String,
        job: JobId,
        reason: String,
    },
    /// A store error rolled back an ingestion batch (shown as `hostile`).
    IngestFailed {
        message: String,
    },
    /// Something changed that views should re-query (merge, scope, meta).
    CaseChanged {
        what: String,
    },
}
