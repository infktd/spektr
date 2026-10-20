//! The graph pane's timeline and path tabs (TUI_SPEC §4.3): their queries
//! and keys. Rendering is in `view::tabs`.

use spektr_engine::{EntitySummary, Query, QueryResult};

use crate::keymap::Action;
use crate::msg::{Effect, Tag};
use crate::state::{AppState, Dialog, Field, Overlay, Pane, Purpose, Tab, Workspace};
use crate::update::{pivot, sync_inspect};

/// Shortest paths stop here (TUI_SPEC §4.3).
pub const MAX_HOPS: usize = 6;

/// Whether an event attribute dated after last-seen is activity (`▲`). An
/// expiry date is set at registration, so it never advances the timeline.
pub fn marks_activity(attr: &str) -> bool {
    attr != spektr_model::attrs::names::EXPIRES_AT
}

/// The query the open tab needs, if any.
pub fn queries(w: &Workspace) -> Vec<Effect> {
    match w.tab {
        Tab::Timeline => vec![Effect::Query(Tag::Timeline, Query::Timeline)],
        Tab::Path => match (&w.path_tab.from, &w.path_tab.target) {
            (Some(f), Some(t)) => vec![path_query(f.id, t.id)],
            _ => Vec::new(),
        },
        Tab::Ego => Vec::new(),
    }
}

fn path_query(from: spektr_model::EntityId, to: spektr_model::EntityId) -> Effect {
    Effect::Query(
        Tag::Path(from, to),
        Query::Path {
            from,
            to,
            max_hops: MAX_HOPS,
        },
    )
}

pub fn open_timeline(w: &mut Workspace, fx: &mut Vec<Effect>) {
    w.tab = Tab::Timeline;
    w.pane = Pane::Graph;
    fx.extend(queries(w));
    sync_inspect(w, fx);
}

/// `3`: the path tab, asking for a target first when there's none.
pub fn open_path(s: &mut AppState, fx: &mut Vec<Effect>) {
    let Some(w) = s.workspace_mut() else { return };
    w.tab = Tab::Path;
    w.pane = Pane::Graph;
    if w.path_tab.target.is_some() {
        fx.extend(queries(w));
        sync_inspect(w, fx);
        return;
    }
    path_picker(s);
}

/// The fuzzy picker for a path target.
pub fn path_picker(s: &mut AppState) {
    let Some(w) = s.workspace() else { return };
    let focus = w.focus;
    let mut d = Dialog::input(
        "Path to…",
        vec![Field::Text {
            label: "find".into(),
            value: String::new(),
            hint: "any entity in the case".into(),
        }],
        Purpose::PathTo,
    );
    d.picks = w
        .entities
        .iter()
        .filter(|e| Some(e.id) != focus)
        .cloned()
        .collect();
    d.body = vec![format!(
        "The shortest path from the focus, up to {MAX_HOPS} hops."
    )];
    s.overlay = Some(Overlay::Dialog(d));
}

/// Show the path from the focus to `target`.
pub fn path_to(w: &mut Workspace, target: EntitySummary, fx: &mut Vec<Effect>) {
    let from = w
        .focus
        .and_then(|f| w.crumbs.iter().rev().find(|c| c.id == f).cloned());
    w.tab = Tab::Path;
    w.pane = Pane::Graph;
    w.path_tab.target = Some(target);
    w.path_tab.from = from;
    w.path_tab.view = None;
    w.path_tab.cursor = 0;
    fx.extend(queries(w));
}

/// Keys on the timeline and path tabs; `false` when it isn't theirs.
pub fn act(w: &mut Workspace, a: Action, fx: &mut Vec<Effect>) -> bool {
    if w.pane != Pane::Graph {
        return false;
    }
    let (len, cur) = match w.tab {
        Tab::Timeline => (w.timeline.visible().len(), w.timeline.cursor),
        Tab::Path => (
            w.path_tab
                .view
                .as_ref()
                .and_then(|v| v.as_ref())
                .map_or(0, |v| v.steps.len()),
            w.path_tab.cursor,
        ),
        Tab::Ego => return false,
    };
    let last = len.saturating_sub(1);
    let next = match a {
        Action::Down => Some((cur + 1).min(last)),
        Action::Up => Some(cur.saturating_sub(1)),
        Action::HalfDown => Some((cur + 10).min(last)),
        Action::HalfUp => Some(cur.saturating_sub(10)),
        Action::Top => Some(0),
        Action::Bottom => Some(last),
        _ => None,
    };
    if let Some(n) = next {
        match w.tab {
            Tab::Timeline => w.timeline.cursor = n,
            _ => w.path_tab.cursor = n,
        }
        sync_inspect(w, fx);
        return true;
    }
    match a {
        Action::Activate => {
            if let Some(e) = w.cursor_entity().cloned() {
                w.tab = Tab::Ego;
                pivot(w, e, true, fx);
                sync_inspect(w, fx);
            }
        }
        Action::EventFilter if w.tab == Tab::Timeline => {
            let kinds = w.timeline.kinds();
            w.timeline.kind = match &w.timeline.kind {
                None => kinds.first().cloned(),
                Some(k) => kinds
                    .iter()
                    .position(|x| x == k)
                    .and_then(|i| kinds.get(i + 1))
                    .cloned(),
            };
            w.timeline.cursor = 0;
            sync_inspect(w, fx);
        }
        Action::Left | Action::Right | Action::PrevPage | Action::NextPage => {}
        _ => return false,
    }
    true
}

pub fn result(w: &mut Workspace, tag: &Tag, r: QueryResult, fx: &mut Vec<Effect>) -> bool {
    match (tag, r) {
        (Tag::Timeline, QueryResult::Timeline(mut ev)) => {
            ev.sort_by_key(|e| e.at);
            // One row per (entity, attribute, time), whichever sources
            // recorded it.
            let mut seen = std::collections::BTreeSet::new();
            ev.retain(|e| seen.insert((e.entity.id, e.attr.clone(), e.at)));
            let keep = w.timeline_entity().map(|e| e.id);
            w.timeline.events = Some(ev);
            if let Some(id) = keep
                && let Some(i) = w.timeline.visible().iter().position(|e| e.entity.id == id)
            {
                w.timeline.cursor = i;
            }
            w.timeline.cursor = w
                .timeline
                .cursor
                .min(w.timeline.visible().len().saturating_sub(1));
            if w.tab == Tab::Timeline {
                sync_inspect(w, fx);
            }
        }
        (Tag::Path(from, to), QueryResult::Path(p)) => {
            let current = w.path_tab.from.as_ref().map(|e| e.id) == Some(*from)
                && w.path_tab.target.as_ref().map(|e| e.id) == Some(*to);
            if current {
                w.path_tab.view = Some(p);
                let n = w
                    .path_tab
                    .view
                    .as_ref()
                    .and_then(|v| v.as_ref())
                    .map_or(0, |v| v.steps.len());
                w.path_tab.cursor = w.path_tab.cursor.min(n.saturating_sub(1));
                if w.tab == Tab::Path {
                    sync_inspect(w, fx);
                }
            }
        }
        _ => return false,
    }
    true
}
