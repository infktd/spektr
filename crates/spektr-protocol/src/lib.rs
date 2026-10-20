//! The module manifest, the exec-protocol message types and the playbook
//! file format (docs/MODULES.md §2–3, docs/PLAYBOOKS.md §1).
//!
//! Native (Rust) and exec (subprocess) modules produce the same
//! [`ModuleMessage`]s; the engine has exactly one ingestion path. Third-party
//! Rust modules depend on this crate alone, so it stays light: no async
//! runtime, no I/O, no C dependencies (ARCHITECTURE §2, rule 3). JSON Schemas
//! are generated from these types behind the `schema` feature
//! (`cargo xtask schemas`).

pub mod duration;
pub mod manifest;
pub mod messages;
pub mod playbook;

pub use duration::{parse_duration, parse_rate};
pub use manifest::{
    ConfigField, ConfigType, Cost, CostClass, Manifest, ManifestError, ManifestFile, PlatformDecl,
    PredicateDecl, Runtime, Technique, TypeDecl,
};
pub use messages::{
    AttrValue, EntitySpec, ErrorCode, HostMessage, InputEntity, LogLevel, ModuleMessage,
};
pub use playbook::{PlaybookError, PlaybookFile};

/// Exec protocol version spoken by this crate.
pub const PROTOCOL_VERSION: u32 = 1;

/// The longest line either side may send, in bytes (32 MiB).
pub const MAX_LINE: usize = 32 * 1024 * 1024;
