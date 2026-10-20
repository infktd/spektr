//! The case file: SQLite schema and migrations, the store actor and read pool,
//! repositories, graph queries, blobs and the audit chain
//! (docs/DATA_MODEL.md §6, §8–10).

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
