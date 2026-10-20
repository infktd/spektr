//! The engine: scheduler, module host, ingestion, correlation and the
//! `EngineHandle` that the TUI and the headless CLI talk to
//! (docs/ARCHITECTURE.md §3–4, docs/MODULES.md §4–5).
//!
//! The engine never depends on `spektr-modules` or `spektr-tui`: built-in
//! modules and the capture service are injected by the binary.

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
