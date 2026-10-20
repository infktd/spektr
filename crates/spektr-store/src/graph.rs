//! Graph queries: the ego view (paginated groups with strength), degree,
//! strength, k-hop neighbourhoods and shortest paths. Results are
//! view-ready; the TUI does no graph logic (ARCHITECTURE §4).
//!
//! Edges are stored against the rows they were written for. Every query
//! resolves endpoints through `merged_into`, drops edges that a merge turned
//! into self-loops, and folds duplicate edges (same predicate, discriminator
//! and resolved endpoints) into one row whose strength counts the union of
//! their evidence.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use spektr_model::{
    ArtifactKind, CorePredicate, EdgeId, EntityFlags, EntityId, ObservationId, Predicate, Strength,
};

use crate::error::{StoreError, StoreResult};
use crate::reads::Reader;
use crate::rows::{EdgeState, EntitySummary};
use crate::sql::{idb, opt_ts_col, parse_col, uuid_col};

/// Which side of the focus an edge sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// `other → focus`.
    In,
    /// `focus → other`.
    Out,
    /// A symmetric predicate.
    Sym,
}

/// One ego group: a predicate on one side (plus the artifact kind for
/// `shares_artifact`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupKey {
    pub predicate: Predicate,
    pub direction: Direction,
    pub artifact: Option<ArtifactKind>,
}

