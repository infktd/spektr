//! The terminal UI and the tutor (docs/TUI_SPEC.md).
//!
//! All data comes through `spektr_engine::EngineHandle`; this crate never
//! depends on the store or the modules. Colors live only in `theme.rs` and key
//! bindings only in `keymap.rs`.

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
