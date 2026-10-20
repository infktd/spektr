//! The module manifest and the exec-protocol message types (docs/MODULES.md §2–3).
//!
//! Third-party Rust modules depend on this crate alone, so it stays light: no
//! async runtime, no I/O, no C dependencies (ARCHITECTURE §2, rule 3). JSON
//! Schemas are generated from these types behind the `schema` feature.

/// Exec protocol version spoken by this crate.
pub const PROTOCOL_VERSION: u32 = 1;