impl GroupKey {
    pub fn is_correlated(&self) -> bool {
        self.artifact.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgoRow {
    /// The stored edges folded into this row; the first is the one to act on.
    pub edges: Vec<EdgeId>,
    pub entity: EntitySummary,
    pub strength: Option<Strength>,
    pub confirmed_at: Option<Timestamp>,
    pub rejected: bool,
    /// Distinct source keys behind the row.
    pub sources: usize,
    /// For `shares_artifact`: the shared value.
    pub artifact_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgoGroup {
    pub key: GroupKey,
    pub total: usize,
    /// Zero-based page shown.
    pub page: usize,
    pub pages: usize,
    pub rows: Vec<EgoRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Degree {
    pub inbound: u64,
    pub outbound: u64,
    pub symmetric: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgoView {
    pub focus: EntitySummary,
    pub degree: Degree,
    /// Observations supporting the focus.
    pub observations: usize,
    /// Groups, largest first. The TUI applies its predicate priority on top.
    pub groups: Vec<EgoGroup>,
}

/// Paging for [`Reader::ego`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgoParams {
    /// Rows per group page (TUI_SPEC §4.3: 5).
    pub page_size: usize,
    /// Page per group; groups not listed show page 0.
    pub pages: BTreeMap<GroupKey, usize>,
    /// Show rejected edges too (hidden by default).
    pub include_rejected: bool,
}

impl Default for EgoParams {
    fn default() -> Self {
        Self {
            page_size: 5,
            pages: BTreeMap::new(),
            include_rejected: false,
        }
    }
}

/// One step of a path: the entity, and the edge that led to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathStep {
    pub entity: EntitySummary,
    /// `None` for the start.
    pub via: Option<(EdgeId, Predicate, Direction, Option<Strength>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathView {
    pub steps: Vec<PathStep>,
}

/// A stored edge seen from one resolved endpoint.
#[derive(Debug, Clone)]
struct Incident {
    edge: EdgeId,
    predicate: Predicate,
    discriminator: String,
    state: EdgeState,
    confirmed_at: Option<Timestamp>,
    focus_is_src: bool,
    other: EntitySummary,
}

fn placeholders(start: usize, n: usize) -> String {
    (start..start + n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Reader<'_> {
    /// Stored edges incident to a root (any member), resolved; self-loops
    /// dropped.
    fn incident(&self, root: EntityId, members: &[EntityId]) -> StoreResult<Vec<Incident>> {
        let ph = placeholders(2, members.len());
        let sql = format!(
            "SELECT e.id, e.predicate, e.discriminator, e.state, e.confirmed_at,
                    COALESCE(s.merged_into, s.id) = ?1 AS focus_is_src,
                    o.id, o.type, o.key, o.value, o.flags, o.depth
             FROM edge e
             JOIN entity s ON s.id = e.src
             JOIN entity d ON d.id = e.dst
             JOIN entity o ON o.id = CASE WHEN COALESCE(s.merged_into, s.id) = ?1
                                          THEN COALESCE(d.merged_into, d.id)
                                          ELSE COALESCE(s.merged_into, s.id) END
             WHERE (e.src IN ({ph}) OR e.dst IN ({ph}))
               AND COALESCE(s.merged_into, s.id) != COALESCE(d.merged_into, d.id)"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut params: Vec<Vec<u8>> = vec![idb(root)];
        params.extend(members.iter().map(|m| idb(*m)));
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
            Ok(Incident {
                edge: EdgeId(uuid_col(r, 0)?),
                predicate: parse_col(r, 1)?,
                discriminator: r.get(2)?,
                state: parse_col(r, 3)?,
                confirmed_at: opt_ts_col(r, 4)?,
                focus_is_src: r.get(5)?,
                other: EntitySummary {
                    id: EntityId(uuid_col(r, 6)?),
                    ty: parse_col(r, 7)?,
                    key: r.get(8)?,
                    value: r.get(9)?,
                    flags: EntityFlags(r.get(10)?),
                    depth: r.get(11)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// `(observation, source_key)` evidence for every edge incident to the
    /// members, in one query.
    fn incident_evidence(
        &self,
        members: &[EntityId],
    ) -> StoreResult<HashMap<EdgeId, Vec<(ObservationId, String)>>> {
        let ph = placeholders(1, members.len());
        let sql = format!(
            "SELECT ev.edge_id, ev.observation_id, ev.source_key FROM edge_evidence ev
             WHERE ev.edge_id IN (SELECT id FROM edge WHERE src IN ({ph}) OR dst IN ({ph}))"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let params: Vec<Vec<u8>> = members.iter().map(|m| idb(*m)).collect();
        let mut out: HashMap<EdgeId, Vec<(ObservationId, String)>> = HashMap::new();
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |r| {
            Ok((
                EdgeId(uuid_col(r, 0)?),
                ObservationId(uuid_col(r, 1)?),
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (edge, obs, key) = row?;
            out.entry(edge).or_default().push((obs, key));
        }
        Ok(out)
    }

    /// Incident edges folded into rows, keyed by group.
    fn folded(
        &self,
        root: EntityId,
        include_rejected: bool,
    ) -> StoreResult<BTreeMap<GroupKey, Vec<EgoRow>>> {
        let members = self.members(root)?;
        let incident = self.incident(root, &members)?;
        let evidence = self.incident_evidence(&members)?;

        struct Acc {
            edges: Vec<EdgeId>,
            entity: EntitySummary,
            keys: BTreeSet<String>,
            confirmed_at: Option<Timestamp>,
            any_active: bool,
            artifact_value: Option<String>,
        }
        let mut acc: BTreeMap<(GroupKey, EntityId, String), Acc> = BTreeMap::new();
        for inc in incident {
            let rejected = inc.state == EdgeState::Rejected;
            if rejected && !include_rejected {
                continue;
            }
            let direction = if inc.predicate.is_symmetric() {
                Direction::Sym
            } else if inc.focus_is_src {
                Direction::Out
            } else {
                Direction::In
            };
            let artifact = if inc.predicate == Predicate::Core(CorePredicate::SharesArtifact) {
                ArtifactKind::parse_discriminator(&inc.discriminator)
            } else {
                None
            };
            let key = GroupKey {
                predicate: inc.predicate.clone(),
                direction,
                artifact: artifact.map(|a| a.0),
            };
            let a = acc
                .entry((key, inc.other.id, inc.discriminator.clone()))
                .or_insert_with(|| Acc {
                    edges: Vec::new(),
                    entity: inc.other.clone(),
                    keys: BTreeSet::new(),
                    confirmed_at: None,
                    any_active: false,
                    artifact_value: artifact.map(|a| a.1.to_string()),
                });
            a.edges.push(inc.edge);
            if !rejected {
                a.any_active = true;
                if let Some(ev) = evidence.get(&inc.edge) {
                    a.keys.extend(ev.iter().map(|(_, k)| k.clone()));
                }
                a.confirmed_at = a.confirmed_at.max(inc.confirmed_at);
            }
        }

        let mut groups: BTreeMap<GroupKey, Vec<EgoRow>> = BTreeMap::new();
        for ((key, _, _), a) in acc {
            let strength = if a.any_active {
                Strength::compute(a.keys.len(), a.confirmed_at.is_some())
            } else {
                None
            };
            groups.entry(key).or_default().push(EgoRow {
                edges: a.edges,
                entity: a.entity,
                strength,
                confirmed_at: a.confirmed_at,
                rejected: !a.any_active,
                sources: a.keys.len(),
                artifact_value: a.artifact_value,
            });
        }
        for rows in groups.values_mut() {
            rows.sort_by(|x, y| {
                y.strength
                    .cmp(&x.strength)
                    .then_with(|| {
                        x.entity
                            .value
                            .to_lowercase()
                            .cmp(&y.entity.value.to_lowercase())
                    })
                    .then_with(|| x.entity.id.cmp(&y.entity.id))
            });
        }
        Ok(groups)
    }

    /// The ego view of `focus` (TUI_SPEC §4.3): inbound and symmetric groups
    /// for the left column, outbound for the right, each paginated.
    pub fn ego(&self, focus: EntityId, params: &EgoParams) -> StoreResult<EgoView> {
        let root = self.resolve(focus)?;
        let focus_summary = self
            .summary(root)?
            .ok_or_else(|| StoreError::NotFound(format!("entity {root}")))?;
        let folded = self.folded(root, params.include_rejected)?;
        let degree = degree_of(&folded);
        let page_size = params.page_size.max(1);
        let mut groups: Vec<EgoGroup> = folded
            .into_iter()
            .map(|(key, rows)| {
                let total = rows.len();
                let pages = total.div_ceil(page_size).max(1);
                let page = params.pages.get(&key).copied().unwrap_or(0).min(pages - 1);
                let rows = rows
                    .into_iter()
                    .skip(page * page_size)
                    .take(page_size)
                    .collect();
                EgoGroup {
                    key,
                    total,
                    page,
                    pages,
                    rows,
                }
            })
            .collect();
        groups.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.key.cmp(&b.key)));
        let observations = self.entity_observation_count(root)?;
        Ok(EgoView {
            focus: focus_summary,
            degree,
            observations,
            groups,
        })
    }

    /// Distinct observations supporting an entity (the focus card's `obs`).
    pub fn entity_observation_count(&self, root: EntityId) -> StoreResult<usize> {
        let members = self.members(root)?;
        let mut ids: BTreeSet<ObservationId> = BTreeSet::new();
        let active: BTreeSet<EdgeId> = self
            .incident(root, &members)?
            .into_iter()
            .filter(|i| i.state == EdgeState::Active)
            .map(|i| i.edge)
            .collect();
        for (edge, ev) in self.incident_evidence(&members)? {
            if active.contains(&edge) {
                ids.extend(ev.into_iter().map(|(o, _)| o));
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
        Ok(ids.len())
    }

    /// Active, distinct neighbours of a root by direction.
    pub fn degree(&self, focus: EntityId) -> StoreResult<Degree> {
        let root = self.resolve(focus)?;
        Ok(degree_of(&self.folded(root, false)?))
    }

    /// Strength of one stored edge.
    pub fn edge_strength(&self, edge: EdgeId) -> StoreResult<Option<Strength>> {
        let Some(e) = self.edge(edge)? else {
            return Err(StoreError::NotFound(format!("edge {edge}")));
        };
        if e.state == EdgeState::Rejected {
            return Ok(None);
        }
        let keys: BTreeSet<String> = self
            .evidence(edge)?
            .into_iter()
            .map(|ev| ev.source_key)
            .collect();
        Ok(Strength::compute(keys.len(), e.confirmed_at.is_some()))
    }

    /// Entity strength for lists and the inspector (DATA_MODEL §4): the
    /// strongest active edge connecting it directly to a seed or the
    /// subject. `None` for seeds and the subject themselves, and for
    /// entities with no such edge.
    pub fn entity_strength(&self, focus: EntityId) -> StoreResult<Option<Strength>> {
        let root = self.resolve(focus)?;
        let anchor = EntityFlags::SEED.with(EntityFlags::SUBJECT);
        let folded = self.folded(root, false)?;
        Ok(folded
            .values()
            .flatten()
            .filter(|r| r.entity.flags.0 & anchor.0 != 0)
            .filter_map(|r| r.strength)
            .max())
    }

    /// Root entities within `k` hops of `focus` (excluding it), over active
    /// edges in either direction.
    pub fn khop(&self, focus: EntityId, k: usize) -> StoreResult<Vec<(EntitySummary, usize)>> {
        let root = self.resolve(focus)?;
        let mut seen: BTreeSet<EntityId> = BTreeSet::from([root]);
        let mut out = Vec::new();
        let mut frontier = vec![root];
        for hop in 1..=k {
            let mut next = Vec::new();
            for node in frontier {
                for rows in self.folded(node, false)?.into_values() {
                    for row in rows {
                        if seen.insert(row.entity.id) {
                            next.push(row.entity.id);
                            out.push((row.entity, hop));
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        Ok(out)
    }

    /// Shortest path from `from` to `to` over active edges (either
    /// direction), at most `max_hops` long. `None` when there is none.
    pub fn shortest_path(
        &self,
        from: EntityId,
        to: EntityId,
        max_hops: usize,
    ) -> StoreResult<Option<PathView>> {
        let start = self.resolve(from)?;
        let goal = self.resolve(to)?;
        let start_summary = self
            .summary(start)?
            .ok_or_else(|| StoreError::NotFound(format!("entity {start}")))?;
        if start == goal {
            return Ok(Some(PathView {
                steps: vec![PathStep {
                    entity: start_summary,
                    via: None,
                }],
            }));
        }
        type Via = (
            EntityId,
            EdgeId,
            Predicate,
            Direction,
            Option<Strength>,
            EntitySummary,
        );
        let mut parent: HashMap<EntityId, Via> = HashMap::new();
        let mut queue = VecDeque::from([(start, 0usize)]);
        let mut visited = BTreeSet::from([start]);
        while let Some((node, dist)) = queue.pop_front() {
            if dist >= max_hops {
                continue;
            }
            for (key, rows) in self.folded(node, false)? {
                for row in rows {
                    let next = row.entity.id;
                    if !visited.insert(next) {
                        continue;
                    }
                    let edge = row
                        .edges
                        .first()
                        .copied()
                        .ok_or_else(|| StoreError::Invalid("empty ego row".into()))?;
                    parent.insert(
                        next,
                        (
                            node,
                            edge,
                            key.predicate.clone(),
                            key.direction,
                            row.strength,
                            row.entity.clone(),
                        ),
                    );
                    if next == goal {
                        let mut steps = Vec::new();
                        let mut cur = goal;
                        while let Some((prev, edge, pred, dir, strength, summary)) =
                            parent.get(&cur).cloned()
                        {
                            steps.push(PathStep {
                                entity: summary,
                                via: Some((edge, pred, dir, strength)),
                            });
                            cur = prev;
                        }
                        steps.push(PathStep {
                            entity: start_summary,
                            via: None,
                        });
                        steps.reverse();
                        return Ok(Some(PathView { steps }));
                    }
                    queue.push_back((next, dist + 1));
                }
            }
        }
        Ok(None)
    }
}

fn degree_of(groups: &BTreeMap<GroupKey, Vec<EgoRow>>) -> Degree {
    let mut d = Degree::default();
    for (key, rows) in groups {
        let n = rows.iter().filter(|r| !r.rejected).count() as u64;
        match key.direction {
            Direction::In => d.inbound += n,
            Direction::Out => d.outbound += n,
            Direction::Sym => d.symmetric += n,
        }
    }
    d.total = d.inbound + d.outbound + d.symmetric;
    d
}
