//! Built-in modules plus the shared network (`net`), media (`media`) and web
//! capture (`capture`) services (docs/MODULES.md §4, §6, §10).
//!
//! All network-, image- and parsing-heavy code lives here so that editing the
//! engine or the TUI never recompiles it.

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
