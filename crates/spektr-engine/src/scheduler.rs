//! The scheduler task (ARCHITECTURE §3): a priority queue (manual, tutor and
//! playbook before automatic; then shallower depth; then first come), a
//! global job limit, per-module concurrency, and start pacing for exec
//! modules (their traffic is invisible to `spektr-modules::net`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use spektr_model::{EntityId, JobId, PlaybookRunId};
use spektr_store::Trigger;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::engine::{Shared, read};
use crate::ingest::{IngestMsg, JobCtx};
use crate::module::{Emitter, EntityView, JobInput, ModuleCx, ModuleError, Secrets};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedJob {
    pub job: JobId,
    pub module: String,
    pub entity: EntityId,
    pub depth: u32,
    pub trigger: Trigger,
    pub run_id: Option<PlaybookRunId>,
    /// Retries so far (at most 2).
    pub attempt: u32,
    pub not_before: Option<Instant>,
}

impl QueuedJob {
    fn priority(&self) -> (u8, u32) {
        let rank = if self.trigger == Trigger::Auto { 1 } else { 0 };
        (rank, self.depth)
    }
}

pub(crate) enum SchedMsg {
    Enqueue(QueuedJob),
    /// A job task finished running (its result is already on its way to
    /// ingestion); free its slot.
    Released {
        job: JobId,
        module: String,
    },
    Cancel(JobId),
    // Sent by the playbook runner's `:playbook stop` (Phase 4).
    #[allow(dead_code)]
    CancelRun(PlaybookRunId),
}

pub(crate) struct Scheduler {
    shared: Arc<Shared>,
    rx: mpsc::UnboundedReceiver<SchedMsg>,
    queue: Vec<(u64, QueuedJob)>,
    seq: u64,
    running: HashMap<JobId, (String, Option<PlaybookRunId>, CancellationToken)>,
    per_module: BTreeMap<String, u32>,
    last_start: BTreeMap<String, Instant>,
}

impl Scheduler {
    pub fn new(shared: Arc<Shared>, rx: mpsc::UnboundedReceiver<SchedMsg>) -> Self {
        Self {
            shared,
            rx,
            queue: Vec::new(),
            seq: 0,
            running: HashMap::new(),
            per_module: BTreeMap::new(),
            last_start: BTreeMap::new(),
        }
    }

    pub async fn run(mut self) {
        loop {
            let wake = self.next_wake();
            let msg = match wake {
                Some(at) => tokio::select! {
                    m = self.rx.recv() => m,
                    _ = tokio::time::sleep_until(at) => { self.dispatch(); continue; }
                },
                None => self.rx.recv().await,
            };
            let Some(msg) = msg else { break };
            match msg {
                SchedMsg::Enqueue(q) => {
                    self.seq += 1;
                    self.queue.push((self.seq, q));
                }
                SchedMsg::Released { job, module } => {
                    self.running.remove(&job);
                    if let Some(n) = self.per_module.get_mut(&module) {
                        *n = n.saturating_sub(1);
                    }
                }
                SchedMsg::Cancel(job) => self.cancel(|j, _| j == job),
                SchedMsg::CancelRun(run) => self.cancel(|_, r| r == Some(run)),
            }
            self.dispatch();
        }
        for (_, (_, _, token)) in self.running.drain() {
            token.cancel();
        }
    }

