//! Read queries. Every read resolves merged entities through
//! `COALESCE(merged_into, id)`, so callers always see root entities.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use spektr_model::{
    ChecklistItemId, EdgeId, EntityId, EntityType, JobId, MergeId, ObservationId, PlaybookRunId,
    ReviewItemId,
};

use crate::audit::{AUDIT_COLS, AuditRow, VerifyReport};
use crate::blob::{BlobInfo, Sha256, decompress};
use crate::db::OpenOptions;
use crate::error::{StoreError, StoreResult};
use crate::records::{
    ChecklistItemRow, FINDING_COLS, FindingRow, ITEM_COLS, ItemState, JOB_COLS, JobRow, JobState,
    MergeRow, ModuleSettingRow, NoteRow, PlaybookRunRow, PlaybookStepRow, RUN_COLS, ReviewItemRow,
    ReviewState, ScopeRuleRow,
};
use crate::rows::{
    EDGE_COLS, ENTITY_COLS, EdgeRow, EntityRow, EntitySummary, EvidenceRow, OBSERVATION_COLS,
    ObservationRow, SUMMARY_COLS,
};
use crate::sql::{idb, ts, uuid_col};

fn count(conn: &Connection, sql: &str) -> StoreResult<u64> {
    let n: i64 = conn.query_row(sql, [], |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// A read view over a connection (or over a write transaction).
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) opts: &'a OpenOptions,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(conn: &'a Connection, opts: &'a OpenOptions) -> Self {
        Self { conn, opts }
    }

    // ------------------------------------------------------------ meta & counts

    pub fn meta(&self, key: &str) -> StoreResult<Option<Value>> {
        let raw: Option<String> = self
            .conn
            .prepare_cached("SELECT value FROM case_meta WHERE key = ?1")?
            .query_row([key], |r| r.get(0))
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(StoreError::from))
            .transpose()
    }

    pub fn meta_all(&self) -> StoreResult<BTreeMap<String, Value>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT key, value FROM case_meta ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, serde_json::from_str(&v)?);
        }
        Ok(out)
    }

    /// Root entities.
    pub fn entity_count(&self) -> StoreResult<u64> {
        count(
            self.conn,
            "SELECT count(*) FROM entity WHERE merged_into IS NULL",
        )
    }

    /// Active edges.
    pub fn edge_count(&self) -> StoreResult<u64> {
        count(
            self.conn,
            "SELECT count(*) FROM edge WHERE state = 'active'",
        )
    }

    pub fn observation_count(&self) -> StoreResult<u64> {
        count(self.conn, "SELECT count(*) FROM observation")
    }

    /// The audit row that recorded an observation (`observation.add`).
    pub fn observation_audit_seq(&self, id: ObservationId) -> StoreResult<Option<i64>> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT seq FROM audit WHERE action = 'observation.add' AND json_extract(payload, '$.id') = ?1",
            )?
            .query_row([id.to_string()], |r| r.get(0))
            .optional()?)
    }

    pub fn audit_count(&self) -> StoreResult<u64> {
        count(self.conn, "SELECT count(*) FROM audit")
    }

    // ------------------------------------------------------------ entities

    pub fn entity(&self, id: EntityId) -> StoreResult<Option<EntityRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!("SELECT {ENTITY_COLS} FROM entity WHERE id = ?1"))?
            .query_row([idb(id)], EntityRow::from_row)
            .optional()?)
    }

    /// The row with this `(type, key)`, merged or not.
    pub fn entity_by_key(&self, ty: &EntityType, key: &str) -> StoreResult<Option<EntityRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {ENTITY_COLS} FROM entity WHERE type = ?1 AND key = ?2"
            ))?
            .query_row(params![ty.as_str(), key], EntityRow::from_row)
            .optional()?)
    }

    /// The root entity for `(type, key)`.
    pub fn find(&self, ty: &EntityType, key: &str) -> StoreResult<Option<EntityId>> {
        let raw: Option<Vec<u8>> = self
            .conn
            .prepare_cached(
                "SELECT COALESCE(merged_into, id) FROM entity WHERE type = ?1 AND key = ?2",
            )?
            .query_row(params![ty.as_str(), key], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|b| EntityId::from_slice(&b)))
    }

    /// `merged_into` always points at a root, so one hop resolves.
    pub fn resolve(&self, id: EntityId) -> StoreResult<EntityId> {
        let raw: Option<Vec<u8>> = self
            .conn
            .prepare_cached("SELECT COALESCE(merged_into, id) FROM entity WHERE id = ?1")?
            .query_row([idb(id)], |r| r.get(0))
            .optional()?;
        raw.and_then(|b| EntityId::from_slice(&b))
            .ok_or_else(|| StoreError::NotFound(format!("entity {id}")))
    }

    /// A root and every entity merged into it.
    pub fn members(&self, root: EntityId) -> StoreResult<Vec<EntityId>> {
        let mut out = vec![root];
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM entity WHERE merged_into = ?1 ORDER BY id")?;
        for id in stmt.query_map([idb(root)], |r| uuid_col(r, 0))? {
            out.push(EntityId(id?));
        }
        Ok(out)
    }

    pub fn summary(&self, id: EntityId) -> StoreResult<Option<EntitySummary>> {
        Ok(self
            .conn
            .prepare_cached(&format!("SELECT {SUMMARY_COLS} FROM entity WHERE id = ?1"))?
            .query_row([idb(id)], EntitySummary::from_row)
            .optional()?)
    }

    /// Every root entity, in entities-pane order (type order, then value).
    pub fn entities(&self) -> StoreResult<Vec<EntitySummary>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {SUMMARY_COLS} FROM entity WHERE merged_into IS NULL"
        ))?;
        let mut out: Vec<EntitySummary> = stmt
            .query_map([], EntitySummary::from_row)?
            .collect::<Result<_, _>>()?;
        out.sort_by(|a, b| {
            a.ty.order()
                .cmp(&b.ty.order())
                .then_with(|| a.value.to_lowercase().cmp(&b.value.to_lowercase()))
                .then_with(|| a.key.cmp(&b.key))
        });
        Ok(out)
    }

    /// Root entities carrying attribute `name` (correlators).
    pub fn entities_with_attr(&self, name: &str) -> StoreResult<Vec<EntityRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ENTITY_COLS} FROM entity WHERE merged_into IS NULL AND json_extract(attrs, '$.' || ?1) IS NOT NULL ORDER BY id"
        ))?;
        Ok(stmt
            .query_map([name], EntityRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Root entities with a flag set (seeds, subject, hubs, …).
    pub fn entities_with_flag(
        &self,
        flag: spektr_model::EntityFlags,
    ) -> StoreResult<Vec<EntitySummary>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {SUMMARY_COLS} FROM entity WHERE merged_into IS NULL AND (flags & ?1) = ?1 ORDER BY id"
        ))?;
        Ok(stmt
            .query_map([flag.0], EntitySummary::from_row)?
            .collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------ edges & evidence

    pub fn edge(&self, id: EdgeId) -> StoreResult<Option<EdgeRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!("SELECT {EDGE_COLS} FROM edge WHERE id = ?1"))?
            .query_row([idb(id)], EdgeRow::from_row)
            .optional()?)
    }

    /// Stored edges touching any of `ids` (any state).
    pub fn edges_touching(&self, ids: &[EntityId]) -> StoreResult<Vec<EdgeRow>> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        let mut by_src = self
            .conn
            .prepare_cached(&format!("SELECT {EDGE_COLS} FROM edge WHERE src = ?1"))?;
        let mut by_dst = self
            .conn
            .prepare_cached(&format!("SELECT {EDGE_COLS} FROM edge WHERE dst = ?1"))?;
        for id in ids {
            for stmt in [&mut by_src, &mut by_dst] {
                for e in stmt.query_map([idb(*id)], EdgeRow::from_row)? {
                    let e = e?;
                    if seen.insert(e.id) {
                        out.push(e);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn evidence(&self, edge: EdgeId) -> StoreResult<Vec<EvidenceRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT ev.observation_id, ev.source_key FROM edge_evidence ev JOIN observation o ON o.id = ev.observation_id
             WHERE ev.edge_id = ?1 ORDER BY o.created_at, ev.observation_id",
        )?;
        let rows = stmt.query_map([idb(edge)], |r| {
            Ok(EvidenceRow {
                edge,
                observation: ObservationId(uuid_col(r, 0)?),
                source_key: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------ observations & blobs

    pub fn observation(&self, id: ObservationId) -> StoreResult<Option<ObservationRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {OBSERVATION_COLS} FROM observation WHERE id = ?1"
            ))?
            .query_row([idb(id)], ObservationRow::from_row)
            .optional()?)
    }

    /// Everything that supports an entity: evidence on its edges, the
    /// sources of its attributes, and runs that took it as input. Newest
    /// first.
    pub fn entity_observations(&self, root: EntityId) -> StoreResult<Vec<ObservationRow>> {
        let members = self.members(root)?;
        let mut ids: BTreeSet<ObservationId> = BTreeSet::new();
        for e in self.edges_touching(&members)? {
            if e.state == crate::rows::EdgeState::Active {
                ids.extend(self.evidence(e.id)?.into_iter().map(|ev| ev.observation));
            }
        }
        for m in &members {
            if let Some(row) = self.entity(*m)? {
                for list in row.attrs.values() {
                    ids.extend(list.iter().map(|a| a.observation));
                }
            }
            let mut stmt = self
                .conn
                .prepare_cached("SELECT id FROM observation WHERE input_entity = ?1")?;
            for id in stmt.query_map([idb(*m)], |r| uuid_col(r, 0))? {
                ids.insert(ObservationId(id?));
            }
        }
        let mut out = Vec::new();
        for id in ids {
            if let Some(o) = self.observation(id)? {
                out.push(o);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        Ok(out)
    }

    /// Decompressed blob bytes.
    pub fn blob(&self, sha: Sha256) -> StoreResult<Option<Vec<u8>>> {
        let row: Option<(i64, String, Vec<u8>)> = self
            .conn
            .prepare_cached("SELECT size, compression, data FROM blob WHERE sha256 = ?1")?
            .query_row([&sha.0[..]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        row.map(|(size, c, data)| decompress(&data, &c, u64::try_from(size).unwrap_or(0)))
            .transpose()
    }

    pub fn blob_info(&self, sha: Sha256) -> StoreResult<Option<BlobInfo>> {
        Ok(self
            .conn
            .prepare_cached("SELECT size, compression FROM blob WHERE sha256 = ?1")?
            .query_row([&sha.0[..]], |r| {
                Ok(BlobInfo {
                    sha256: sha,
                    size: r.get::<_, i64>(0)?.try_into().unwrap_or(0),
                    compression: r.get(1)?,
                })
            })
            .optional()?)
    }

    /// The newest module observation for `(module, version, input)` at or
    /// after `not_before` — the engine's cache lookup (MODULES §5).
    pub fn cached_observation(
        &self,
        module: &str,
        version: &str,
        input: EntityId,
        not_before: Timestamp,
    ) -> StoreResult<Option<ObservationId>> {
        let raw: Option<Vec<u8>> = self
            .conn
            .prepare_cached(
                "SELECT id FROM observation WHERE module = ?1 AND module_version = ?2 AND input_entity = ?3 AND created_at >= ?4 AND kind = 'module'
                 ORDER BY created_at DESC LIMIT 1",
            )?
            .query_row(params![module, version, idb(input), ts(not_before)], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|b| ObservationId::from_slice(&b)))
    }

    // ------------------------------------------------------------ jobs

    pub fn job(&self, id: JobId) -> StoreResult<Option<JobRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!("SELECT {JOB_COLS} FROM job WHERE id = ?1"))?
            .query_row([idb(id)], JobRow::from_row)
            .optional()?)
    }

    pub fn jobs(&self, states: &[JobState]) -> StoreResult<Vec<JobRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {JOB_COLS} FROM job ORDER BY queued_at, id"
        ))?;
        let all: Vec<JobRow> = stmt
            .query_map([], JobRow::from_row)?
            .collect::<Result<_, _>>()?;
        Ok(all
            .into_iter()
            .filter(|j| states.is_empty() || states.contains(&j.state))
            .collect())
    }

    /// A queued or running job for `(module, input)`, if any.
    pub fn active_job(&self, module: &str, input: EntityId) -> StoreResult<Option<JobId>> {
        let raw: Option<Vec<u8>> = self
            .conn
            .prepare_cached("SELECT id FROM job WHERE module = ?1 AND input_entity = ?2 AND state IN ('queued', 'running') LIMIT 1")?
            .query_row(params![module, idb(input)], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|b| JobId::from_slice(&b)))
    }

    /// Jobs of `module` that ran (or are running) since `since` — the daily
    /// budget's usage.
    pub fn jobs_since(&self, module: &str, since: Timestamp) -> StoreResult<u64> {
        let n: i64 = self
            .conn
            .prepare_cached("SELECT count(*) FROM job WHERE module = ?1 AND queued_at >= ?2 AND state IN ('running', 'done', 'failed')")?
            .query_row(params![module, ts(since)], |r| r.get(0))?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    pub fn jobs_for_run(&self, run: PlaybookRunId) -> StoreResult<Vec<JobRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {JOB_COLS} FROM job WHERE run_id = ?1 ORDER BY queued_at, id"
        ))?;
        Ok(stmt
            .query_map([idb(run)], JobRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn job_counts(&self) -> StoreResult<BTreeMap<JobState, u64>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT state, count(*) FROM job GROUP BY state")?;
        let mut out = BTreeMap::new();
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (s, n) = row?;
            if let Ok(state) = s.parse() {
                out.insert(state, u64::try_from(n).unwrap_or(0));
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ findings & notes

    pub fn findings(&self) -> StoreResult<Vec<FindingRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {FINDING_COLS} FROM finding ORDER BY id"))?;
        Ok(stmt
            .query_map([], FindingRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn finding(&self, id: i64) -> StoreResult<Option<FindingRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!("SELECT {FINDING_COLS} FROM finding WHERE id = ?1"))?
            .query_row([id], FindingRow::from_row)
            .optional()?)
    }

    /// Root entities attached to a finding.
    pub fn finding_entities(&self, id: i64) -> StoreResult<Vec<EntityId>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT COALESCE(e.merged_into, e.id) FROM finding_entity fe JOIN entity e ON e.id = fe.entity_id WHERE fe.finding_id = ?1",
        )?;
        let rows = stmt.query_map([id], |r| uuid_col(r, 0))?;
        Ok(rows.map(|u| u.map(EntityId)).collect::<Result<_, _>>()?)
    }

    pub fn finding_evidence(&self, id: i64) -> StoreResult<Vec<ObservationId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT observation_id FROM finding_evidence WHERE finding_id = ?1 ORDER BY observation_id")?;
        let rows = stmt.query_map([id], |r| uuid_col(r, 0))?;
        Ok(rows
            .map(|u| u.map(ObservationId))
            .collect::<Result<_, _>>()?)
    }

    /// Findings that reference an entity (the inspector's "Used in").
    pub fn findings_for_entity(&self, root: EntityId) -> StoreResult<Vec<FindingRow>> {
        let members = self.members(root)?;
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {} FROM finding f JOIN finding_entity fe ON fe.finding_id = f.id WHERE fe.entity_id = ?1",
            FINDING_COLS.split(", ").map(|c| format!("f.{c}")).collect::<Vec<_>>().join(", ")
        ))?;
        for m in members {
            for f in stmt.query_map([idb(m)], FindingRow::from_row)? {
                let f = f?;
                if seen.insert(f.id) {
                    out.push(f);
                }
            }
        }
        out.sort_by_key(|f| f.id);
        Ok(out)
    }

    pub fn notes_for_entity(&self, root: EntityId) -> StoreResult<Vec<NoteRow>> {
        let mut out = Vec::new();
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, entity_id, finding_id, body, created_at FROM note WHERE entity_id = ?1",
        )?;
        for m in self.members(root)? {
            for n in stmt.query_map([idb(m)], NoteRow::from_row)? {
                out.push(n?);
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    pub fn notes_for_finding(&self, finding: i64) -> StoreResult<Vec<NoteRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, entity_id, finding_id, body, created_at FROM note WHERE finding_id = ?1 ORDER BY created_at, id",
        )?;
        Ok(stmt
            .query_map([finding], NoteRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------ scope, settings, review

    pub fn scope_rules(&self) -> StoreResult<Vec<ScopeRuleRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, matcher, pattern, created_at FROM scope_rule ORDER BY created_at, id",
        )?;
        Ok(stmt
            .query_map([], ScopeRuleRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn module_settings(&self) -> StoreResult<Vec<ModuleSettingRow>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT module, enabled, config FROM module_setting ORDER BY module")?;
        let rows = stmt.query_map([], |r| {
            Ok(ModuleSettingRow {
                module: r.get(0)?,
                enabled: r.get(1)?,
                config: crate::sql::json_col(r, 2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn review_items(&self, state: Option<ReviewState>) -> StoreResult<Vec<ReviewItemRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, payload, state, created_at, decided_at FROM review_item ORDER BY created_at, id",
        )?;
        let all: Vec<ReviewItemRow> = stmt
            .query_map([], ReviewItemRow::from_row)?
            .collect::<Result<_, _>>()?;
        Ok(all
            .into_iter()
            .filter(|r| state.is_none_or(|s| r.state == s))
            .collect())
    }

    pub fn review_item(&self, id: ReviewItemId) -> StoreResult<Option<ReviewItemRow>> {
        Ok(self
            .conn
            .prepare_cached("SELECT id, kind, payload, state, created_at, decided_at FROM review_item WHERE id = ?1")?
            .query_row([idb(id)], ReviewItemRow::from_row)
            .optional()?)
    }

    pub fn merge_row(&self, id: MergeId) -> StoreResult<Option<MergeRow>> {
        Ok(self
            .conn
            .prepare_cached("SELECT id, survivor, merged, repointed, created_at, undone_at FROM merge WHERE id = ?1")?
            .query_row([idb(id)], MergeRow::from_row)
            .optional()?)
    }

    /// Merges in the order they were made.
    pub fn merges(&self) -> StoreResult<Vec<MergeRow>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, survivor, merged, repointed, created_at, undone_at FROM merge ORDER BY rowid")?;
        Ok(stmt
            .query_map([], MergeRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------ playbooks

    pub fn playbook_runs(&self) -> StoreResult<Vec<PlaybookRunRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {RUN_COLS} FROM playbook_run ORDER BY started_at, rowid"
        ))?;
        Ok(stmt
            .query_map([], PlaybookRunRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Runs started by `parent` (nested playbook steps).
    pub fn child_runs(&self, parent: PlaybookRunId) -> StoreResult<Vec<PlaybookRunRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {RUN_COLS} FROM playbook_run WHERE parent_run = ?1 ORDER BY started_at, rowid"
        ))?;
        Ok(stmt
            .query_map([idb(parent)], PlaybookRunRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Observations that jobs of these runs produced, hit in the cache, or
    /// joined (PLAYBOOKS §2.3, run membership). A cache hit stands for the
    /// whole job that produced the cached observation.
    pub fn run_observations(&self, runs: &[PlaybookRunId]) -> StoreResult<BTreeSet<ObservationId>> {
        let mut out = BTreeSet::new();
        for run in runs {
            let mut stmt = self.conn.prepare_cached(
                "SELECT o.id FROM observation o JOIN job j ON o.job_id = j.id WHERE j.run_id = ?1
                 UNION SELECT j.cached_observation FROM job j WHERE j.run_id = ?1 AND j.cached_observation IS NOT NULL
                 UNION SELECT o.id FROM job j JOIN observation c ON c.id = j.cached_observation
                       JOIN observation o ON o.job_id = c.job_id WHERE j.run_id = ?1
                 UNION SELECT o.id FROM observation o JOIN job j ON o.job_id = j.joined_job WHERE j.run_id = ?1",
            )?;
            let rows = stmt.query_map([idb(*run)], |r| uuid_col(r, 0))?;
            for row in rows {
                out.insert(ObservationId(row?));
            }
        }
        Ok(out)
    }

    pub fn playbook_run(&self, id: PlaybookRunId) -> StoreResult<Option<PlaybookRunRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {RUN_COLS} FROM playbook_run WHERE id = ?1"
            ))?
            .query_row([idb(id)], PlaybookRunRow::from_row)
            .optional()?)
    }

    pub fn playbook_steps(&self, run: PlaybookRunId) -> StoreResult<Vec<PlaybookStepRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT run_id, step, kind, state, selected, capped, result, started_at, finished_at FROM playbook_step WHERE run_id = ?1 ORDER BY step",
        )?;
        Ok(stmt
            .query_map([idb(run)], PlaybookStepRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn checklist_items(
        &self,
        run: Option<PlaybookRunId>,
        state: Option<ItemState>,
    ) -> StoreResult<Vec<ChecklistItemRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {ITEM_COLS} FROM checklist_item ORDER BY run_id, step, rowid"
        ))?;
        let all: Vec<ChecklistItemRow> = stmt
            .query_map([], ChecklistItemRow::from_row)?
            .collect::<Result<_, _>>()?;
        Ok(all
            .into_iter()
            .filter(|i| run.is_none_or(|r| i.run_id == r) && state.is_none_or(|s| i.state == s))
            .collect())
    }

    pub fn checklist_item(&self, id: ChecklistItemId) -> StoreResult<Option<ChecklistItemRow>> {
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT {ITEM_COLS} FROM checklist_item WHERE id = ?1"
            ))?
            .query_row([idb(id)], ChecklistItemRow::from_row)
            .optional()?)
    }

    /// Open items for an entity (the inspector's "Checklist" section).
    pub fn checklist_items_for_entity(&self, root: EntityId) -> StoreResult<Vec<ChecklistItemRow>> {
        let members = self.members(root)?;
        Ok(self
            .checklist_items(None, Some(ItemState::Open))?
            .into_iter()
            .filter(|i| i.entity.is_some_and(|e| members.contains(&e)))
            .collect())
    }

    // ------------------------------------------------------------ whole-case dumps (export)

    /// Every entity row, merged ones included, by id.
    pub fn all_entities(&self) -> StoreResult<Vec<EntityRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {ENTITY_COLS} FROM entity ORDER BY id"))?;
        Ok(stmt
            .query_map([], EntityRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Every edge row, any state, by id.
    pub fn all_edges(&self) -> StoreResult<Vec<EdgeRow>> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT {EDGE_COLS} FROM edge ORDER BY id"))?;
        Ok(stmt
            .query_map([], EdgeRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Every evidence row, by edge.
    pub fn all_evidence(&self) -> StoreResult<Vec<EvidenceRow>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT edge_id, observation_id, source_key FROM edge_evidence ORDER BY edge_id, observation_id")?;
        let rows = stmt.query_map([], |r| {
            Ok(EvidenceRow {
                edge: EdgeId(uuid_col(r, 0)?),
                observation: ObservationId(uuid_col(r, 1)?),
                source_key: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every observation, oldest first.
    pub fn all_observations(&self) -> StoreResult<Vec<ObservationRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {OBSERVATION_COLS} FROM observation ORDER BY created_at, id"
        ))?;
        Ok(stmt
            .query_map([], ObservationRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn all_notes(&self) -> StoreResult<Vec<NoteRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, entity_id, finding_id, body, created_at FROM note ORDER BY created_at, id",
        )?;
        Ok(stmt
            .query_map([], NoteRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    // ------------------------------------------------------------ audit

    pub fn audit_rows(&self, from_seq: i64, limit: u32) -> StoreResult<Vec<AuditRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {AUDIT_COLS} FROM audit WHERE seq >= ?1 ORDER BY seq LIMIT ?2"
        ))?;
        Ok(stmt
            .query_map(params![from_seq, limit], AuditRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Audit rows with a given action, oldest first.
    pub fn audit_by_action(&self, action: &str) -> StoreResult<Vec<AuditRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {AUDIT_COLS} FROM audit WHERE action = ?1 ORDER BY seq"
        ))?;
        Ok(stmt
            .query_map([action], AuditRow::from_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn verify(&self) -> StoreResult<VerifyReport> {
        crate::audit::verify(self.conn)
    }

    pub fn options(&self) -> &OpenOptions {
        self.opts
    }
}
