//! spektr's data model: the 15 core entity types, normalization, the predicate
//! vocabulary, attributes and strength (docs/DATA_MODEL.md §1–5, §7).
//!
//! This crate is pure: no async runtime, no I/O, no C dependencies
//! (ARCHITECTURE §2, rule 3). Everything else in the workspace depends on it,
//! so keep it small.

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
