//! The ingestion pipeline (MODULES §5, ARCHITECTURE §3): one path for
//! native and exec modules.
//!
//! Messages are batched (up to 200, or 50 ms) and each batch is applied in
//! one store transaction: validate, normalize and upsert entities, store
//! blobs, upsert edges and attach evidence, write audit rows. Then events
//! are emitted, correlators run on the changed entities, and follow-up
//! automatic jobs are planned. A message that fails validation is rejected
//! and logged; the job continues.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde_json::json;
use spektr_model::{
    Actor, Attr, AttrValue, CorePredicate, EntityFlags, EntityId, EntityType, JobId, KeyHint,
    NormalizeCtx, ObservationId, Predicate, Vocabulary, normalize, strength::source_key_for_url,
};
use spektr_protocol::{AttrValue as PAttr, EntitySpec, Manifest, ModuleMessage};
use spektr_store::{
    EdgeAttrs, EntitySummary, JobState, NewEdge, NewObservation, ObservationKind, ReviewKind,
    Sha256, StoreError, StoreResult, Tx,
};
use tokio::sync::mpsc;

use crate::engine::Shared;
use crate::events::EventKind;
use crate::module::ModuleError;
use crate::plan::{auto_requests, plan};
use crate::scheduler::{QueuedJob, SchedMsg};
use crate::scope::Scope;

/// Retries for `rate_limited` / `upstream` errors (MODULES §3.3).
pub const MAX_RETRIES: u32 = 2;

pub(crate) enum IngestMsg {
    Started {
        job: JobId,
        ctx: Box<JobCtx>,
    },
    Message {
        job: JobId,
        msg: ModuleMessage,
    },
    Finished {
        job: JobId,
        result: Result<(), ModuleError>,
        queued: QueuedJob,
    },
}

#[derive(Debug, Clone)]
struct ObsRef {
    id: ObservationId,
    source_key: String,
    blob: Option<Sha256>,
}

/// Per-job validation state: local observation ids and entity refs.
#[derive(Debug, Clone)]
pub(crate) struct JobCtx {
    job: JobId,
    module: String,
    version: String,
    manifest_hash: String,
    manifest: Arc<Manifest>,
    input: EntityId,
    input_ty: EntityType,
    input_summary: EntitySummary,
    depth: u32,
    obs: HashMap<String, ObsRef>,
    refs: HashMap<String, (EntityId, EntityType)>,
    found: BTreeMap<String, u32>,
    note: Option<String>,
}

impl JobCtx {
    pub fn new(
        q: &QueuedJob,
        manifest: &Manifest,
        manifest_hash: String,
        input_summary: EntitySummary,
        input_ty: EntityType,
        depth: u32,
    ) -> Self {
        Self {
            job: q.job,
            module: manifest.name.clone(),
            version: manifest.version.clone(),
            manifest_hash,
            manifest: Arc::new(manifest.clone()),
            input: input_summary.id,
            input_ty,
            input_summary,
            depth,
            obs: HashMap::new(),
            refs: HashMap::new(),
            found: BTreeMap::new(),
            note: None,
        }
    }

    fn actor(&self) -> Actor {
        Actor::Module {
            name: self.module.clone(),
            version: self.version.clone(),
        }
    }
}

/// What a batch did, for after the transaction.
#[derive(Default)]
struct Applied {
    events: Vec<EventKind>,
    /// New entities: `(id, type, platform)`, for follow-up auto jobs.
    created: Vec<(EntityId, String, Option<String>)>,
    /// Entities whose edges or attributes changed, for the correlators.
    touched: BTreeSet<EntityId>,
    /// Jobs finished for good.
    finished: Vec<JobId>,
    /// Jobs to retry after a delay.
    retries: Vec<(QueuedJob, Duration)>,
}

pub(crate) struct Ingestor {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<IngestMsg>,
    ctxs: Arc<Mutex<HashMap<JobId, JobCtx>>>,
}

