//! What the TUI needs from the binary: the recent-cases list, the doctor,
//! opening and creating cases. The binary owns paths, config and the store;
//! the TUI only sees these results (ARCHITECTURE §2, rule 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use futures::future::BoxFuture;
use jiff::Timestamp;
use spektr_engine::EngineHandle;
use spektr_model::EntityType;

use crate::glyph::GlyphSet;
use crate::theme::ColorMode;

/// One row of the launcher's recent-cases table.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentCase {
    pub path: PathBuf,
    pub name: String,
    pub entities: Option<u64>,
    pub edges: Option<u64>,
    pub last_opened: Option<Timestamp>,
    pub chain: Chain,
    pub size: Option<u64>,
    pub seeds: Vec<(EntityType, String)>,
}

/// Audit-chain state as the launcher shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chain {
    /// Not checked yet.
    Unknown,
    Ok {
        rows: u64,
    },
    /// `✕ break at #77`.
    Broken {
        at: String,
    },
    /// The file is gone or unreadable.
    Missing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorLine {
    pub name: String,
    pub level: Level,
    pub detail: String,
}

/// The new-case dialog's fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewCase {
    pub name: String,
    pub profile: String,
    pub subject: Option<String>,
    pub last_seen: Option<String>,
}

/// TUI settings from config.toml, resolved by the binary.
#[derive(Debug, Clone)]
pub struct Settings {
    pub mouse: bool,
    /// `images` in config.toml (TUI_SPEC §9).
    pub images: crate::images::Mode,
    pub glyphs: GlyphSet,
    pub colors: ColorMode,
    pub keys: BTreeMap<String, Vec<String>>,
    pub version: String,
    pub home: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse: true,
            images: crate::images::Mode::Auto,
            glyphs: GlyphSet::Unicode,
            colors: ColorMode::TrueColor,
            keys: BTreeMap::new(),
            version: String::new(),
            home: None,
        }
    }
}

/// Something opened outside the TUI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum External {
    Url(String),
    /// A file in the system viewer.
    Open(PathBuf),
    /// A file selected in the file manager.
    Reveal(PathBuf),
    /// A file in `$PAGER`; blocks until the pager exits.
    Pager(PathBuf),
}

pub trait Host: Send + Sync + 'static {
    /// Recent cases, newest first, with counts and chain state. Blocking;
    /// the runtime calls it off the render path.
    fn recent(&self) -> Vec<RecentCase>;
    /// Doctor checks for the launcher panel. Blocking.
    fn doctor(&self) -> Vec<DoctorLine>;
    /// Re-hash a case's audit chain and blobs. Blocking.
    fn verify(&self, path: &Path) -> Chain;
    /// Rules profiles available for a new case.
    fn profiles(&self) -> Vec<String>;
    /// Open a case and start its engine with every module.
    fn open(&self, path: PathBuf) -> BoxFuture<'_, Result<EngineHandle, String>>;
    /// Create a case in the current directory and start its engine with
    /// every module (the case is then open; it isn't opened twice).
    fn create(&self, req: NewCase) -> BoxFuture<'_, Result<(PathBuf, EngineHandle), String>>;
    /// Record that a case was opened (recent.toml).
    fn opened(&self, path: &Path);
    /// The clock: fixed under `SPEKTR_CLOCK` in test mode.
    fn now(&self) -> Timestamp;
    /// Open a URL, a file or a pager. Test mode records it instead and
    /// returns the target, which the rail shows as `↗ would open …`.
    fn external(&self, what: &External) -> Result<Option<String>, String>;
    /// Whether `External::Pager` takes over the terminal (it doesn't in
    /// test mode).
    fn pager_takes_terminal(&self) -> bool {
        true
    }
}
