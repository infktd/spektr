//! The timeline tab's data: every event attribute across the case
//! (DATA_MODEL §2, TUI_SPEC §4.3), oldest first.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use spektr_model::{Attrs, EntityId, ObservationId};

use crate::error::StoreResult;
use crate::reads::Reader;
use crate::rows::EntitySummary;
use crate::sql::{idb, json_col, uuid_col};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub at: Timestamp,
    /// The root entity carrying the attribute.
    pub entity: EntitySummary,
    pub attr: String,
    pub value: String,
    pub observation: ObservationId,
    /// The module (or `analyst`, `capture`, …) that recorded it.
    pub module: String,
}

impl Reader<'_> {
    pub fn timeline(&self) -> StoreResult<Vec<TimelineEvent>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT COALESCE(merged_into, id), attrs FROM entity WHERE attrs != '{}'",
        )?;
        let rows: Vec<(EntityId, Attrs)> = stmt
            .query_map([], |r| Ok((EntityId(uuid_col(r, 0)?), json_col(r, 1)?)))?
            .collect::<Result<_, _>>()?;
        let mut summaries: HashMap<EntityId, EntitySummary> = HashMap::new();
        let mut modules: HashMap<ObservationId, String> = HashMap::new();
        let mut module_stmt = self
            .conn
            .prepare_cached("SELECT module FROM observation WHERE id = ?1")?;
        let mut out = Vec::new();
        for (root, attrs) in rows {
            for (name, list) in attrs {
                for a in list.into_iter().filter(|a| a.event) {
                    let Some(at) = a.value.instant() else {
                        continue;
                    };
                    if !summaries.contains_key(&root)
                        && let Some(s) = self.summary(root)?
                    {
                        summaries.insert(root, s);
                    }
                    let Some(entity) = summaries.get(&root).cloned() else {
                        continue;
                    };
                    let module = match modules.get(&a.observation) {
                        Some(m) => m.clone(),
                        None => {
                            let m: String = module_stmt
                                .query_row([idb(a.observation)], |r| r.get(0))
                                .unwrap_or_else(|_| "unknown".into());
                            modules.insert(a.observation, m.clone());
                            m
                        }
                    };
                    out.push(TimelineEvent {
                        at,
                        entity,
                        attr: name.clone(),
                        value: a.value.display(),
                        observation: a.observation,
                        module,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            a.at.cmp(&b.at)
                .then_with(|| a.entity.value.cmp(&b.entity.value))
                .then_with(|| a.attr.cmp(&b.attr))
        });
        Ok(out)
    }
}
