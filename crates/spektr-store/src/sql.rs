//! Small conversions between model types and SQLite values.

use jiff::Timestamp;
use rusqlite::Row;
use rusqlite::types::Type;
use uuid::Uuid;

use crate::error::{StoreResult, invalid};

pub(crate) fn uuid_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Uuid> {
    let bytes: Vec<u8> = row.get(idx)?;
    Uuid::from_slice(&bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Blob, Box::new(e)))
}

pub(crate) fn opt_uuid_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<Uuid>> {
    let bytes: Option<Vec<u8>> = row.get(idx)?;
    bytes
        .map(|b| {
            Uuid::from_slice(&b).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(idx, Type::Blob, Box::new(e))
            })
        })
        .transpose()
}

pub(crate) fn ts_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Timestamp> {
    let s: String = row.get(idx)?;
    s.parse().map_err(|e: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e))
    })
}

pub(crate) fn opt_ts_col(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<Timestamp>> {
    let s: Option<String> = row.get(idx)?;
    s.map(|s| {
        s.parse().map_err(|e: jiff::Error| {
            rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e))
        })
    })
    .transpose()
}

pub(crate) fn json_col<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<T> {
    let s: String = row.get(idx)?;
    serde_json::from_str(&s)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

pub(crate) fn parse_col<T: std::str::FromStr>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: String = row.get(idx)?;
    s.parse()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

pub(crate) fn ts(t: Timestamp) -> String {
    spektr_model::time::format_ts(t)
}

/// A 32-byte digest column.
pub(crate) fn digest(bytes: &[u8]) -> StoreResult<[u8; 32]> {
    bytes.try_into().map_err(|_| {
        invalid(format!(
            "expected a 32-byte digest, got {} bytes",
            bytes.len()
        ))
    })
}

/// Typed ids as SQLite BLOB parameters.
pub(crate) trait IdBytes: Copy {
    fn uuid(self) -> Uuid;
}

macro_rules! id_bytes {
    ($($t:ty),*) => { $(impl IdBytes for $t { fn uuid(self) -> Uuid { self.0 } })* };
}
id_bytes!(
    spektr_model::EntityId,
    spektr_model::EdgeId,
    spektr_model::ObservationId,
    spektr_model::JobId,
    spektr_model::NoteId,
    spektr_model::MergeId,
    spektr_model::ReviewItemId,
    spektr_model::ScopeRuleId,
    spektr_model::PlaybookRunId,
    spektr_model::ChecklistItemId
);

pub(crate) fn idb<T: IdBytes>(id: T) -> Vec<u8> {
    id.uuid().as_bytes().to_vec()
}
