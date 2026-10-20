//! The case file: SQLite schema and migrations, the store actor and read pool,
//! repositories, graph queries, blobs and the audit chain
//! (docs/DATA_MODEL.md §6, §8–10, PLAYBOOKS §3).
//!
//! Two layers:
//!
//! - [`Db`]: the synchronous core. It owns one `rusqlite::Connection` and runs
//!   closures in a transaction ([`Tx`]) or against a read view ([`Reader`]).
//!   Every mutation writes exactly one audit row in the same transaction.
//! - [`Store`]: the cheap-to-clone async handle. The writer `Db` lives on a
//!   dedicated OS thread; reads run on a small pool of read-only connections
//!   (WAL allows concurrent readers), so UI queries never wait behind
//!   ingestion (ARCHITECTURE §3).

mod audit;
mod blob;
mod db;
mod error;
mod graph;
mod reads;
mod records;
mod rows;
mod schema;
mod sql;
mod store;
pub mod synth;
mod timeline;
mod tx;

pub use audit::{
    AuditRow, VerifyFailure, VerifyLocation, VerifyReport, canonical_json, chain_hash,
};
pub use blob::{BlobInfo, Sha256, sha256};
pub use db::{CaseInit, Db, OpenOptions};
pub use error::{StoreError, StoreResult};
pub use graph::{
    Degree, Direction, EgoGroup, EgoParams, EgoRow, EgoView, GroupKey, PathStep, PathView,
};
pub use reads::Reader;
pub use records::*;
pub use rows::*;
pub use schema::{APPLICATION_ID, SCHEMA_VERSION};
pub use store::Store;
pub use timeline::TimelineEvent;
pub use tx::{FindingUpdate, IngestedEntity, NewEdge, NewObservation, Tx};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
