//! Write transactions. Every mutation writes exactly one audit row in the
//! same transaction (DATA_MODEL §9); a no-op (a fact already known) writes
//! none.

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Serialize;
use serde_json::{Value, json};
use spektr_model::{
    Actor, Attr, Attrs, ChecklistItemId, CorePredicate, EdgeId, EntityFlags, EntityId, EntityType,
    JobId, MergeId, Normalized, NoteId, ObservationId, PlaybookRunId, Predicate, ReviewItemId,
    ScopeRuleId, attrs,
};
use uuid::Uuid;

use crate::audit;
use crate::blob::{Sha256, compress, sha256};
use crate::db::OpenOptions;
use crate::error::{StoreError, StoreResult, invalid};
use crate::reads::Reader;
use crate::records::{
    FindingStatus, ItemState, JobState, ReviewKind, ReviewState, RunState, ScopeKind, ScopeMatcher,
    StepKind, StepState, Trigger,
};
use crate::rows::{EdgeAttrs, EdgeState, ObservationKind};
use crate::sql::{idb, ts};

/// One write transaction.
pub struct Tx<'a> {
    tx: Transaction<'a>,
    now: Timestamp,
    opts: &'a OpenOptions,
    tip: (i64, [u8; 32]),
    wrote: bool,
}

/// A new observation (raw evidence) to record.
#[derive(Debug, Clone)]
pub struct NewObservation {
    pub job: Option<JobId>,
    pub module: String,
    pub module_version: String,
    pub manifest_hash: String,
    pub input_entity: Option<EntityId>,
    pub source_url: Option<String>,
    pub content_type: Option<String>,
    pub blob: Option<Sha256>,
    pub summary: Value,
    pub kind: ObservationKind,
}

/// A new edge, or more support for an existing one.
#[derive(Debug, Clone)]
pub struct NewEdge {
    pub src: EntityId,
    pub predicate: Predicate,
    pub dst: EntityId,
    /// Empty except for `shares_artifact` (`artifact:value`).
    pub discriminator: String,
    pub attrs: EdgeAttrs,
    /// `module:<name>`, `correlator:<rule>`, `analyst`, `model:normalize`, …
    pub origin: String,
}

/// Changes to a finding; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingUpdate {
    pub title: Option<String>,
    pub category: Option<String>,
    pub claim: Option<String>,
    pub relevance: Option<String>,
    pub status: Option<FindingStatus>,
}

/// The result of ingesting a normalized entity (and what it implies).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestedEntity {
    pub id: EntityId,
    pub ty: EntityType,
    pub created: bool,
    /// Entities created or touched through derived edges, with those edges.
    pub derived: Vec<(EdgeId, EntityId)>,
}

impl<'a> Tx<'a> {
    pub(crate) fn begin(
        tx: Transaction<'a>,
        now: Timestamp,
        opts: &'a OpenOptions,
    ) -> StoreResult<Self> {
        let tip = audit::load_tip(&tx)?;
        Ok(Self {
            tx,
            now,
            opts,
            tip,
            wrote: false,
        })
    }

    pub(crate) fn commit(self) -> StoreResult<()> {
        if self.wrote {
            audit::write_tip(&self.tx, self.tip)?;
        }
        self.tx.commit()?;
        Ok(())
    }

    fn conn(&self) -> &Connection {
        &self.tx
    }

    /// The transaction's clock reading; every row it writes uses it.
    pub fn now(&self) -> Timestamp {
        self.now
    }

    pub fn new_uuid(&self) -> Uuid {
        self.opts.ids.next(self.now)
    }

