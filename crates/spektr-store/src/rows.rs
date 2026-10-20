//! Graph rows: entities, edges, observations.

use jiff::Timestamp;
use rusqlite::Row;
use serde::{Deserialize, Serialize};
use spektr_model::{
    Attrs, EdgeId, EntityFlags, EntityId, EntityType, JobId, ObservationId, Predicate,
};

use crate::blob::Sha256;
use crate::sql::{json_col, opt_ts_col, opt_uuid_col, parse_col, ts_col, uuid_col};

/// A string-backed enum stored as TEXT.
macro_rules! str_enum {
    ($(#[$doc:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $s),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::rows::UnknownVariant;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok($name::$variant),)+
                    _ => Err(crate::rows::UnknownVariant(format!("{}: `{s}`", stringify!($name)))),
                }
            }
        }
    };
}
pub(crate) use str_enum;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value {0}")]
pub struct UnknownVariant(pub String);

str_enum!(
    /// Edge state (DATA_MODEL §4).
    EdgeState { Active => "active", Rejected => "rejected" }
);

str_enum!(
    /// Who wrote an observation (DATA_MODEL §4).
    ObservationKind {
        Module => "module",
        Capture => "capture",
        Attachment => "attachment",
        Analyst => "analyst",
        Correlator => "correlator",
    }
);

/// An entity as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRow {
    pub id: EntityId,
    pub ty: EntityType,
    pub key: String,
    pub value: String,
    pub attrs: Attrs,
    pub flags: EntityFlags,
    pub depth: u32,
    pub merged_into: Option<EntityId>,
    pub created_at: Timestamp,
    pub created_by: String,
}

pub(crate) const ENTITY_COLS: &str =
    "id, type, key, value, attrs, flags, depth, merged_into, created_at, created_by";

impl EntityRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: EntityId(uuid_col(r, 0)?),
            ty: parse_col(r, 1)?,
            key: r.get(2)?,
            value: r.get(3)?,
            attrs: json_col(r, 4)?,
            flags: EntityFlags(r.get(5)?),
            depth: r.get(6)?,
            merged_into: opt_uuid_col(r, 7)?.map(EntityId),
            created_at: ts_col(r, 8)?,
            created_by: r.get(9)?,
        })
    }

    pub fn summary(&self) -> EntitySummary {
        EntitySummary {
            id: self.id,
            ty: self.ty.clone(),
            key: self.key.clone(),
            value: self.value.clone(),
            flags: self.flags,
            depth: self.depth,
        }
    }
}

/// What lists and graph views need about an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySummary {
    pub id: EntityId,
    pub ty: EntityType,
    pub key: String,
    pub value: String,
    pub flags: EntityFlags,
    pub depth: u32,
}

pub(crate) const SUMMARY_COLS: &str = "id, type, key, value, flags, depth";

impl EntitySummary {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: EntityId(uuid_col(r, 0)?),
            ty: parse_col(r, 1)?,
            key: r.get(2)?,
            value: r.get(3)?,
            flags: EntityFlags(r.get(4)?),
            depth: r.get(5)?,
        })
    }
}

/// Edge attributes: a sorted JSON object (`relation`, `role`, `artifact`, …).
pub type EdgeAttrs = serde_json::Map<String, serde_json::Value>;

/// An edge as stored. Endpoints are the entity rows the edge was written
/// against; reads resolve them through `merged_into`.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRow {
    pub id: EdgeId,
    pub src: EntityId,
    pub dst: EntityId,
    pub predicate: Predicate,
    pub discriminator: String,
    pub attrs: EdgeAttrs,
    pub state: EdgeState,
    pub confirmed_at: Option<Timestamp>,
    pub origin: String,
    pub created_at: Timestamp,
}

pub(crate) const EDGE_COLS: &str =
    "id, src, dst, predicate, discriminator, attrs, state, confirmed_at, origin, created_at";

impl EdgeRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: EdgeId(uuid_col(r, 0)?),
            src: EntityId(uuid_col(r, 1)?),
            dst: EntityId(uuid_col(r, 2)?),
            predicate: parse_col(r, 3)?,
            discriminator: r.get(4)?,
            attrs: json_col(r, 5)?,
            state: parse_col(r, 6)?,
            confirmed_at: opt_ts_col(r, 7)?,
            origin: r.get(8)?,
            created_at: ts_col(r, 9)?,
        })
    }
}

/// Raw evidence with provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRow {
    pub id: ObservationId,
    pub job_id: Option<JobId>,
    pub module: String,
    pub module_version: String,
    pub manifest_hash: String,
    pub input_entity: Option<EntityId>,
    pub source_url: Option<String>,
    pub content_type: Option<String>,
    pub blob: Option<Sha256>,
    pub summary: serde_json::Value,
    pub kind: ObservationKind,
    pub created_at: Timestamp,
}

pub(crate) const OBSERVATION_COLS: &str = "id, job_id, module, module_version, manifest_hash, input_entity, source_url, content_type, blob_sha256, summary, kind, created_at";

impl ObservationRow {
    pub(crate) fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        let blob: Option<Vec<u8>> = r.get(8)?;
        Ok(Self {
            id: ObservationId(uuid_col(r, 0)?),
            job_id: opt_uuid_col(r, 1)?.map(JobId),
            module: r.get(2)?,
            module_version: r.get(3)?,
            manifest_hash: r.get(4)?,
            input_entity: opt_uuid_col(r, 5)?.map(EntityId),
            source_url: r.get(6)?,
            content_type: r.get(7)?,
            blob: blob.and_then(|b| Sha256::from_slice(&b)),
            summary: json_col(r, 9)?,
            kind: parse_col(r, 10)?,
            created_at: ts_col(r, 11)?,
        })
    }
}

/// One piece of evidence on an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRow {
    pub edge: EdgeId,
    pub observation: ObservationId,
    pub source_key: String,
}
