//! spektr's data model: the 15 core entity types, normalization, the predicate
//! vocabulary, attributes, strength and the known-hub list
//! (docs/DATA_MODEL.md §1–5, §7).
//!
//! This crate is pure: no async runtime, no I/O, no C dependencies
//! (ARCHITECTURE §2, rule 3). Everything else in the workspace depends on it,
//! so keep it small.

pub mod actor;
pub mod attrs;
pub mod extension;
pub mod flags;
pub mod hubs;
pub mod ids;
pub mod normalize;
pub mod platforms;
pub mod predicates;
pub mod strength;
pub mod text;
pub mod time;
pub mod types;

pub use actor::Actor;
pub use attrs::{Attr, AttrValue, Attrs};
pub use extension::{ExtNormalize, Vocabulary};
pub use flags::EntityFlags;
pub use hubs::KnownHubs;
pub use ids::{
    ChecklistItemId, EdgeId, EntityId, IdGen, JobId, MergeId, NoteId, ObservationId, PlaybookRunId,
    ReviewItemId, ScopeRuleId,
};
pub use normalize::{
    Derived, KeyHint, NormalizeCtx, NormalizeError, Normalized, has_known_suffix, normalize,
};
pub use platforms::{Platform, PlatformRegistry};
pub use predicates::{ArtifactKind, CorePredicate, Predicate, VocabError};
pub use strength::Strength;
pub use time::Clock;
pub use types::{CoreType, EntityRef, EntityType, ExtName};

/// Name of this crate, for diagnostics.
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