impl Ingestor {
    pub fn new(shared: Arc<Shared>, rx: mpsc::Receiver<IngestMsg>) -> Self {
        Self {
            shared,
            rx,
            ctxs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(mut self) {
        let max = self.shared.config.batch_max.max(1);
        let window = self.shared.config.batch_window;
        while let Some(first) = self.rx.recv().await {
            let mut batch = vec![first];
            let deadline = tokio::time::Instant::now() + window;
            while batch.len() < max {
                match self.rx.try_recv() {
                    Ok(m) => batch.push(m),
                    Err(mpsc::error::TryRecvError::Empty) => {
                        tokio::select! {
                            m = self.rx.recv() => match m {
                                Some(m) => batch.push(m),
                                None => break,
                            },
                            _ = tokio::time::sleep_until(deadline) => break,
                        }
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => break,
                }
            }
            let n = batch.len() as i64;
            self.process(batch).await;
            self.shared.work.done(n);
        }
    }

    async fn process(&mut self, batch: Vec<IngestMsg>) {
        let shared = Arc::clone(&self.shared);
        let vocab = shared.vocab();
        let settings = shared.settings();
        let ctxs = Arc::clone(&self.ctxs);
        let scope_rows = shared
            .store
            .read(|r| r.scope_rules())
            .await
            .unwrap_or_default();
        let scope = Scope::from_rows(&scope_rows);
        let jobs_in_batch: BTreeSet<JobId> = batch
            .iter()
            .map(|m| match m {
                IngestMsg::Started { job, .. }
                | IngestMsg::Message { job, .. }
                | IngestMsg::Finished { job, .. } => *job,
            })
            .collect();
        let finishing: Vec<QueuedJob> = batch
            .iter()
            .filter_map(|m| match m {
                IngestMsg::Finished { queued, .. } => Some(queued.clone()),
                _ => None,
            })
            .collect();
        let threshold = settings.hub_threshold;
        let region = settings.region.clone();

        let result = shared
            .store
            .write(move |tx| {
                let mut out = Applied::default();
                let mut ctxs = ctxs.lock().unwrap_or_else(|e| e.into_inner());
                for m in batch {
                    apply(
                        tx, &vocab, &scope, threshold, &region, &mut ctxs, m, &mut out,
                    )?;
                }
                Ok(out)
            })
            .await;

        match result {
            Ok(out) => {
                for e in out.events {
                    shared.emit(e);
                }
                self.after_batch(out.created, out.touched).await;
                for (q, delay) in out.retries {
                    let mut q = q;
                    q.attempt += 1;
                    q.not_before = Some(tokio::time::Instant::now() + delay);
                    // The job keeps its outstanding-work unit across the retry.
                    if shared.sched.send(SchedMsg::Enqueue(q)).is_err() {
                        shared.work.done(1);
                    }
                }
                for _ in out.finished {
                    shared.work.done(1);
                }
            }
            Err(e) => {
                tracing::error!("ingestion batch rolled back: {e}");
                shared.emit(EventKind::IngestFailed {
                    message: format!("store error, batch rolled back: {e}"),
                });
                // The batch's effects are gone; fail its jobs so nothing
                // waits on them.
                let failed: Vec<JobId> = jobs_in_batch.into_iter().collect();
                {
                    let mut c = self.ctxs.lock().unwrap_or_else(|e| e.into_inner());
                    for j in &failed {
                        c.remove(j);
                    }
                }
                let msg = e.to_string();
                let to_fail = failed.clone();
                let _ = shared
                    .store
                    .write(move |tx| {
                        for j in to_fail {
                            if tx
                                .reader()
                                .job(j)?
                                .is_some_and(|row| !row.state.is_terminal())
                            {
                                tx.set_job_state(
                                    &Actor::Engine,
                                    j,
                                    JobState::Failed,
                                    None,
                                    Some(&msg),
                                )?;
                            }
                        }
                        Ok(())
                    })
                    .await;
                for _ in finishing {
                    shared.work.done(1);
                }
            }
        }
    }

    /// Correlators on changed entities, then follow-up automatic jobs.
    async fn after_batch(
        &self,
        created: Vec<(EntityId, String, Option<String>)>,
        mut touched: BTreeSet<EntityId>,
    ) {
        let shared = Arc::clone(&self.shared);
        touched.extend(created.iter().map(|c| c.0));
        if !touched.is_empty() && !shared.correlators.is_empty() {
            let shared2 = Arc::clone(&shared);
            let touched2 = touched.clone();
            let found = shared
                .store
                .write(move |tx| {
                    let mut events = Vec::new();
                    for c in &shared2.correlators {
                        events.extend(c.correlate(tx, &touched2)?);
                    }
                    Ok(events)
                })
                .await;
            match found {
                Ok(events) => {
                    for e in events {
                        shared.emit(e);
                    }
                }
                Err(e) => shared.emit(EventKind::IngestFailed {
                    message: format!("correlation failed: {e}"),
                }),
            }
        }
        let reqs = auto_requests(&shared, &created);
        if let Err(e) = plan(&shared, reqs).await {
            shared.emit(EventKind::IngestFailed {
                message: format!("planning follow-up jobs failed: {e}"),
            });
        }
    }
}

fn reject(out: &mut Applied, ctx: &JobCtx, reason: String) {
    tracing::warn!(module = %ctx.module, job = %ctx.job, "rejected module message: {reason}");
    out.events.push(EventKind::MessageRejected {
        module: ctx.module.clone(),
        job: ctx.job,
        reason,
    });
}

#[allow(clippy::too_many_arguments)]
fn apply(
    tx: &mut Tx<'_>,
    vocab: &Vocabulary,
    scope: &Scope,
    hub_threshold: u32,
    region: &str,
    ctxs: &mut HashMap<JobId, JobCtx>,
    msg: IngestMsg,
    out: &mut Applied,
) -> StoreResult<()> {
    match msg {
        IngestMsg::Started { job, ctx } => {
            tx.set_job_state(&Actor::Engine, job, JobState::Running, None, None)?;
            out.events.push(EventKind::JobStarted {
                job,
                module: ctx.module.clone(),
                entity: ctx.input_summary.clone(),
            });
            ctxs.insert(job, *ctx);
        }
        IngestMsg::Finished {
            job,
            result,
            queued,
        } => {
            let ctx = ctxs.remove(&job);
            let module = queued.module.clone();
            let entity = match &ctx {
                Some(c) => c.input_summary.clone(),
                None => tx
                    .reader()
                    .summary(queued.entity)?
                    .ok_or_else(|| StoreError::NotFound(format!("entity {}", queued.entity)))?,
            };
            let found = ctx.as_ref().map(|c| c.found.clone()).unwrap_or_default();
            let note = ctx.as_ref().and_then(|c| c.note.clone());
            match result {
                Ok(()) => {
                    tx.set_job_state(&Actor::Engine, job, JobState::Done, None, None)?;
                    out.events.push(EventKind::JobFinished {
                        job,
                        module,
                        entity,
                        state: JobState::Done,
                        found,
                        note,
                        error: None,
                        retry_in_secs: None,
                        attempts_left: None,
                    });
                    out.finished.push(job);
                }
                Err(e) if e.retryable() && queued.attempt < MAX_RETRIES => {
                    let delay = e
                        .retry_after()
                        .unwrap_or(Duration::from_secs(2u64.pow(queued.attempt + 1)));
                    let left = MAX_RETRIES - queued.attempt;
                    let reason = format!(
                        "retry {}/{MAX_RETRIES} in {}s",
                        queued.attempt + 1,
                        delay.as_secs()
                    );
                    tx.set_job_state(
                        &Actor::Engine,
                        job,
                        JobState::Queued,
                        Some(&reason),
                        Some(&e.to_string()),
                    )?;
                    out.events.push(EventKind::JobFinished {
                        job,
                        module,
                        entity,
                        state: JobState::Queued,
                        found,
                        note,
                        error: Some(e.to_string()),
                        retry_in_secs: Some(delay.as_secs()),
                        attempts_left: Some(left),
                    });
                    out.retries.push((queued, delay));
                }
                Err(e) => {
                    let err = e.to_string();
                    tx.set_job_state(&Actor::Engine, job, JobState::Failed, None, Some(&err))?;
                    out.events.push(EventKind::JobFinished {
                        job,
                        module,
                        entity,
                        state: JobState::Failed,
                        found,
                        note,
                        error: Some(err),
                        retry_in_secs: None,
                        attempts_left: None,
                    });
                    out.finished.push(job);
                }
            }
        }
        IngestMsg::Message { job, msg } => {
            let Some(ctx) = ctxs.get_mut(&job) else {
                tracing::warn!("message for unknown job {job}");
                return Ok(());
            };
            apply_message(tx, vocab, scope, hub_threshold, region, ctx, msg, out)?;
        }
    }
    Ok(())
}

/// Resolve evidence ids; all must be known observations of this job.
fn evidence(ctx: &JobCtx, ids: &[String]) -> Result<Vec<ObsRef>, String> {
    if ids.is_empty() {
        return Err("must cite at least one observation".into());
    }
    ids.iter()
        .map(|id| {
            ctx.obs
                .get(id)
                .cloned()
                .ok_or_else(|| format!("cites unknown observation `{id}`"))
        })
        .collect()
}

fn resolve_ref(ctx: &JobCtx, r: &str) -> Result<(EntityId, EntityType), String> {
    if r == "$input" {
        return Ok((ctx.input, ctx.input_ty.clone()));
    }
    ctx.refs
        .get(r)
        .cloned()
        .ok_or_else(|| format!("unknown entity ref `{r}`"))
}

fn convert_attr(v: &PAttr) -> Result<AttrValue, String> {
    Ok(match v {
        PAttr::Text(s) => AttrValue::Text(s.clone()),
        PAttr::Int(i) => AttrValue::Int(*i),
        PAttr::Bool(b) => AttrValue::Bool(*b),
        PAttr::Time(s) => AttrValue::Time(s.parse().map_err(|e| format!("bad time `{s}`: {e}"))?),
        PAttr::Date(s) => AttrValue::Date(s.parse().map_err(|e| format!("bad date `{s}`: {e}"))?),
        PAttr::Geo { lat, lon } => {
            if !(lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(lat)
                && (-180.0..=180.0).contains(lon))
            {
                return Err("coordinates out of range".into());
            }
            AttrValue::Geo {
                lat: *lat,
                lon: *lon,
            }
        }
        PAttr::Url(s) => AttrValue::Url(s.clone()),
    })
}

/// Set `out_of_scope` on new entities the case's scope rules exclude, and
/// record them for follow-ups.
fn note_created(
    tx: &mut Tx<'_>,
    scope: &Scope,
    actor: &Actor,
    ids: &[EntityId],
    out: &mut Applied,
    ctx_found: Option<&mut BTreeMap<String, u32>>,
    by: &str,
) -> StoreResult<()> {
    let mut found = ctx_found;
    for id in ids {
        let Some(e) = tx.reader().entity(*id)? else {
            continue;
        };
        let mut summary = e.summary();
        if !scope.is_empty() && !scope.in_scope(&e.ty, &e.key) {
            summary.flags =
                tx.set_flags(actor, e.id, EntityFlags::OUT_OF_SCOPE, EntityFlags::NONE)?;
        }
        if let Some(f) = found.as_deref_mut() {
            *f.entry(e.ty.to_string()).or_default() += 1;
        }
        let platform = (e.ty.as_str() == "account")
            .then(|| e.key.split_once(':').map_or("", |p| p.0).to_string());
        out.created.push((e.id, e.ty.to_string(), platform));
        out.events.push(EventKind::EntityAdded {
            entity: summary,
            by: by.to_string(),
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_message(
    tx: &mut Tx<'_>,
    vocab: &Vocabulary,
    scope: &Scope,
    hub_threshold: u32,
    region: &str,
    ctx: &mut JobCtx,
    msg: ModuleMessage,
    out: &mut Applied,
) -> StoreResult<()> {
    let actor = ctx.actor();
    match msg {
        ModuleMessage::Observation {
            id,
            source_url,
            content_type,
            raw,
            raw_b64,
            summary,
            ..
        } => {
            if ctx.obs.contains_key(&id) {
                reject(out, ctx, format!("observation id `{id}` used twice"));
                return Ok(());
            }
            let bytes = match (raw, raw_b64) {
                (Some(_), Some(_)) => {
                    reject(
                        out,
                        ctx,
                        format!("observation `{id}` has both raw and raw_b64"),
                    );
                    return Ok(());
                }
                (Some(text), None) => Some(text.into_bytes()),
                (None, Some(b64)) => {
                    match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
                        Ok(b) => Some(b),
                        Err(e) => {
                            reject(
                                out,
                                ctx,
                                format!("observation `{id}`: raw_b64 is not base64: {e}"),
                            );
                            return Ok(());
                        }
                    }
                }
                (None, None) => None,
            };
            let blob = match bytes {
                Some(b) if !b.is_empty() => match tx.put_blob(&actor, &b) {
                    Ok(sha) => Some(sha),
                    Err(StoreError::BlobTooLarge { size, max }) => {
                        reject(
                            out,
                            ctx,
                            format!("observation `{id}`: {size} bytes exceeds {max}"),
                        );
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                },
                _ => None,
            };
            let source_key = source_url
                .as_deref()
                .and_then(source_key_for_url)
                .unwrap_or_else(|| ctx.module.clone());
            let oid = tx.add_observation(
                &actor,
                NewObservation {
                    job: Some(ctx.job),
                    module: ctx.module.clone(),
                    module_version: ctx.version.clone(),
                    manifest_hash: ctx.manifest_hash.clone(),
                    input_entity: Some(ctx.input),
                    source_url,
                    content_type,
                    blob,
                    summary: summary.unwrap_or_else(|| json!({})),
                    kind: ObservationKind::Module,
                },
            )?;
            ctx.obs.insert(
                id,
                ObsRef {
                    id: oid,
                    source_key,
                    blob,
                },
            );
        }
        ModuleMessage::Entity {
            reference,
            entity,
            evidence: ev,
            ..
        } => {
            if reference == "$input" || ctx.refs.contains_key(&reference) {
                reject(
                    out,
                    ctx,
                    format!("entity ref `{reference}` is reserved or already used"),
                );
                return Ok(());
            }
            let ev = match evidence(ctx, &ev) {
                Ok(e) => e,
                Err(r) => {
                    reject(out, ctx, format!("entity `{reference}` {r}"));
                    return Ok(());
                }
            };
            let EntitySpec {
                ty,
                value,
                blob,
                dedup_id,
                key,
            } = entity;
            let ty = match vocab.entity_type(&ty) {
                Ok(t) => t,
                Err(e) => {
                    reject(out, ctx, format!("entity `{reference}`: {e}"));
                    return Ok(());
                }
            };
            if !ctx.manifest.produces_type(ty.as_str()) && ty != ctx.input_ty {
                reject(
                    out,
                    ctx,
                    format!(
                        "entity `{reference}`: {} doesn't declare that it produces `{ty}`",
                        ctx.module
                    ),
                );
                return Ok(());
            }
            let fresh = tx.new_uuid();
            let blob_hex;
            let hint = match ty.core() {
                Some(spektr_model::CoreType::Image) => {
                    let Some(sha) = blob
                        .as_deref()
                        .and_then(|b| ctx.obs.get(b))
                        .and_then(|o| o.blob)
                    else {
                        reject(
                            out,
                            ctx,
                            format!(
                                "image `{reference}` must name the observation holding its bytes (`blob`)"
                            ),
                        );
                        return Ok(());
                    };
                    blob_hex = sha.hex();
                    KeyHint::Sha256(&blob_hex)
                }
                Some(spektr_model::CoreType::Person | spektr_model::CoreType::Organization) => {
                    match dedup_id.as_deref() {
                        Some(id) => KeyHint::Dedup {
                            module: &ctx.module,
                            id,
                        },
                        None => KeyHint::NewUuid(fresh),
                    }
                }
                Some(spektr_model::CoreType::Certificate) => match key.as_deref() {
                    Some(k) => KeyHint::Key(k),
                    None => KeyHint::None,
                },
                _ => KeyHint::None,
            };
            let n = match normalize(
                &ty,
                &value,
                hint,
                NormalizeCtx {
                    vocab,
                    default_region: Some(region),
                },
            ) {
                Ok(n) => n,
                Err(e) => {
                    reject(out, ctx, format!("entity `{reference}`: {e}"));
                    return Ok(());
                }
            };
            let first = &ev[0];
            let ing = tx.ingest_entity(
                &actor,
                &n,
                ctx.depth.saturating_add(1),
                Some((first.id, &first.source_key)),
            )?;
            // implied edges get every cited observation as evidence
            for (edge, target) in &ing.derived {
                for o in &ev[1..] {
                    tx.add_evidence(&actor, *edge, o.id, &o.source_key)?;
                }
                out.touched.insert(*target);
            }
            ctx.refs.insert(reference, (ing.id, ing.ty.clone()));
            let module = ctx.module.clone();
            note_created(
                tx,
                scope,
                &actor,
                &ing.new_entities,
                out,
                Some(&mut ctx.found),
                &module,
            )?;
            if !ing.derived.is_empty() {
                out.touched.insert(ing.id);
            }
        }
        ModuleMessage::Attr {
            target,
            name,
            value,
            event,
            evidence: ev,
            ..
        } => {
            let (id, _) = match resolve_ref(ctx, &target) {
                Ok(t) => t,
                Err(r) => {
                    reject(out, ctx, format!("attr `{name}`: {r}"));
                    return Ok(());
                }
            };
            let ev = match evidence(ctx, &ev) {
                Ok(e) => e,
                Err(r) => {
                    reject(out, ctx, format!("attr `{name}` {r}"));
                    return Ok(());
                }
            };
            if !spektr_model::attrs::valid_name(&name) {
                reject(
                    out,
                    ctx,
                    format!("attr name `{name}` must match [a-z0-9_]{{1,64}}"),
                );
                return Ok(());
            }
            let value = match convert_attr(&value) {
                Ok(v) => v,
                Err(r) => {
                    reject(out, ctx, format!("attr `{name}`: {r}"));
                    return Ok(());
                }
            };
            if tx.add_attr(
                &actor,
                id,
                &name,
                Attr {
                    value,
                    observation: ev[0].id,
                    event,
                },
            )? {
                out.touched.insert(id);
            }
        }
        ModuleMessage::Edge {
            src,
            predicate,
            dst,
            attrs,
            evidence: ev,
            ..
        } => {
            let resolved =
                resolve_ref(ctx, &src).and_then(|s| resolve_ref(ctx, &dst).map(|d| (s, d)));
            let ((s, sty), (d, dty)) = match resolved {
                Ok(x) => x,
                Err(r) => {
                    reject(out, ctx, format!("edge `{predicate}`: {r}"));
                    return Ok(());
                }
            };
            let ev = match evidence(ctx, &ev) {
                Ok(e) => e,
                Err(r) => {
                    reject(out, ctx, format!("edge `{predicate}` {r}"));
                    return Ok(());
                }
            };
            let pred = match vocab.predicate(&predicate) {
                Ok(p) => p,
                Err(e) => {
                    reject(out, ctx, format!("edge: {e}"));
                    return Ok(());
                }
            };
            if !ctx.manifest.produces_predicate(&predicate) {
                reject(
                    out,
                    ctx,
                    format!(
                        "edge: {} doesn't declare that it produces `{predicate}`",
                        ctx.module
                    ),
                );
                return Ok(());
            }
            if let Predicate::Core(c) = &pred
                && (c.analyst_only() || *c == CorePredicate::SharesArtifact)
            {
                reject(
                    out,
                    ctx,
                    format!("edge: modules can't assert `{predicate}`"),
                );
                return Ok(());
            }
            if let Err(e) = vocab.validate_edge(&sty, &pred, &dty) {
                reject(out, ctx, format!("edge: {e}"));
                return Ok(());
            }
            if s == d {
                reject(
                    out,
                    ctx,
                    format!("edge `{predicate}` connects an entity to itself"),
                );
                return Ok(());
            }
            let attrs: EdgeAttrs = attrs;
            let (edge, created) = tx.upsert_edge(
                &actor,
                NewEdge {
                    src: s,
                    predicate: pred.clone(),
                    dst: d,
                    discriminator: String::new(),
                    attrs,
                    origin: format!("module:{}", ctx.module),
                },
            )?;
            for o in &ev {
                tx.add_evidence(&actor, edge, o.id, &o.source_key)?;
            }
            for endpoint in [s, d] {
                tx.refresh_hub(&Actor::Engine, endpoint, hub_threshold)?;
                out.touched.insert(endpoint);
            }
            if created {
                let r = tx.reader();
                if let (Some(a), Some(b)) = (r.summary(r.resolve(s)?)?, r.summary(r.resolve(d)?)?) {
                    out.events.push(EventKind::EdgeAdded {
                        edge,
                        src: a,
                        predicate: pred,
                        dst: b,
                    });
                }
            } else {
                out.events.push(EventKind::EdgeChanged { edge });
            }
        }
        ModuleMessage::Review {
            kind,
            payload,
            evidence: ev,
            ..
        } => {
            let Ok(kind) = kind.parse::<ReviewKind>() else {
                reject(out, ctx, format!("unknown review kind `{kind}`"));
                return Ok(());
            };
            let ev = match evidence(ctx, &ev) {
                Ok(e) => e,
                Err(r) => {
                    reject(out, ctx, format!("review {r}"));
                    return Ok(());
                }
            };
            let payload = json!({
                "module": ctx.module,
                "job": ctx.job,
                "input": ctx.input,
                "evidence": ev.iter().map(|o| o.id).collect::<Vec<_>>(),
                "detail": payload,
            });
            let id = tx.add_review_item(&actor, kind, &payload)?;
            out.events.push(EventKind::ReviewAdded {
                id,
                kind,
                module: ctx.module.clone(),
            });
        }
        ModuleMessage::Progress { pct, note, .. } => {
            if note.is_some() {
                ctx.note = note.clone();
            }
            out.events.push(EventKind::JobProgress {
                job: ctx.job,
                module: ctx.module.clone(),
                pct: pct.min(100),
                note,
            });
        }
        ModuleMessage::Log { level, msg, .. } => {
            tracing::info!(module = %ctx.module, "{msg}");
            out.events.push(EventKind::ModuleLog {
                module: ctx.module.clone(),
                level,
                msg,
            });
        }
        // Lifecycle messages are consumed by the exec runtime; a native
        // module's return value is its outcome.
        ModuleMessage::Ready | ModuleMessage::Done { .. } | ModuleMessage::Error { .. } => {}
    }
    Ok(())
}

/// Insert an analyst seed (`spektr run --seed`, `:seed`): an analyst
/// observation is its evidence (DATA_MODEL §4).
pub(crate) async fn seed(
    shared: &Arc<Shared>,
    ty: EntityType,
    value: String,
) -> crate::error::EngineResult<(EntitySummary, bool, Vec<(EntityId, String, Option<String>)>)> {
    let vocab = shared.vocab();
    let region = shared.settings().region;
    let scope_rows = shared.store.read(|r| r.scope_rules()).await?;
    let scope = Scope::from_rows(&scope_rows);
    let (summary, created, applied) = shared
        .store
        .write(move |tx| {
            let fresh = tx.new_uuid();
            let hint = match ty.core() {
                Some(spektr_model::CoreType::Person | spektr_model::CoreType::Organization) => {
                    KeyHint::NewUuid(fresh)
                }
                _ => KeyHint::None,
            };
            let n = normalize(
                &ty,
                &value,
                hint,
                NormalizeCtx {
                    vocab: &vocab,
                    default_region: Some(&region),
                },
            )?;
            let obs = tx.add_observation(
                &Actor::Analyst,
                NewObservation {
                    job: None,
                    module: "analyst".into(),
                    module_version: env!("CARGO_PKG_VERSION").into(),
                    manifest_hash: "builtin".into(),
                    input_entity: None,
                    source_url: None,
                    content_type: None,
                    blob: None,
                    summary: json!({ "action": "seed", "type": n.ty, "value": n.value }),
                    kind: ObservationKind::Analyst,
                },
            )?;
            let ing = tx.ingest_entity(&Actor::Analyst, &n, 0, Some((obs, "analyst")))?;
            tx.set_flags(
                &Actor::Analyst,
                ing.id,
                EntityFlags::SEED,
                EntityFlags::NONE,
            )?;
            let mut out = Applied::default();
            note_created(
                tx,
                &scope,
                &Actor::Analyst,
                &ing.new_entities,
                &mut out,
                None,
                "analyst",
            )?;
            // re-seeding an existing entity still expands it
            if !ing.created {
                let e = tx
                    .reader()
                    .entity(ing.id)?
                    .ok_or_else(|| StoreError::NotFound("seed".into()))?;
                let platform = (e.ty.as_str() == "account")
                    .then(|| e.key.split_once(':').map_or("", |p| p.0).to_string());
                out.created.insert(0, (e.id, e.ty.to_string(), platform));
            }
            let root = tx.reader().resolve(ing.id)?;
            let summary = tx
                .reader()
                .summary(root)?
                .ok_or_else(|| StoreError::NotFound("seed".into()))?;
            Ok((summary, ing.created, out))
        })
        .await
        .map_err(|e| match e {
            StoreError::Normalize(n) => crate::error::EngineError::Normalize(n),
            other => crate::error::EngineError::Store(other),
        })?;
    for e in applied.events {
        shared.emit(e);
    }
    Ok((summary, created, applied.created))
}
