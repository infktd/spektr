//! The synchronous case-file handle.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::civil::Date;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use spektr_model::{
    Actor, Clock, CoreType, EntityFlags, IdGen, KeyHint, KnownHubs, NormalizeCtx, Vocabulary,
    normalize,
};

use crate::audit::{self, VerifyReport};
use crate::error::{StoreError, StoreResult, invalid};
use crate::reads::Reader;
use crate::schema;
use crate::tx::Tx;

/// Shared services the store needs.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    pub clock: Clock,
    pub ids: Arc<IdGen>,
    pub hubs: Arc<KnownHubs>,
    /// Largest blob accepted, in bytes.
    pub max_blob: usize,
    /// Read-only connections in the [`crate::Store`] pool.
    pub readers: usize,
}

impl OpenOptions {
    pub fn new(clock: Clock, ids: IdGen) -> StoreResult<Self> {
        Ok(Self {
            clock,
            ids: Arc::new(ids),
            hubs: Arc::new(
                KnownHubs::builtin().map_err(|e| invalid(format!("built-in hub list: {e}")))?,
            ),
            max_blob: 64 * 1024 * 1024,
            readers: 4,
        })
    }

    /// System clock, random ids.
    pub fn system() -> StoreResult<Self> {
        Self::new(Clock::System, IdGen::random())
    }
}

/// What `spektr new` knows about a case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseInit {
    pub name: String,
    pub subject: Option<String>,
    pub last_seen: Option<Date>,
    pub rules_profile: String,
    pub default_depth: u32,
    pub default_region: String,
    pub hub_threshold: u32,
}

impl CaseInit {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            subject: None,
            last_seen: None,
            rules_profile: "default".into(),
            default_depth: 2,
            default_region: "US".into(),
            hub_threshold: 50,
        }
    }
}

/// One connection to a case file.
#[derive(Debug)]
pub struct Db {
    conn: Connection,
    path: PathBuf,
    opts: OpenOptions,
    read_only: bool,
}

impl Db {
    /// Create a new case file. Fails if `path` exists.
    pub fn create(path: &Path, init: &CaseInit, opts: OpenOptions) -> StoreResult<Self> {
        if path.exists() {
            return Err(StoreError::Exists(path.to_path_buf()));
        }
        let mut conn = Connection::open(path)?;
        schema::configure(&conn)?;
        schema::migrate(&mut conn, path, true)?;
        let mut db = Self {
            conn,
            path: path.to_path_buf(),
            opts,
            read_only: false,
        };
        let init = init.clone();
        db.write(|tx| {
            let actor = Actor::Analyst;
            let meta = serde_json::json!({
                "name": init.name,
                "created_at": crate::sql::ts(tx.now()),
                "last_seen": init.last_seen.map(|d| d.to_string()),
                "rules_profile": init.rules_profile,
                "default_depth": init.default_depth,
                "default_region": init.default_region,
                "hub_threshold": init.hub_threshold,
                "schema_version": schema::SCHEMA_VERSION,
                "spektr_version": env!("CARGO_PKG_VERSION"),
            });
            tx.audit(&actor, "case.create", &meta)?;
            if let Some(obj) = meta.as_object() {
                for (k, v) in obj {
                    if !v.is_null() {
                        tx.put_meta_unaudited(k, v)?;
                    }
                }
            }
            if let Some(subject) = &init.subject {
                let vocab = Vocabulary::builtin();
                let id = tx.new_uuid();
                let n = normalize(
                    &CoreType::Person.into(),
                    subject,
                    KeyHint::NewUuid(id),
                    NormalizeCtx {
                        vocab: &vocab,
                        default_region: None,
                    },
                )?;
                let e = tx.ingest_entity(&actor, &n, 0, None)?;
                tx.set_flags(&actor, e.id, EntityFlags::SUBJECT, EntityFlags::NONE)?;
                tx.set_meta(
                    &actor,
                    "subject_entity",
                    &serde_json::json!(e.id.to_string()),
                )?;
            }
            Ok(())
        })?;
        Ok(db)
    }

    /// Open an existing case for writing: identity and schema checks, a
    /// `quick_check`, then pending migrations.
    pub fn open(path: &Path, opts: OpenOptions) -> StoreResult<Self> {
        if !path.exists() {
            return Err(StoreError::NotFound(path.display().to_string()));
        }
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        schema::configure(&conn)?;
        schema::quick_check(&conn, path)?;
        schema::migrate(&mut conn, path, false)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            opts,
            read_only: false,
        })
    }

    /// A read-only connection (the read pool, `verify`, `export`).
    pub fn open_read_only(path: &Path, opts: OpenOptions) -> StoreResult<Self> {
        if !path.exists() {
            return Err(StoreError::NotFound(path.display().to_string()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        let app_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if app_id != schema::APPLICATION_ID {
            return Err(StoreError::NotACase(path.to_path_buf()));
        }
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > schema::SCHEMA_VERSION {
            return Err(StoreError::NewerSchema {
                path: path.to_path_buf(),
                found: version,
                supported: schema::SCHEMA_VERSION,
            });
        }
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            opts,
            read_only: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn options(&self) -> &OpenOptions {
        &self.opts
    }

    /// Run `f` in one write transaction. Every mutation inside writes its
    /// audit row in this transaction; an error rolls everything back.
    pub fn write<T>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> StoreResult<T>) -> StoreResult<T> {
        if self.read_only {
            return Err(invalid("this connection is read-only"));
        }
        let now = self.opts.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut t = Tx::begin(tx, now, &self.opts)?;
        let out = f(&mut t)?;
        t.commit()?;
        Ok(out)
    }

    /// Run `f` against a consistent read snapshot.
    pub fn read<T>(&mut self, f: impl FnOnce(&Reader<'_>) -> StoreResult<T>) -> StoreResult<T> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let out = f(&Reader::new(&tx, &self.opts))?;
        tx.finish()?;
        Ok(out)
    }

    /// Reads outside a transaction (each statement sees the latest commit).
    pub fn reader(&self) -> Reader<'_> {
        Reader::new(&self.conn, &self.opts)
    }

    /// `spektr verify`: recompute the chain, re-hash blobs, check
    /// observations' blobs exist.
    pub fn verify(&mut self) -> StoreResult<VerifyReport> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let report = audit::verify(&tx)?;
        tx.finish()?;
        Ok(report)
    }

    /// The raw connection, for tests that need to tamper with the file.
    #[doc(hidden)]
    pub fn raw_connection(&self) -> &Connection {
        &self.conn
    }
}
