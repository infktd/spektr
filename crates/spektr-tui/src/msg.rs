//! Messages into `update`, and the effects it asks the runtime to perform
//! (ARCHITECTURE §3).

use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent};
use jiff::Timestamp;
use spektr_engine::{ActionOutcome, EngineAction, EngineEvent, Query, QueryResult};
use spektr_model::{EdgeId, EntityId, ObservationId, ReviewItemId};

use crate::host::{Chain, DoctorLine, NewCase, RecentCase};

/// What a query result is for, so stale replies can be dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tag {
    Status,
    Entities,
    Modules,
    Jobs,
    /// The ego view for `focus` at generation `gen`.
    Ego {
        focus: EntityId,
        generation: u64,
    },
    Detail(EntityId),
    /// Resolve an entity for a pivot or seed.
    Find,
    /// Resolve the `:path` target.
    FindPath,
    Runs,
    Playbooks,
    Checklist,
    Review,
    /// One side of a `same_person` comparison (`true`: the `a` side).
    ReviewSide(ReviewItemId, bool),
    Timeline,
    Path(EntityId, EntityId),
    Edge(EdgeId),
    Findings,
    Finding(i64),
    /// Candidates for `a` on a finding.
    Candidates(i64),
    /// Export text: to the clipboard (`None`) or a file.
    Export(Option<PathBuf>),
}

#[derive(Debug)]
pub enum Msg {
    Key(KeyEvent),
    /// A mouse event and its time in milliseconds (for double-click).
    Mouse(MouseEvent, u64),
    Paste(String),
    Resize(u16, u16),
    Tick(Timestamp),
    /// Debounced re-query after engine events.
    Refresh,
    Engine(Box<EngineEvent>),
    /// The broadcast lagged: re-query everything.
    Lagged,
    Recent(Vec<RecentCase>),
    Doctor(Vec<DoctorLine>),
    Profiles(Vec<String>),
    Verified(PathBuf, Chain),
    Opened(PathBuf, Result<(), String>),
    /// Creating a case failed (success arrives as `Opened`).
    Created(Result<PathBuf, String>),
    Query(Tag, Result<QueryResult, String>),
    Acted {
        what: String,
        result: Result<ActionOutcome, String>,
    },
    /// The tutor's case opened with this progress.
    TutorStarted(usize, usize),
    /// An export was written (or failed).
    Wrote(Result<PathBuf, String>),
    /// `$EDITOR` closed on a finding's text.
    Edited(i64, String),
    /// Test mode recorded an open instead of performing it.
    WouldOpen(String),
    /// An image finished decoding and encoding (TUI_SPEC §9).
    Image(crate::images::Key, Result<crate::images::Ready, String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    LoadRecent,
    LoadDoctor,
    LoadProfiles,
    Verify(PathBuf),
    Open(PathBuf),
    Create(NewCase),
    Query(Tag, Query),
    /// Run an engine action; `what` describes it for errors and toasts.
    Act {
        action: EngineAction,
        what: String,
    },
    /// Send `Msg::Refresh` after the debounce interval.
    ScheduleRefresh,
    /// OSC 52 clipboard.
    Copy(String),
    OpenUrl(String),
    /// Write a blob to a temp file and open it in the system viewer, or
    /// reveal it in the file manager.
    OpenBlob {
        sha: String,
        name: String,
        reveal: bool,
    },
    /// An observation: a capture's screenshot or an image in the viewer,
    /// anything else in `$PAGER`.
    OpenEvidence(ObservationId),
    /// Read a local file and attach it (`:attach <path>`).
    AttachFile {
        path: PathBuf,
        entity: Option<EntityId>,
    },
    /// Save tutor progress.
    SaveTutor(usize, usize),
    /// Start (or restart) the tutor.
    StartTutor(bool),
    /// Write text to a file (exports).
    WriteFile(PathBuf, String),
    /// Edit a finding's text in `$EDITOR`.
    EditExternal(i64, String),
    /// Swap the image protocol (`:images <mode>`).
    SetImages(crate::images::Mode),
    SetMouse(bool),
    Quit,
}
