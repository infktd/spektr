//! The hash-chained audit log and `verify` (DATA_MODEL §9).
//!
//! `hash = SHA-256(prev_hash ‖ seq as u64 BE ‖ ts ‖ 0x00 ‖ actor ‖ 0x00 ‖
//! action ‖ 0x00 ‖ payload)`; row 1 uses 32 zero bytes as `prev_hash`.
//!
//! The chain alone can't detect truncation of its last rows, so every write
//! transaction also records the chain tip (`audit_tip` in `case_meta`, not
//! itself audited) and `verify` checks the last row against it.

use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::Digest;
use spektr_model::ObservationId;

use crate::blob::{Sha256, decompress, sha256};
use crate::error::StoreResult;
use crate::sql::digest;

/// Canonical JSON: keys sorted, compact, UTF-8. `serde_json::Value`'s map is
/// ordered by key as long as `preserve_order` is off (deps-check enforces it).
pub fn canonical_json<T: Serialize>(value: &T) -> StoreResult<String> {
    let v = serde_json::to_value(value)?;
    Ok(serde_json::to_string(&v)?)
}

pub fn chain_hash(
    prev: &[u8; 32],
    seq: u64,
    ts: &str,
    actor: &str,
    action: &str,
    payload: &str,
) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(prev);
    h.update(seq.to_be_bytes());
    h.update(ts.as_bytes());
    h.update([0u8]);
    h.update(actor.as_bytes());
    h.update([0u8]);
    h.update(action.as_bytes());
    h.update([0u8]);
    h.update(payload.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub seq: i64,
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub payload: String,
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],
}

impl AuditRow {
    pub(crate) fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let prev: Vec<u8> = r.get(5)?;
        let hash: Vec<u8> = r.get(6)?;
        let to32 = |v: Vec<u8>| {
            let mut a = [0u8; 32];
            if v.len() == 32 {
                a.copy_from_slice(&v);
            }
            a
        };
        Ok(Self {
            seq: r.get(0)?,
            ts: r.get(1)?,
            actor: r.get(2)?,
            action: r.get(3)?,
            payload: r.get(4)?,
            prev_hash: to32(prev),
            hash: to32(hash),
        })
    }
}

pub(crate) const AUDIT_COLS: &str = "seq, ts, actor, action, payload, prev_hash, hash";

/// The last `(seq, hash)` in the chain, or `(0, zeros)` for an empty log.
pub(crate) fn load_tip(conn: &Connection) -> StoreResult<(i64, [u8; 32])> {
    let row: Option<(i64, Vec<u8>)> = conn
        .query_row(
            "SELECT seq, hash FROM audit ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => Ok((0, [0u8; 32])),
        Some((seq, hash)) => Ok((seq, digest(&hash)?)),
    }
}

pub(crate) fn append(
    conn: &Connection,
    tip: &mut (i64, [u8; 32]),
    ts: &str,
    actor: &str,
    action: &str,
    payload: &str,
) -> StoreResult<i64> {
    let seq = tip.0 + 1;
    let seq_u = u64::try_from(seq).unwrap_or(0);
    let hash = chain_hash(&tip.1, seq_u, ts, actor, action, payload);
    conn.prepare_cached(
        "INSERT INTO audit (seq, ts, actor, action, payload, prev_hash, hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![seq, ts, actor, action, payload, &tip.1[..], &hash[..]])?;
    *tip = (seq, hash);
    Ok(seq)
}

pub(crate) fn write_tip(conn: &Connection, tip: (i64, [u8; 32])) -> StoreResult<()> {
    let v = serde_json::json!({ "seq": tip.0, "hash": hex(&tip.1) });
    conn.prepare_cached("INSERT INTO case_meta (key, value) VALUES ('audit_tip', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?
        .execute([serde_json::to_string(&v)?])?;
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Where `verify` found a problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyLocation {
    AuditRow(i64),
    /// The recorded chain tip disagrees with the last row (truncation).
    AuditTip,
    Blob(Sha256),
    Observation(ObservationId),
}

impl fmt::Display for VerifyLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyLocation::AuditRow(seq) => write!(f, "audit row #{seq}"),
            VerifyLocation::AuditTip => f.write_str("audit chain tip"),
            VerifyLocation::Blob(sha) => write!(f, "blob {sha}"),
            VerifyLocation::Observation(id) => write!(f, "observation {id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyFailure {
    pub location: VerifyLocation,
    pub reason: String,
}

impl fmt::Display for VerifyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.reason)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub audit_rows: u64,
    pub blobs: u64,
    pub observations: u64,
    /// The first problem found, or `None` when everything checks out.
    pub failure: Option<VerifyFailure>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.failure.is_none()
    }
}

