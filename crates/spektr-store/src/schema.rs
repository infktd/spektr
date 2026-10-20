//! Embedded migrations (DATA_MODEL §8). A case created by a newer schema
//! refuses to open; it is never downgraded.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{StoreError, StoreResult};

/// `PRAGMA application_id`: "SPKR".
pub const APPLICATION_ID: i32 = 0x5350_4B52;
pub const SCHEMA_VERSION: i64 = 1;

const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_initial.sql")];

pub(crate) fn configure(conn: &Connection) -> StoreResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    Ok(())
}

/// Check identity and version, then apply pending migrations.
pub(crate) fn migrate(conn: &mut Connection, path: &Path, creating: bool) -> StoreResult<()> {
    let app_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let objects: i64 = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))?;
    if creating {
        if objects != 0 || version != 0 {
            return Err(StoreError::Exists(path.to_path_buf()));
        }
        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    } else if app_id != APPLICATION_ID {
        return Err(StoreError::NotACase(path.to_path_buf()));
    }
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            path: path.to_path_buf(),
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let start = usize::try_from(version).unwrap_or(0);
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(start) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(
            None,
            "user_version",
            i64::try_from(i + 1).unwrap_or(i64::MAX),
        )?;
        tx.commit()?;
    }
    Ok(())
}

pub(crate) fn quick_check(conn: &Connection, path: &Path) -> StoreResult<()> {
    let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if result != "ok" {
        return Err(StoreError::Corrupt {
            path: path.to_path_buf(),
            detail: result,
        });
    }
    Ok(())
}