    /// Reads that see this transaction's writes.
    pub fn reader(&self) -> Reader<'_> {
        Reader::new(&self.tx, self.opts)
    }

    /// Append one audit row.
    pub fn audit<T: Serialize>(
        &mut self,
        actor: &Actor,
        action: &str,
        payload: &T,
    ) -> StoreResult<i64> {
        let payload = audit::canonical_json(payload)?;
        let seq = audit::append(
            &self.tx,
            &mut self.tip,
            &ts(self.now),
            &actor.to_string(),
            action,
            &payload,
        )?;
        self.wrote = true;
        Ok(seq)
    }

    // ------------------------------------------------------------ meta

    pub fn set_meta(&mut self, actor: &Actor, key: &str, value: &Value) -> StoreResult<()> {
        if key == "audit_tip" {
            return Err(invalid("audit_tip is maintained by the store"));
        }
        self.put_meta_unaudited(key, value)?;
        self.audit(actor, "meta.set", &json!({ "key": key, "value": value }))?;
        Ok(())
    }

    pub(crate) fn put_meta_unaudited(&self, key: &str, value: &Value) -> StoreResult<()> {
        self.conn()
            .prepare_cached("INSERT INTO case_meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")?
            .execute(params![key, serde_json::to_string(value)?])?;
        Ok(())
    }

    // ------------------------------------------------------------ blobs & observations

    /// Store bytes (deduplicated by SHA-256 of the uncompressed content).
    pub fn put_blob(&mut self, actor: &Actor, bytes: &[u8]) -> StoreResult<Sha256> {
        if bytes.len() > self.opts.max_blob {
            return Err(StoreError::BlobTooLarge {
                size: bytes.len(),
                max: self.opts.max_blob,
            });
        }
        let sha = sha256(bytes);
        let exists: bool = self
            .conn()
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM blob WHERE sha256 = ?1)")?
            .query_row([&sha.0[..]], |r| r.get(0))?;
        if !exists {
            let data = compress(bytes)?;
            self.conn()
                .prepare_cached("INSERT INTO blob (sha256, size, compression, data) VALUES (?1, ?2, 'zstd', ?3)")?
                .execute(params![&sha.0[..], bytes.len() as i64, data])?;
            self.audit(
                actor,
                "blob.add",
                &json!({ "sha256": sha.hex(), "size": bytes.len() }),
            )?;
        }
        Ok(sha)
    }

    pub fn add_observation(
        &mut self,
        actor: &Actor,
        o: NewObservation,
    ) -> StoreResult<ObservationId> {
        let id = ObservationId(self.new_uuid());
        self.conn()
            .prepare_cached(
                "INSERT INTO observation (id, job_id, module, module_version, manifest_hash, input_entity, source_url, content_type, blob_sha256, summary, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?
            .execute(params![
                idb(id),
                o.job.map(idb),
                o.module,
                o.module_version,
                o.manifest_hash,
                o.input_entity.map(idb),
                o.source_url,
                o.content_type,
                o.blob.map(|s| s.0.to_vec()),
                serde_json::to_string(&o.summary)?,
                o.kind.as_str(),
                ts(self.now),
            ])?;
        self.audit(
            actor,
            "observation.add",
            &json!({
                "id": id, "kind": o.kind, "module": o.module, "module_version": o.module_version,
                "job": o.job, "input_entity": o.input_entity, "source_url": o.source_url,
                "blob": o.blob,
            }),
        )?;
        Ok(id)
    }

    // ------------------------------------------------------------ entities

    /// Insert an entity, or find the existing one with the same `(type, key)`.
    /// An existing entity keeps its value and takes the lower depth. The
    /// returned id is the row's own id, even if it has been merged; reads
    /// resolve it.
    pub fn upsert_entity(
        &mut self,
        actor: &Actor,
        ty: &EntityType,
        key: &str,
        value: &str,
        depth: u32,
    ) -> StoreResult<(EntityId, bool)> {
        let existing: Option<(Vec<u8>, u32)> = self
            .conn()
            .prepare_cached("SELECT id, depth FROM entity WHERE type = ?1 AND key = ?2")?
            .query_row(params![ty.as_str(), key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if let Some((raw, old_depth)) = existing {
            let id = EntityId::from_slice(&raw).ok_or_else(|| invalid("bad entity id"))?;
            if depth < old_depth {
                self.conn()
                    .prepare_cached("UPDATE entity SET depth = ?2 WHERE id = ?1")?
                    .execute(params![idb(id), depth])?;
                self.audit(actor, "entity.depth", &json!({ "id": id, "depth": depth }))?;
            }
            return Ok((id, false));
        }
        let id = EntityId(self.new_uuid());
        let flags = if self.opts.hubs.is_hub(ty, key, value) {
            EntityFlags::HUB
        } else {
            EntityFlags::NONE
        };
        self.conn()
            .prepare_cached(
                "INSERT INTO entity (id, type, key, value, attrs, flags, depth, created_at, created_by) VALUES (?1, ?2, ?3, ?4, '{}', ?5, ?6, ?7, ?8)",
            )?
            .execute(params![idb(id), ty.as_str(), key, value, flags.0, depth, ts(self.now), actor.to_string()])?;
        self.audit(
            actor,
            "entity.add",
            &json!({ "id": id, "type": ty, "key": key, "value": value, "depth": depth, "flags": flags.0 }),
        )?;
        Ok((id, true))
    }

    /// Upsert a normalized entity and everything its normalization implies
    /// (`subdomain_of`, `at_domain`, `on_host`, an account's `links_to`).
    /// Implied entities sit one level deeper. Implied edges carry `evidence`,
    /// which is required whenever there are any.
    pub fn ingest_entity(
        &mut self,
        actor: &Actor,
        n: &Normalized,
        depth: u32,
        evidence: Option<(ObservationId, &str)>,
    ) -> StoreResult<IngestedEntity> {
        let (id, created) = self.upsert_entity(actor, &n.ty, &n.key, &n.value, depth)?;
        let mut derived = Vec::new();
        for d in &n.derived {
            let (obs, source_key) = evidence.ok_or_else(|| {
                invalid(format!(
                    "`{}` implies a `{}` edge, which needs evidence",
                    n.key, d.predicate
                ))
            })?;
            let target = self.ingest_entity(actor, &d.target, depth.saturating_add(1), evidence)?;
            let (edge, _) = self.upsert_edge(
                actor,
                NewEdge {
                    src: id,
                    predicate: d.predicate.into(),
                    dst: target.id,
                    discriminator: String::new(),
                    attrs: EdgeAttrs::new(),
                    origin: "model:normalize".into(),
                },
            )?;
            self.add_evidence(actor, edge, obs, source_key)?;
            derived.push((edge, target.id));
            derived.extend(target.derived);
        }
        Ok(IngestedEntity {
            id,
            ty: n.ty.clone(),
            created,
            derived,
        })
    }

    fn load_attrs(&self, entity: EntityId) -> StoreResult<Attrs> {
        let raw: String = self
            .conn()
            .prepare_cached("SELECT attrs FROM entity WHERE id = ?1")?
            .query_row([idb(entity)], |r| r.get(0))
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("entity {entity}")))?;
        Ok(serde_json::from_str(&raw)?)
    }

    /// Add one sourced attribute value. Returns false if that value from that
    /// observation was already there.
    pub fn add_attr(
        &mut self,
        actor: &Actor,
        entity: EntityId,
        name: &str,
        attr: Attr,
    ) -> StoreResult<bool> {
        if !attrs::valid_name(name) {
            return Err(invalid(format!("bad attribute name `{name}`")));
        }
        let mut current = self.load_attrs(entity)?;
        let payload = json!({
            "entity": entity, "name": name, "value": attr.value,
            "observation": attr.observation, "event": attr.event,
        });
        if !attrs::insert(&mut current, name, attr) {
            return Ok(false);
        }
        self.conn()
            .prepare_cached("UPDATE entity SET attrs = ?2 WHERE id = ?1")?
            .execute(params![idb(entity), serde_json::to_string(&current)?])?;
        self.audit(actor, "attr.add", &payload)?;
        Ok(true)
    }

    pub fn set_flags(
        &mut self,
        actor: &Actor,
        entity: EntityId,
        add: EntityFlags,
        remove: EntityFlags,
    ) -> StoreResult<EntityFlags> {
        let old: u32 = self
            .conn()
            .prepare_cached("SELECT flags FROM entity WHERE id = ?1")?
            .query_row([idb(entity)], |r| r.get(0))
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("entity {entity}")))?;
        let new = EntityFlags(old).without(remove).with(add);
        if new.0 != old {
            self.conn()
                .prepare_cached("UPDATE entity SET flags = ?2 WHERE id = ?1")?
                .execute(params![idb(entity), new.0])?;
            self.audit(
                actor,
                "entity.flags",
                &json!({ "id": entity, "flags": new.0, "previous": old }),
            )?;
        }
        Ok(new)
    }

    /// Mark the entity a hub if its degree exceeds `threshold`. Returns
    /// whether it is now a hub.
    pub fn refresh_hub(
        &mut self,
        actor: &Actor,
        entity: EntityId,
        threshold: u32,
    ) -> StoreResult<bool> {
        let root = self.reader().resolve(entity)?;
        let degree = self.reader().degree(root)?;
        if degree.total > u64::from(threshold) {
            self.set_flags(actor, root, EntityFlags::HUB, EntityFlags::NONE)?;
            return Ok(true);
        }
        let flags = self
            .reader()
            .entity(root)?
            .map(|e| e.flags)
            .unwrap_or_default();
        Ok(flags.contains(EntityFlags::HUB))
    }

    // ------------------------------------------------------------ edges

    /// Insert an edge, or find the existing one with the same identity
    /// `(src, predicate, dst, discriminator)`. Symmetric predicates are
    /// stored with `src < dst`. New attribute keys are merged into an
    /// existing edge; an analyst's rejection stands.
    pub fn upsert_edge(&mut self, actor: &Actor, mut e: NewEdge) -> StoreResult<(EdgeId, bool)> {
        if e.src == e.dst {
            return Err(invalid("an edge can't connect an entity to itself"));
        }
        if e.predicate == Predicate::Core(CorePredicate::SharesArtifact)
            && e.discriminator.is_empty()
        {
            return Err(invalid(
                "shares_artifact needs an artifact:value discriminator",
            ));
        }
        if e.predicate.is_symmetric() && e.src.as_bytes() > e.dst.as_bytes() {
            std::mem::swap(&mut e.src, &mut e.dst);
        }
        let existing: Option<(Vec<u8>, String)> = self
            .conn()
            .prepare_cached("SELECT id, attrs FROM edge WHERE src = ?1 AND predicate = ?2 AND dst = ?3 AND discriminator = ?4")?
            .query_row(params![idb(e.src), e.predicate.as_str(), idb(e.dst), e.discriminator], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        if let Some((raw, attrs_raw)) = existing {
            let id = EdgeId::from_slice(&raw).ok_or_else(|| invalid("bad edge id"))?;
            let mut current: EdgeAttrs = serde_json::from_str(&attrs_raw)?;
            let mut added = EdgeAttrs::new();
            for (k, v) in e.attrs {
                if !current.contains_key(&k) {
                    current.insert(k.clone(), v.clone());
                    added.insert(k, v);
                }
            }
            if !added.is_empty() {
                self.conn()
                    .prepare_cached("UPDATE edge SET attrs = ?2 WHERE id = ?1")?
                    .execute(params![idb(id), serde_json::to_string(&current)?])?;
                self.audit(actor, "edge.attrs", &json!({ "id": id, "added": added }))?;
            }
            return Ok((id, false));
        }
        let id = EdgeId(self.new_uuid());
        self.conn()
            .prepare_cached(
                "INSERT INTO edge (id, src, dst, predicate, discriminator, attrs, state, origin, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8)",
            )?
            .execute(params![
                idb(id),
                idb(e.src),
                idb(e.dst),
                e.predicate.as_str(),
                e.discriminator,
                serde_json::to_string(&e.attrs)?,
                e.origin,
                ts(self.now),
            ])?;
        self.audit(
            actor,
            "edge.add",
            &json!({
                "id": id, "src": e.src, "predicate": e.predicate, "dst": e.dst,
                "discriminator": e.discriminator, "attrs": e.attrs, "origin": e.origin,
            }),
        )?;
        Ok((id, true))
    }

    /// Attach evidence to an edge. Returns false if already attached.
    pub fn add_evidence(
        &mut self,
        actor: &Actor,
        edge: EdgeId,
        observation: ObservationId,
        source_key: &str,
    ) -> StoreResult<bool> {
        let n = self
            .conn()
            .prepare_cached("INSERT OR IGNORE INTO edge_evidence (edge_id, observation_id, source_key) VALUES (?1, ?2, ?3)")?
            .execute(params![idb(edge), idb(observation), source_key])?;
        if n == 0 {
            return Ok(false);
        }
        self.audit(
            actor,
            "evidence.add",
            &json!({ "edge": edge, "observation": observation, "source_key": source_key }),
        )?;
        Ok(true)
    }

    fn edge_update(
        &mut self,
        actor: &Actor,
        edge: EdgeId,
        action: &str,
        sql: &str,
    ) -> StoreResult<()> {
        let n = self
            .conn()
            .prepare_cached(sql)?
            .execute(params![idb(edge), ts(self.now)])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("edge {edge}")));
        }
        self.audit(actor, action, &json!({ "id": edge }))?;
        Ok(())
    }

    /// `v`: strength → confirmed.
    pub fn confirm_edge(&mut self, actor: &Actor, edge: EdgeId) -> StoreResult<()> {
        self.edge_update(
            actor,
            edge,
            "edge.confirm",
            "UPDATE edge SET confirmed_at = ?2, state = 'active' WHERE id = ?1",
        )
    }

    /// `V`.
    pub fn unconfirm_edge(&mut self, actor: &Actor, edge: EdgeId) -> StoreResult<()> {
        self.edge_update(
            actor,
            edge,
            "edge.unconfirm",
            "UPDATE edge SET confirmed_at = NULL WHERE id = ?1 AND ?2 IS NOT NULL",
        )
    }

    /// `x`: hidden by default, kept for audit, excluded from strength and
    /// correlation.
    pub fn reject_edge(&mut self, actor: &Actor, edge: EdgeId) -> StoreResult<()> {
        self.edge_update(actor, edge, "edge.reject", "UPDATE edge SET state = 'rejected', confirmed_at = NULL WHERE id = ?1 AND ?2 IS NOT NULL")
    }

    pub fn restore_edge(&mut self, actor: &Actor, edge: EdgeId) -> StoreResult<()> {
        self.edge_update(
            actor,
            edge,
            "edge.restore",
            "UPDATE edge SET state = 'active' WHERE id = ?1 AND ?2 IS NOT NULL",
        )
    }

    // ------------------------------------------------------------ merge

    /// Merge `merged` into `survivor` (DATA_MODEL §6). Both resolve to their
    /// roots first; they must be the same type. Entities already pointing at
    /// the merged root are re-pointed to the survivor in this transaction and
    /// recorded, so [`Tx::unmerge`] is exact. Edges are never rewritten.
    pub fn merge(
        &mut self,
        actor: &Actor,
        survivor: EntityId,
        merged: EntityId,
    ) -> StoreResult<MergeId> {
        let r = self.reader();
        let s = r.resolve(survivor)?;
        let m = r.resolve(merged)?;
        if s == m {
            return Err(invalid("those entities are already the same"));
        }
        let (se, me) = (
            r.entity(s)?
                .ok_or_else(|| StoreError::NotFound(format!("entity {s}")))?,
            r.entity(m)?
                .ok_or_else(|| StoreError::NotFound(format!("entity {m}")))?,
        );
        if se.ty != me.ty {
            return Err(invalid(format!("can't merge a {} into a {}", me.ty, se.ty)));
        }
        let pointing: Vec<EntityId> = {
            let mut stmt = self
                .conn()
                .prepare_cached("SELECT id FROM entity WHERE merged_into = ?1 ORDER BY id")?;
            let rows = stmt.query_map([idb(m)], |r| crate::sql::uuid_col(r, 0))?;
            rows.map(|u| u.map(EntityId)).collect::<Result<_, _>>()?
        };
        let repointed: Vec<(EntityId, EntityId)> = pointing.iter().map(|e| (*e, m)).collect();
        for e in &pointing {
            self.conn()
                .prepare_cached("UPDATE entity SET merged_into = ?2 WHERE id = ?1")?
                .execute(params![idb(*e), idb(s)])?;
        }
        self.conn()
            .prepare_cached("UPDATE entity SET merged_into = ?2 WHERE id = ?1")?
            .execute(params![idb(m), idb(s)])?;
        let id = MergeId(self.new_uuid());
        self.conn()
            .prepare_cached("INSERT INTO merge (id, survivor, merged, repointed, created_at) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![idb(id), idb(s), idb(m), serde_json::to_string(&repointed)?, ts(self.now)])?;
        self.audit(
            actor,
            "entity.merge",
            &json!({ "id": id, "survivor": s, "merged": m, "repointed": repointed }),
        )?;
        Ok(id)
    }

    /// Undo a merge exactly. Merges must be undone newest-first where they
    /// touch the same entities.
    pub fn unmerge(&mut self, actor: &Actor, merge: MergeId) -> StoreResult<()> {
        let row = self
            .reader()
            .merge_row(merge)?
            .ok_or_else(|| StoreError::NotFound(format!("merge {merge}")))?;
        if row.undone_at.is_some() {
            return Err(invalid("that merge was already undone"));
        }
        let later: i64 = self.conn().query_row(
            "SELECT count(*) FROM merge WHERE undone_at IS NULL
               AND rowid > (SELECT rowid FROM merge WHERE id = ?1)
               AND merged IN (?2, ?3)",
            params![idb(merge), idb(row.survivor), idb(row.merged)],
            |r| r.get(0),
        )?;
        if later > 0 {
            return Err(invalid(
                "a later merge involves these entities; undo it first",
            ));
        }
        let current: Option<Vec<u8>> = self.conn().query_row(
            "SELECT merged_into FROM entity WHERE id = ?1",
            [idb(row.merged)],
            |r| r.get(0),
        )?;
        if current.as_deref() != Some(&idb(row.survivor)[..]) {
            return Err(invalid(
                "the merged entity no longer points at the survivor",
            ));
        }
        self.conn()
            .prepare_cached("UPDATE entity SET merged_into = NULL WHERE id = ?1")?
            .execute([idb(row.merged)])?;
        for (e, prev) in &row.repointed {
            self.conn()
                .prepare_cached("UPDATE entity SET merged_into = ?2 WHERE id = ?1")?
                .execute(params![idb(*e), idb(*prev)])?;
        }
        self.conn()
            .prepare_cached("UPDATE merge SET undone_at = ?2 WHERE id = ?1")?
            .execute(params![idb(merge), ts(self.now)])?;
        self.audit(actor, "entity.unmerge", &json!({ "id": merge }))?;
        Ok(())
    }

    // ------------------------------------------------------------ jobs

    #[allow(clippy::too_many_arguments)]
    pub fn add_job(
        &mut self,
        actor: &Actor,
        module: &str,
        input_entity: EntityId,
        trigger: Trigger,
        run_id: Option<PlaybookRunId>,
        depth: u32,
        state: JobState,
        reason: Option<&str>,
    ) -> StoreResult<JobId> {
        let id = JobId(self.new_uuid());
        self.conn()
            .prepare_cached(
                "INSERT INTO job (id, module, input_entity, trigger, run_id, depth, state, reason, queued_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?
            .execute(params![
                idb(id),
                module,
                idb(input_entity),
                trigger.as_str(),
                run_id.map(idb),
                depth,
                state.as_str(),
                reason,
                ts(self.now),
                state.is_terminal().then(|| ts(self.now)),
            ])?;
        self.audit(
            actor,
            "job.add",
            &json!({ "id": id, "module": module, "input": input_entity, "trigger": trigger, "run": run_id, "depth": depth, "state": state, "reason": reason }),
        )?;
        Ok(id)
    }

    pub fn set_job_state(
        &mut self,
        actor: &Actor,
        job: JobId,
        state: JobState,
        reason: Option<&str>,
        error: Option<&str>,
    ) -> StoreResult<()> {
        let now = ts(self.now);
        let n = self
            .conn()
            .prepare_cached(
                "UPDATE job SET state = ?2, reason = COALESCE(?3, reason), error = COALESCE(?4, error),
                   started_at = CASE WHEN ?2 = 'running' AND started_at IS NULL THEN ?5 ELSE started_at END,
                   finished_at = CASE WHEN ?2 IN ('done','failed','blocked','cached') THEN ?5 ELSE finished_at END
                 WHERE id = ?1",
            )?
            .execute(params![idb(job), state.as_str(), reason, error, now])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("job {job}")));
        }
        self.audit(
            actor,
            "job.state",
            &json!({ "id": job, "state": state, "reason": reason, "error": error }),
        )?;
        Ok(())
    }

    /// A playbook job satisfied by an existing observation or job.
    pub fn link_job(
        &mut self,
        actor: &Actor,
        job: JobId,
        cached: Option<ObservationId>,
        joined: Option<JobId>,
    ) -> StoreResult<()> {
        self.conn()
            .prepare_cached("UPDATE job SET cached_observation = COALESCE(?2, cached_observation), joined_job = COALESCE(?3, joined_job) WHERE id = ?1")?
            .execute(params![idb(job), cached.map(idb), joined.map(idb)])?;
        self.audit(
            actor,
            "job.link",
            &json!({ "id": job, "cached_observation": cached, "joined_job": joined }),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ findings & notes

    pub fn add_finding(
        &mut self,
        actor: &Actor,
        title: &str,
        category: &str,
        claim: &str,
        relevance: &str,
    ) -> StoreResult<i64> {
        let now = ts(self.now);
        self.conn()
            .prepare_cached("INSERT INTO finding (title, category, claim, relevance, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?5)")?
            .execute(params![title, category, claim, relevance, now])?;
        let id = self.conn().last_insert_rowid();
        self.audit(actor, "finding.add", &json!({ "id": id, "title": title, "category": category, "claim": claim, "relevance": relevance }))?;
        Ok(id)
    }

    /// Change any of a finding's fields; `None` leaves a field alone.
    pub fn update_finding(&mut self, actor: &Actor, id: i64, u: &FindingUpdate) -> StoreResult<()> {
        let n = self
            .conn()
            .prepare_cached(
                "UPDATE finding SET title = COALESCE(?2, title), category = COALESCE(?3, category), claim = COALESCE(?4, claim),
                   relevance = COALESCE(?5, relevance), status = COALESCE(?6, status), updated_at = ?7 WHERE id = ?1",
            )?
            .execute(params![id, u.title, u.category, u.claim, u.relevance, u.status.map(FindingStatus::as_str), ts(self.now)])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("finding F-{id}")));
        }
        self.audit(
            actor,
            "finding.update",
            &json!({ "id": id, "title": u.title, "category": u.category, "claim": u.claim, "relevance": u.relevance, "status": u.status }),
        )?;
        Ok(())
    }

    pub fn delete_finding(&mut self, actor: &Actor, id: i64) -> StoreResult<()> {
        let c = self.conn();
        c.execute("DELETE FROM finding_entity WHERE finding_id = ?1", [id])?;
        c.execute("DELETE FROM finding_evidence WHERE finding_id = ?1", [id])?;
        c.execute(
            "UPDATE note SET finding_id = NULL WHERE finding_id = ?1",
            [id],
        )?;
        let n = c.execute("DELETE FROM finding WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("finding F-{id}")));
        }
        self.audit(actor, "finding.delete", &json!({ "id": id }))?;
        Ok(())
    }

    /// Attach (`attach = true`) or detach an entity from a finding.
    pub fn finding_entity(
        &mut self,
        actor: &Actor,
        finding: i64,
        entity: EntityId,
        attach: bool,
    ) -> StoreResult<bool> {
        let sql = if attach {
            "INSERT OR IGNORE INTO finding_entity (finding_id, entity_id) VALUES (?1, ?2)"
        } else {
            "DELETE FROM finding_entity WHERE finding_id = ?1 AND entity_id = ?2"
        };
        let n = self
            .conn()
            .prepare_cached(sql)?
            .execute(params![finding, idb(entity)])?;
        if n > 0 {
            let action = if attach {
                "finding.entity.add"
            } else {
                "finding.entity.remove"
            };
            self.audit(
                actor,
                action,
                &json!({ "finding": finding, "entity": entity }),
            )?;
        }
        Ok(n > 0)
    }

    /// Attach or detach an observation as a finding's evidence.
    pub fn finding_evidence(
        &mut self,
        actor: &Actor,
        finding: i64,
        observation: ObservationId,
        attach: bool,
    ) -> StoreResult<bool> {
        let sql = if attach {
            "INSERT OR IGNORE INTO finding_evidence (finding_id, observation_id) VALUES (?1, ?2)"
        } else {
            "DELETE FROM finding_evidence WHERE finding_id = ?1 AND observation_id = ?2"
        };
        let n = self
            .conn()
            .prepare_cached(sql)?
            .execute(params![finding, idb(observation)])?;
        if n > 0 {
            let action = if attach {
                "finding.evidence.add"
            } else {
                "finding.evidence.remove"
            };
            self.audit(
                actor,
                action,
                &json!({ "finding": finding, "observation": observation }),
            )?;
        }
        Ok(n > 0)
    }

    pub fn add_note(
        &mut self,
        actor: &Actor,
        entity: Option<EntityId>,
        finding: Option<i64>,
        body: &str,
    ) -> StoreResult<NoteId> {
        let id = NoteId(self.new_uuid());
        self.conn()
            .prepare_cached("INSERT INTO note (id, entity_id, finding_id, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![idb(id), entity.map(idb), finding, body, ts(self.now)])?;
        self.audit(
            actor,
            "note.add",
            &json!({ "id": id, "entity": entity, "finding": finding, "body": body }),
        )?;
        Ok(id)
    }

    pub fn edit_note(&mut self, actor: &Actor, id: NoteId, body: &str) -> StoreResult<()> {
        let n = self.conn().execute(
            "UPDATE note SET body = ?2 WHERE id = ?1",
            params![idb(id), body],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("note {id}")));
        }
        self.audit(actor, "note.edit", &json!({ "id": id, "body": body }))?;
        Ok(())
    }

    // ------------------------------------------------------------ scope & settings

    pub fn add_scope_rule(
        &mut self,
        actor: &Actor,
        kind: ScopeKind,
        matcher: ScopeMatcher,
        pattern: &str,
    ) -> StoreResult<ScopeRuleId> {
        let id = ScopeRuleId(self.new_uuid());
        self.conn()
            .prepare_cached("INSERT INTO scope_rule (id, kind, matcher, pattern, created_at) VALUES (?1, ?2, ?3, ?4, ?5)")?
            .execute(params![idb(id), kind.as_str(), matcher.as_str(), pattern, ts(self.now)])?;
        self.audit(
            actor,
            "scope.add",
            &json!({ "id": id, "kind": kind, "matcher": matcher, "pattern": pattern }),
        )?;
        Ok(id)
    }

    pub fn remove_scope_rule(&mut self, actor: &Actor, id: ScopeRuleId) -> StoreResult<()> {
        let n = self
            .conn()
            .execute("DELETE FROM scope_rule WHERE id = ?1", [idb(id)])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("scope rule {id}")));
        }
        self.audit(actor, "scope.remove", &json!({ "id": id }))?;
        Ok(())
    }

    pub fn set_module_setting(
        &mut self,
        actor: &Actor,
        module: &str,
        enabled: bool,
        config: &Value,
    ) -> StoreResult<()> {
        self.conn()
            .prepare_cached(
                "INSERT INTO module_setting (module, enabled, config) VALUES (?1, ?2, ?3)
                 ON CONFLICT(module) DO UPDATE SET enabled = excluded.enabled, config = excluded.config",
            )?
            .execute(params![module, enabled, serde_json::to_string(config)?])?;
        self.audit(
            actor,
            "module.setting",
            &json!({ "module": module, "enabled": enabled, "config": config }),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ review queue

    pub fn add_review_item(
        &mut self,
        actor: &Actor,
        kind: ReviewKind,
        payload: &Value,
    ) -> StoreResult<ReviewItemId> {
        let id = ReviewItemId(self.new_uuid());
        self.conn()
            .prepare_cached("INSERT INTO review_item (id, kind, payload, state, created_at) VALUES (?1, ?2, ?3, 'pending', ?4)")?
            .execute(params![idb(id), kind.as_str(), serde_json::to_string(payload)?, ts(self.now)])?;
        self.audit(
            actor,
            "review.add",
            &json!({ "id": id, "kind": kind, "payload": payload }),
        )?;
        Ok(id)
    }

    pub fn decide_review_item(
        &mut self,
        actor: &Actor,
        id: ReviewItemId,
        state: ReviewState,
    ) -> StoreResult<()> {
        let decided =
            (state != ReviewState::Pending && state != ReviewState::Later).then(|| ts(self.now));
        let n = self.conn().execute(
            "UPDATE review_item SET state = ?2, decided_at = ?3 WHERE id = ?1",
            params![idb(id), state.as_str(), decided],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("review item {id}")));
        }
        self.audit(actor, "review.decide", &json!({ "id": id, "state": state }))?;
        Ok(())
    }

    // ------------------------------------------------------------ playbooks

    pub fn add_playbook_run(
        &mut self,
        actor: &Actor,
        playbook: &str,
        version: &str,
        hash: &str,
        seed: EntityId,
        parent: Option<PlaybookRunId>,
    ) -> StoreResult<PlaybookRunId> {
        let id = PlaybookRunId(self.new_uuid());
        self.conn()
            .prepare_cached(
                "INSERT INTO playbook_run (id, playbook, playbook_version, playbook_hash, seed_entity, parent_run, state, current_step, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', 0, ?7)",
            )?
            .execute(params![idb(id), playbook, version, hash, idb(seed), parent.map(idb), ts(self.now)])?;
        self.audit(
            actor,
            "playbook.start",
            &json!({ "id": id, "playbook": playbook, "version": version, "hash": hash, "seed": seed, "parent": parent }),
        )?;
        Ok(id)
    }

    pub fn set_run_state(
        &mut self,
        actor: &Actor,
        run: PlaybookRunId,
        state: RunState,
        current_step: u32,
    ) -> StoreResult<()> {
        let finished = (state != RunState::Running).then(|| ts(self.now));
        let n = self.conn().execute(
            "UPDATE playbook_run SET state = ?2, current_step = ?3, finished_at = ?4 WHERE id = ?1",
            params![idb(run), state.as_str(), current_step, finished],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("playbook run {run}")));
        }
        let action = match state {
            RunState::Running => "playbook.progress",
            RunState::Done => "playbook.finish",
            RunState::Stopped => "playbook.stop",
            RunState::Failed => "playbook.fail",
        };
        self.audit(
            actor,
            action,
            &json!({ "id": run, "state": state, "current_step": current_step }),
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_step(
        &mut self,
        actor: &Actor,
        run: PlaybookRunId,
        step: u32,
        kind: StepKind,
        state: StepState,
        selected: u32,
        capped: u32,
        result: &Value,
    ) -> StoreResult<()> {
        let now = ts(self.now);
        self.conn()
            .prepare_cached(
                "INSERT INTO playbook_step (run_id, step, kind, state, selected, capped, result, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CASE WHEN ?4 = 'pending' THEN NULL ELSE ?8 END,
                         CASE WHEN ?4 IN ('done','stopped') THEN ?8 ELSE NULL END)
                 ON CONFLICT(run_id, step) DO UPDATE SET state = excluded.state, selected = excluded.selected,
                   capped = excluded.capped, result = excluded.result,
                   started_at = COALESCE(playbook_step.started_at, excluded.started_at),
                   finished_at = excluded.finished_at",
            )?
            .execute(params![idb(run), step, kind.as_str(), state.as_str(), selected, capped, serde_json::to_string(result)?, now])?;
        self.audit(
            actor,
            "playbook.step",
            &json!({ "run": run, "step": step, "kind": kind, "state": state, "selected": selected, "capped": capped, "result": result }),
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_checklist_item(
        &mut self,
        actor: &Actor,
        run: PlaybookRunId,
        step: u32,
        entity: Option<EntityId>,
        text: &str,
        launchers: &[String],
        screen: Option<&str>,
    ) -> StoreResult<ChecklistItemId> {
        let id = ChecklistItemId(self.new_uuid());
        self.conn()
            .prepare_cached(
                "INSERT INTO checklist_item (id, run_id, step, entity_id, text, launchers, screen, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open')",
            )?
            .execute(params![idb(id), idb(run), step, entity.map(idb), text, serde_json::to_string(launchers)?, screen])?;
        self.audit(
            actor,
            "checklist.add",
            &json!({ "id": id, "run": run, "step": step, "entity": entity, "text": text, "launchers": launchers, "screen": screen }),
        )?;
        Ok(id)
    }

    /// Resolve (`done`/`skipped`) or reopen an item, optionally with a note.
    pub fn resolve_checklist_item(
        &mut self,
        actor: &Actor,
        id: ChecklistItemId,
        state: ItemState,
        note: Option<&str>,
    ) -> StoreResult<()> {
        let resolved = (state != ItemState::Open).then(|| ts(self.now));
        let n = self.conn().execute(
            "UPDATE checklist_item SET state = ?2, note = COALESCE(?3, note), resolved_at = ?4 WHERE id = ?1",
            params![idb(id), state.as_str(), note, resolved],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("checklist item {id}")));
        }
        self.audit(
            actor,
            "checklist.resolve",
            &json!({ "id": id, "state": state, "note": note }),
        )?;
        Ok(())
    }

    pub fn set_checklist_note(
        &mut self,
        actor: &Actor,
        id: ChecklistItemId,
        note: &str,
    ) -> StoreResult<()> {
        let n = self.conn().execute(
            "UPDATE checklist_item SET note = ?2 WHERE id = ?1",
            params![idb(id), note],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("checklist item {id}")));
        }
        self.audit(actor, "checklist.note", &json!({ "id": id, "note": note }))?;
        Ok(())
    }

    /// An audited record of an analyst action that changes nothing else
    /// (e.g. opening a launcher, MODULES §8).
    pub fn record(&mut self, actor: &Actor, action: &str, payload: &Value) -> StoreResult<i64> {
        self.audit(actor, action, payload)
    }

    /// Edge state for tests and the engine.
    pub fn edge_state(&self, edge: EdgeId) -> StoreResult<EdgeState> {
        self.reader()
            .edge(edge)?
            .map(|e| e.state)
            .ok_or_else(|| StoreError::NotFound(format!("edge {edge}")))
    }
}