/// Recompute the chain, re-hash every blob, and check every observation's
/// blob exists. Stops at the first failure.
pub fn verify(conn: &Connection) -> StoreResult<VerifyReport> {
    let mut report = VerifyReport {
        audit_rows: 0,
        blobs: 0,
        observations: 0,
        failure: None,
    };
    let fail = |report: &mut VerifyReport, location, reason: String| {
        report.failure = Some(VerifyFailure { location, reason });
    };

    // 1. The chain.
    let mut prev = [0u8; 32];
    let mut expected_seq = 1i64;
    {
        let mut stmt = conn.prepare(&format!("SELECT {AUDIT_COLS} FROM audit ORDER BY seq"))?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let raw_prev: Vec<u8> = r.get(5)?;
            let raw_hash: Vec<u8> = r.get(6)?;
            let row = AuditRow::from_row(r)?;
            if row.seq != expected_seq {
                fail(
                    &mut report,
                    VerifyLocation::AuditRow(expected_seq),
                    format!(
                        "expected row #{expected_seq}, found #{} (rows missing or reordered)",
                        row.seq
                    ),
                );
                return Ok(report);
            }
            if raw_prev.len() != 32 || raw_hash.len() != 32 {
                fail(
                    &mut report,
                    VerifyLocation::AuditRow(row.seq),
                    "hash column is not 32 bytes".into(),
                );
                return Ok(report);
            }
            if row.prev_hash != prev {
                fail(
                    &mut report,
                    VerifyLocation::AuditRow(row.seq),
                    "prev_hash does not match the previous row's hash".into(),
                );
                return Ok(report);
            }
            let seq_u = u64::try_from(row.seq).unwrap_or(0);
            let computed = chain_hash(&prev, seq_u, &row.ts, &row.actor, &row.action, &row.payload);
            if computed != row.hash {
                fail(
                    &mut report,
                    VerifyLocation::AuditRow(row.seq),
                    "hash does not match the row's contents".into(),
                );
                return Ok(report);
            }
            prev = row.hash;
            expected_seq += 1;
            report.audit_rows += 1;
        }
    }
    let tip: Option<String> = conn
        .query_row(
            "SELECT value FROM case_meta WHERE key = 'audit_tip'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(tip) = tip {
        let v: serde_json::Value = serde_json::from_str(&tip)?;
        let seq = v
            .get("seq")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);
        let hash = v
            .get("hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let last = expected_seq - 1;
        if seq != last || hash != hex(&prev) {
            fail(
                &mut report,
                VerifyLocation::AuditTip,
                format!(
                    "the chain ends at row #{last} but the case recorded row #{seq} as its last"
                ),
            );
            return Ok(report);
        }
    }

    // 2. Blobs.
    {
        let mut stmt =
            conn.prepare("SELECT sha256, size, compression, data FROM blob ORDER BY sha256")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let key: Vec<u8> = r.get(0)?;
            let size: i64 = r.get(1)?;
            let compression: String = r.get(2)?;
            let data: Vec<u8> = r.get(3)?;
            let Some(sha) = Sha256::from_slice(&key) else {
                fail(
                    &mut report,
                    VerifyLocation::Blob(Sha256([0; 32])),
                    "blob key is not 32 bytes".into(),
                );
                return Ok(report);
            };
            let bytes = match decompress(&data, &compression, u64::try_from(size).unwrap_or(0)) {
                Ok(b) => b,
                Err(e) => {
                    fail(
                        &mut report,
                        VerifyLocation::Blob(sha),
                        format!("cannot decompress: {e}"),
                    );
                    return Ok(report);
                }
            };
            if sha256(&bytes) != sha {
                fail(
                    &mut report,
                    VerifyLocation::Blob(sha),
                    "content does not match its SHA-256".into(),
                );
                return Ok(report);
            }
            report.blobs += 1;
        }
    }

    // 3. Every observation's blob exists.
    {
        let mut stmt = conn.prepare(
            "SELECT o.id, o.blob_sha256, b.sha256 IS NOT NULL FROM observation o LEFT JOIN blob b ON b.sha256 = o.blob_sha256 ORDER BY o.id",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let id = ObservationId(crate::sql::uuid_col(r, 0)?);
            let has_ref: Option<Vec<u8>> = r.get(1)?;
            let exists: bool = r.get(2)?;
            if has_ref.is_some() && !exists {
                fail(
                    &mut report,
                    VerifyLocation::Observation(id),
                    "its blob is missing".into(),
                );
                return Ok(report);
            }
            report.observations += 1;
        }
    }
    Ok(report)
}
