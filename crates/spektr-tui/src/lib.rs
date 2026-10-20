//! The terminal UI and the tutor (docs/TUI_SPEC.md).
//!
//! All data comes through `spektr_engine::EngineHandle`; this crate never
//! depends on the store or the modules. Colors live only in `theme.rs` and key
//! bindings only in `keymap.rs`.

pub mod ego;
pub mod entities;
pub mod glyph;
pub mod host;
pub mod keymap;
pub mod layout;
pub mod menu;
pub mod msg;
pub mod palette;
pub mod runtime;
pub mod state;
pub mod text;
pub mod theme;
pub mod update;
pub mod view;

pub use host::{Chain, DoctorLine, Host, Level, NewCase, RecentCase, Settings};
pub use runtime::{Start, run};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
