//! The entities pane's rows (TUI_SPEC §4.2): groups by type in spec order,
//! collapsed or expanded, at most 4 rows then `··· N more`, out-of-scope
//! entities summarized at the bottom, fuzzy filtering with nucleo.

use std::collections::{BTreeMap, BTreeSet};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use spektr_engine::EntitySummary;
use spektr_model::{EntityFlags, EntityType};

use crate::state::EntItem;

/// Rows shown per expanded group before `··· N more`.
pub const GROUP_ROWS: usize = 4;
/// Groups with at most this many members start expanded.
pub const AUTO_EXPAND: usize = 5;

/// Sort for the pane: type order, then subject and seeds, then depth, then
/// value.
pub fn sort(entities: &mut [EntitySummary]) {
    entities.sort_by(|a, b| {
        let rank = |e: &EntitySummary| {
            if e.flags.contains(EntityFlags::SUBJECT) {
                0
            } else if e.flags.contains(EntityFlags::SEED) {
                1
            } else {
                2
            }
        };
        a.ty.order()
            .cmp(&b.ty.order())
            .then(rank(a).cmp(&rank(b)))
            .then(a.depth.cmp(&b.depth))
            .then(a.value.cmp(&b.value))
    });
}

/// Indices of entities matching a fuzzy filter, in list order.
pub fn matching(entities: &[EntitySummary], text: &str) -> Vec<usize> {
    if text.trim().is_empty() {
        return (0..entities.len()).collect();
    }
    let pattern = Pattern::parse(text, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    entities
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            pattern
                .score(Utf32Str::new(&e.value, &mut buf), &mut matcher)
                .is_some()
        })
        .map(|(i, _)| i)
        .collect()
}

pub struct Build<'a> {
    pub entities: &'a [EntitySummary],
    pub filter: Option<&'a str>,
    pub open: &'a BTreeMap<String, bool>,
    pub all: &'a BTreeSet<String>,
}

/// The pane's rows, and how many entities matched the filter.
pub fn build(b: &Build<'_>) -> (Vec<EntItem>, usize) {
    let filtering = b.filter.is_some_and(|f| !f.trim().is_empty());
    let picked = matching(b.entities, b.filter.unwrap_or(""));
    let matches = picked.len();
    let mut groups: Vec<(EntityType, Vec<usize>)> = Vec::new();
    let mut hidden = 0;
    for i in picked {
        let e = &b.entities[i];
        if e.flags.contains(EntityFlags::OUT_OF_SCOPE) && !filtering {
            hidden += 1;
            continue;
        }
        match groups.last_mut() {
            Some((ty, rows)) if *ty == e.ty => rows.push(i),
            _ => groups.push((e.ty.clone(), vec![i])),
        }
    }
    let mut out = Vec::new();
    for (ty, rows) in groups {
        let name = ty.as_str().to_string();
        let expanded = filtering
            || b.open
                .get(&name)
                .copied()
                .unwrap_or(rows.len() <= AUTO_EXPAND);
        out.push(EntItem::Header {
            ty: ty.clone(),
            count: rows.len(),
            expanded,
        });
        if expanded {
            let show_all = filtering || b.all.contains(&name) || rows.len() <= GROUP_ROWS;
            let shown = if show_all { rows.len() } else { GROUP_ROWS };
            out.extend(rows.iter().take(shown).map(|i| EntItem::Row(*i)));
            if shown < rows.len() {
                out.push(EntItem::More {
                    ty,
                    hidden: rows.len() - shown,
                });
            }
            out.push(EntItem::Blank);
        }
    }
    if hidden > 0 {
        if !matches!(out.last(), Some(EntItem::Blank) | None) {
            out.push(EntItem::Blank);
        }
        out.push(EntItem::Hidden(hidden));
    } else if matches!(out.last(), Some(EntItem::Blank)) {
        out.pop();
    }
    (out, matches)
}

/// Whether the cursor can rest on a row.
pub fn selectable(item: &EntItem) -> bool {
    !matches!(item, EntItem::Blank | EntItem::Hidden(_))
}
