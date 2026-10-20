//! The async store handle: a writer thread owning the write connection, and
//! a pool of read-only connections on their own threads (ARCHITECTURE §3).

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use tokio::sync::oneshot;

use crate::audit::VerifyReport;
use crate::db::{CaseInit, Db, OpenOptions};
use crate::error::{StoreError, StoreResult};
use crate::reads::Reader;
use crate::tx::Tx;

type WriteJob = Box<dyn FnOnce(&mut Db) + Send>;
type ReadJob = Box<dyn FnOnce(&mut Db) + Send>;

/// Cheap to clone. Dropping the last clone shuts the threads down.
#[derive(Clone)]
pub struct Store {
    writer: mpsc::Sender<WriteJob>,
    readers: mpsc::Sender<ReadJob>,
    inner: Arc<Inner>,
}

struct Inner {
    path: std::path::PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.inner.path)
            .finish()
    }
}

impl Store {
    /// Open an existing case.
    pub fn open(path: &Path, opts: OpenOptions) -> StoreResult<Self> {
        let db = Db::open(path, opts.clone())?;
        Self::spawn(db, opts)
    }

    /// Create a new case.
    pub fn create(path: &Path, init: &CaseInit, opts: OpenOptions) -> StoreResult<Self> {
        let db = Db::create(path, init, opts.clone())?;
        Self::spawn(db, opts)
    }

    fn spawn(writer_db: Db, opts: OpenOptions) -> StoreResult<Self> {
        let path = writer_db.path().to_path_buf();
        let (wtx, wrx) = mpsc::channel::<WriteJob>();
        thread::Builder::new()
            .name("spektr-store-writer".into())
            .spawn(move || {
                let mut db = writer_db;
                while let Ok(job) = wrx.recv() {
                    job(&mut db);
                }
            })?;

        let (rtx, rrx) = mpsc::channel::<ReadJob>();
        let rrx = Arc::new(Mutex::new(rrx));
        for i in 0..opts.readers.max(1) {
            let mut db = Db::open_read_only(&path, opts.clone())?;
            let rrx = Arc::clone(&rrx);
            thread::Builder::new()
                .name(format!("spektr-store-reader-{i}"))
                .spawn(move || {
                    loop {
                        // A poisoned lock means another reader panicked while
                        // holding it; the receiver itself is still usable.
                        let job = {
                            let guard = rrx.lock().unwrap_or_else(|e| e.into_inner());
                            guard.recv()
                        };
                        match job {
                            Ok(job) => job(&mut db),
                            Err(_) => break,
                        }
                    }
                })?;
        }
        Ok(Self {
            writer: wtx,
            readers: rtx,
            inner: Arc::new(Inner { path }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Run `f` in a write transaction on the writer thread.
    pub async fn write<T, F>(&self, f: F) -> StoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Tx<'_>) -> StoreResult<T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.writer
            .send(Box::new(move |db: &mut Db| {
                let _ = tx.send(db.write(f));
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }

    /// Run `f` against a read snapshot on a pool thread.
    pub async fn read<T, F>(&self, f: F) -> StoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Reader<'_>) -> StoreResult<T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.readers
            .send(Box::new(move |db: &mut Db| {
                let _ = tx.send(db.read(f));
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }

    /// `write` for callers without an async runtime.
    pub fn write_blocking<T, F>(&self, f: F) -> StoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Tx<'_>) -> StoreResult<T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.writer
            .send(Box::new(move |db: &mut Db| {
                let _ = tx.send(db.write(f));
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.blocking_recv().map_err(|_| StoreError::Closed)?
    }

    /// `read` for callers without an async runtime.
    pub fn read_blocking<T, F>(&self, f: F) -> StoreResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Reader<'_>) -> StoreResult<T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.readers
            .send(Box::new(move |db: &mut Db| {
                let _ = tx.send(db.read(f));
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.blocking_recv().map_err(|_| StoreError::Closed)?
    }

    pub async fn verify(&self) -> StoreResult<VerifyReport> {
        let (tx, rx) = oneshot::channel();
        self.readers
            .send(Box::new(move |db: &mut Db| {
                let _ = tx.send(db.verify());
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }

    /// Wait until every write queued so far has committed, then checkpoint
    /// the WAL into the main file.
    pub async fn flush(&self) -> StoreResult<()> {
        let (tx, rx) = oneshot::channel();
        self.writer
            .send(Box::new(move |db: &mut Db| {
                let r = db
                    .raw_connection()
                    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
                    .map_err(StoreError::from);
                let _ = tx.send(r);
            }))
            .map_err(|_| StoreError::Closed)?;
        rx.await.map_err(|_| StoreError::Closed)?
    }
}
