//! The native module contract (MODULES §4).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use spektr_model::{Attrs, EntityId, EntityType, JobId};
use spektr_protocol::{ErrorCode, Manifest, ModuleMessage};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ingest::IngestMsg;

/// A transform from one entity to observations, entities, edges and
/// attributes. A module never writes to the case file; it emits messages.
pub trait Module: Send + Sync + 'static {
    fn manifest(&self) -> &Manifest;
    fn run<'a>(&'a self, input: JobInput, cx: ModuleCx) -> BoxFuture<'a, Result<(), ModuleError>>;
}

/// The job's input entity, as a module sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub id: EntityId,
    pub ty: EntityType,
    pub key: String,
    pub value: String,
    pub attrs: Attrs,
}

impl EntityView {
    /// For `account` inputs: the platform part of the key.
    pub fn platform(&self) -> Option<&str> {
        if self.ty.as_str() == "account" {
            self.key.split_once(':').map(|(p, _)| p)
        } else {
            None
        }
    }

    /// For `account` inputs: the handle part of the key.
    pub fn handle(&self) -> &str {
        match self.key.split_once(':') {
            Some((_, h)) if self.ty.as_str() == "account" => h,
            _ => &self.key,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobInput {
    pub job: JobId,
    pub entity: EntityView,
    /// Image bytes when `entity.ty` is `image`.
    pub blob: Option<Arc<[u8]>>,
    pub depth: u32,
}

/// Secrets the module declared that are set: required plus optional ones.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets(pub BTreeMap<String, String>);

impl Secrets {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.keys()).finish()
    }
}

/// What a running module gets besides its input.
pub struct ModuleCx {
    pub emit: Emitter,
    pub config: serde_json::Value,
    pub secrets: Secrets,
    pub cancel: CancellationToken,
}

/// Per-job limits (MODULES §3.3), configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobLimits {
    pub messages: u64,
    pub raw_bytes: u64,
}

impl Default for JobLimits {
    fn default() -> Self {
        Self {
            messages: 10_000,
            raw_bytes: 50 * 1024 * 1024,
        }
    }
}

/// Sends a job's messages into the ingestion pipeline, in order. Exceeding a
/// per-job limit fails the job; what was ingested up to then is kept.
#[derive(Clone)]
pub struct Emitter {
    job: JobId,
    tx: mpsc::Sender<IngestMsg>,
    work: crate::quiesce::Work,
    limits: JobLimits,
    messages: Arc<AtomicU64>,
    raw: Arc<AtomicU64>,
}

impl Emitter {
    pub(crate) fn new(
        job: JobId,
        tx: mpsc::Sender<IngestMsg>,
        work: crate::quiesce::Work,
        limits: JobLimits,
    ) -> Self {
        Self {
            job,
            tx,
            work,
            limits,
            messages: Arc::new(AtomicU64::new(0)),
            raw: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn job(&self) -> JobId {
        self.job
    }

    pub async fn emit(&self, msg: ModuleMessage) -> Result<(), ModuleError> {
        let n = self.messages.fetch_add(1, Ordering::Relaxed) + 1;
        if n > self.limits.messages {
            return Err(ModuleError::LimitExceeded(format!(
                "more than {} messages",
                self.limits.messages
            )));
        }
        if let ModuleMessage::Observation { raw, raw_b64, .. } = &msg {
            let len = raw.as_ref().map_or(0, String::len)
                + raw_b64.as_ref().map_or(0, |b| b.len() / 4 * 3);
            let total = self.raw.fetch_add(len as u64, Ordering::Relaxed) + len as u64;
            if total > self.limits.raw_bytes {
                return Err(ModuleError::LimitExceeded(format!(
                    "more than {} bytes of raw data",
                    self.limits.raw_bytes
                )));
            }
        }
        self.work.add(1);
        if self
            .tx
            .send(IngestMsg::Message { job: self.job, msg })
            .await
            .is_err()
        {
            self.work.done(1);
            return Err(ModuleError::Internal("the engine has shut down".into()));
        }
        Ok(())
    }
}

/// How a job ended badly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModuleError {
    #[error("{code}: {message}", code = .code.as_str())]
    Code {
        code: ErrorCode,
        message: String,
        /// The module's own view; only `rate_limited` and `upstream` are
        /// ever retried, and only when this is true.
        retryable: bool,
        retry_after: Option<Duration>,
    },
    #[error("cancelled")]
    Cancelled,
    #[error("limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("{0}")]
    Internal(String),
}

impl ModuleError {
    pub fn code(code: ErrorCode, message: impl Into<String>) -> Self {
        ModuleError::Code {
            code,
            message: message.into(),
            retryable: code.auto_retry(),
            retry_after: None,
        }
    }

    /// Only `rate_limited` and `upstream` are retried automatically.
    pub fn retryable(&self) -> bool {
        matches!(self, ModuleError::Code { code, retryable: true, .. } if code.auto_retry())
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            ModuleError::Code { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}