    /// Cancel matching jobs: running ones through their token, queued ones
    /// by finishing them as cancelled without running.
    fn cancel(&mut self, matches: impl Fn(JobId, Option<PlaybookRunId>) -> bool) {
        for (job, (_, run, token)) in &self.running {
            if matches(*job, *run) {
                token.cancel();
            }
        }
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.queue)
            .into_iter()
            .partition(|(_, q)| matches(q.job, q.run_id));
        self.queue = keep;
        for (_, q) in gone {
            self.shared.work.add(1);
            let tx = self.shared.ingest.clone();
            let work = self.shared.work.clone();
            tokio::spawn(async move {
                if tx
                    .send(IngestMsg::Finished {
                        job: q.job,
                        result: Err(ModuleError::Cancelled),
                        queued: q,
                    })
                    .await
                    .is_err()
                {
                    work.done(1);
                }
            });
        }
    }

    fn pacing(&self, module: &str) -> Option<Duration> {
        if !read!(self.shared.exec_names).contains(module) {
            return None;
        }
        let m = self.shared.module(module)?;
        let (n, per) = spektr_protocol::parse_rate(m.manifest().rate_limit.as_deref()?)?;
        Some(per / n)
    }

    /// When a delayed job (retry backoff, exec pacing) becomes runnable.
    fn next_wake(&self) -> Option<Instant> {
        self.queue
            .iter()
            .filter_map(|(_, q)| {
                let paced = self
                    .pacing(&q.module)
                    .and_then(|gap| self.last_start.get(&q.module).map(|t| *t + gap));
                match (q.not_before, paced) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (a, b) => a.or(b),
                }
            })
            .min()
    }

    fn dispatch(&mut self) {
        self.queue.sort_by_key(|(seq, q)| (q.priority(), *seq));
        let now = Instant::now();
        let mut i = 0;
        while i < self.queue.len() {
            if self.running_total() >= self.shared.config.concurrency {
                break;
            }
            let q = &self.queue[i].1;
            let Some(module) = self.shared.module(&q.module) else {
                i += 1;
                continue;
            };
            let limit = module.manifest().concurrency.max(1);
            let busy = self.per_module.get(&q.module).copied().unwrap_or(0);
            let delayed = q.not_before.is_some_and(|t| t > now);
            let paced = self
                .pacing(&q.module)
                .zip(self.last_start.get(&q.module))
                .is_some_and(|(gap, last)| *last + gap > now);
            if busy >= limit || delayed || paced {
                i += 1;
                continue;
            }
            let (_, q) = self.queue.remove(i);
            *self.per_module.entry(q.module.clone()).or_default() += 1;
            self.last_start.insert(q.module.clone(), now);
            let token = CancellationToken::new();
            self.running
                .insert(q.job, (q.module.clone(), q.run_id, token.clone()));
            let shared = Arc::clone(&self.shared);
            tokio::spawn(async move {
                let module_name = q.module.clone();
                let job = q.job;
                let result = run_job(&shared, &q, token).await;
                shared.work.add(1);
                if shared
                    .ingest
                    .send(IngestMsg::Finished {
                        job,
                        result,
                        queued: q,
                    })
                    .await
                    .is_err()
                {
                    shared.work.done(1);
                }
                let _ = shared.sched.send(SchedMsg::Released {
                    job,
                    module: module_name,
                });
            });
        }
    }

    fn running_total(&self) -> usize {
        self.per_module.values().map(|n| *n as usize).sum()
    }
}

/// Run one job to completion: load its input, announce it to ingestion, run
/// the module under its timeout and cancellation token.
async fn run_job(
    shared: &Arc<Shared>,
    q: &QueuedJob,
    token: CancellationToken,
) -> Result<(), ModuleError> {
    let module = shared.module(&q.module).ok_or_else(|| {
        ModuleError::Internal(format!("module {} is no longer registered", q.module))
    })?;
    let manifest = module.manifest().clone();
    let entity_id = q.entity;
    let loaded = shared
        .store
        .read(move |r| {
            let root = r.resolve(entity_id)?;
            let e = r
                .entity(root)?
                .ok_or_else(|| spektr_store::StoreError::NotFound(format!("entity {root}")))?;
            let blob = if e.ty.as_str() == "image" {
                match e
                    .key
                    .strip_prefix("sha256:")
                    .and_then(|h| h.parse::<spektr_store::Sha256>().ok())
                {
                    Some(sha) => r.blob(sha)?,
                    None => None,
                }
            } else {
                None
            };
            Ok((e, blob))
        })
        .await;
    let (entity, blob) =
        loaded.map_err(|e| ModuleError::Internal(format!("loading the input: {e}")))?;
    let view = EntityView {
        id: entity.id,
        ty: entity.ty.clone(),
        key: entity.key.clone(),
        value: entity.value.clone(),
        attrs: entity.attrs.clone(),
    };
    let ctx = JobCtx::new(
        q,
        &manifest,
        shared.manifest_hash(&q.module),
        entity.summary(),
        entity.ty.clone(),
        entity.depth,
    );
    shared.work.add(1);
    if shared
        .ingest
        .send(IngestMsg::Started {
            job: q.job,
            ctx: Box::new(ctx),
        })
        .await
        .is_err()
    {
        shared.work.done(1);
        return Err(ModuleError::Internal("the engine has shut down".into()));
    }

    let secrets = Secrets(
        manifest
            .secrets
            .iter()
            .chain(&manifest.optional_secrets)
            .filter_map(|s| shared.config.secrets.get(s).map(|v| (s.clone(), v.clone())))
            .collect(),
    );
    let mut config = manifest.default_config();
    if let (Some(obj), Some(user)) = (
        config.as_object_mut(),
        shared
            .config
            .module_config
            .get(&q.module)
            .and_then(|c| c.as_object()),
    ) {
        for (k, v) in user {
            obj.insert(k.clone(), v.clone());
        }
    }
    let cx = ModuleCx {
        emit: Emitter::new(
            q.job,
            shared.ingest.clone(),
            shared.work.clone(),
            shared.config.limits,
        ),
        config,
        secrets,
        cancel: token.clone(),
    };
    let input = JobInput {
        job: q.job,
        entity: view,
        blob: blob.map(Arc::from),
        depth: entity.depth,
    };
    let timeout = manifest.timeout_duration();
    tokio::select! {
        r = tokio::time::timeout(timeout, module.run(input, cx)) => match r {
            Ok(r) => r,
            Err(_) => Err(ModuleError::code(spektr_protocol::ErrorCode::Timeout, format!("timed out after {}", manifest.timeout))),
        },
        _ = token.cancelled() => Err(ModuleError::Cancelled),
    }
}
