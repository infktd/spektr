//! The engine: scheduler, module host, ingestion, correlation and the
//! `EngineHandle` that the TUI and the headless CLI talk to
//! (docs/ARCHITECTURE.md §3–4, docs/MODULES.md §4–5).
//!
//! The engine never depends on `spektr-modules` or `spektr-tui`: built-in
//! modules and the capture service are injected by the binary.

pub mod capture;
pub mod correlate;
pub mod eligibility;
mod engine;
mod error;
pub mod events;
mod evidence;
pub mod exec;
pub mod export;
pub mod findings;
mod handle;
mod ingest;
pub mod launcher;
pub mod module;
pub mod pin;
mod plan;
pub mod playbook;
pub mod profile;
mod quiesce;
mod runner;
mod scheduler;
pub mod scope;
mod workflow;

pub use capture::{CaptureError, CaptureRequest, CaptureService, Captured, MAX_CAPTURE};
pub use engine::{CaseSettings, Engine, EngineConfig};
pub use error::{EngineError, EngineResult};
pub use events::{EngineEvent, EventKind};
pub use evidence::{RawView, is_image};
pub use handle::{
    ActionOutcome, CaseStatus, EdgeDetail, EngineAction, EngineHandle, EntityDetail, EntityRowView,
    FindingView, JobView, ModuleInfo, NoteView, ObservationView, Query, QueryResult, ReviewView,
};
pub use module::{
    Emitter, EntityView, JobInput, JobLimits, Module, ModuleCx, ModuleError, Secrets,
};
pub use plan::{PlanOutcome, PlanResult};
pub use profile::{Profiles, RulesProfile};
pub use workflow::{ItemView, LauncherView, PlaybookInfo, RunView, StepInfo, StepView};

// Manifest vocabulary the TUI shows (cost, technique, runtime).
pub use spektr_protocol::{CostClass, LogLevel, Runtime, Technique};

// View-ready types the TUI and CLI use, re-exported so they never depend on
// the store crate directly (ARCHITECTURE §2, rule 2).
pub use spektr_store::{
    CaseInit, Degree, Direction, EgoGroup, EgoParams, EgoRow, EgoView, EntitySummary,
    FindingStatus, GroupKey, ItemState, JobState, OpenOptions, PathStep, PathView, ReviewKind,
    ReviewState, RunState, StepState, TimelineEvent, Trigger,
};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
