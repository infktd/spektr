//! Messages into `update`, and the effects it asks the runtime to perform
//! (ARCHITECTURE §3).

use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent};
use jiff::Timestamp;
use spektr_engine::{ActionOutcome, EngineAction, EngineEvent, Query, QueryResult};
use spektr_model::EntityId;

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
    SetMouse(bool),
    Quit,
}
