//! The Checklist and Review screens (TUI_SPEC §5): their queries, keys and
//! results. Rendering is in `view::checklist` and `view::review`.

use spektr_engine::{EngineAction, ItemState, Query, QueryResult, ReviewState};
use spektr_model::EntityId;

use crate::keymap::Action;
use crate::msg::{Effect, Tag};
use crate::palette::NOT_YET;
use crate::state::{
    AppState, ChecklistPane, ChecklistScreen, Dialog, Field, ItemFilter, Menu, MenuAct, MenuItem,
    Overlay, Purpose, ReviewFilter, Tab, View, Workspace, serde_json_id,
};
use crate::theme::Token;
use crate::update::{pivot, sync_inspect, toast};

/// The queries the open screen needs.
pub fn queries(w: &Workspace) -> Vec<Effect> {
    match w.view {
        View::Checklist => vec![
            Effect::Query(Tag::Runs, Query::Runs),
            Effect::Query(Tag::Playbooks, Query::Playbooks),
            Effect::Query(Tag::Checklist, Query::Checklist(None)),
        ],
        View::Review => vec![Effect::Query(Tag::Review, Query::Review(None))],
        View::Main => Vec::new(),
    }
}

pub fn open_checklist(
    w: &mut Workspace,
    want: Option<spektr_model::ChecklistItemId>,
    fx: &mut Vec<Effect>,
) {
    w.view = View::Checklist;
    w.checklist.want_item = want;
    if want.is_some() {
        w.checklist.pane = ChecklistPane::Items;
        w.checklist.filter = ItemFilter::All;
    }
    fx.extend(queries(w));
}

pub fn open_review(w: &mut Workspace, only: Option<EntityId>, fx: &mut Vec<Effect>) {
    w.view = View::Review;
    w.review.only = only;
    w.review.sel = 0;
    w.review.compare = None;
    fx.extend(queries(w));
}

/// Back to the panes.
fn close(w: &mut Workspace, fx: &mut Vec<Effect>) {
    w.view = View::Main;
    w.review.only = None;
    sync_inspect(w, fx);
}

/// Keys on a screen. Returns `false` when the action isn't the screen's,
/// so the workspace handles it (global keys).
pub fn act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) -> bool {
    let Some(w) = s.workspace_mut() else {
        return false;
    };
    match w.view {
        View::Checklist => checklist_act(s, a, fx),
        View::Review => review_act(s, a, fx),
        View::Main => false,
    }
}

fn step(len: usize, cur: usize, a: Action) -> Option<usize> {
    let last = len.saturating_sub(1);
    Some(match a {
        Action::Down => (cur + 1).min(last),
        Action::Up => cur.saturating_sub(1),
        Action::HalfDown => (cur + 10).min(last),
        Action::HalfUp => cur.saturating_sub(10),
        Action::Top => 0,
        Action::Bottom => last,
        _ => return None,
    })
}

fn cycle<T: Copy + PartialEq>(all: &[T], cur: T, fwd: bool) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0);
    let n = all.len();
    all[if fwd { (i + 1) % n } else { (i + n - 1) % n }]
}

// ---- checklist

fn checklist_act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) -> bool {
    let glyphs = s.glyphs;
    let Some(w) = s.workspace_mut() else {
        return false;
    };
    let c = &mut w.checklist;
    if let Some(n) = match c.pane {
        ChecklistPane::Runs => step(c.runs.len(), c.run_sel, a),
        ChecklistPane::Items => step(c.visible().len(), c.item_sel, a),
    } {
        match c.pane {
            ChecklistPane::Runs => c.run_sel = n,
            ChecklistPane::Items => c.item_sel = n,
        }
        return true;
    }
    match a {
        Action::Escape => close(w, fx),
        Action::SwitchPane | Action::NextPane | Action::PrevPane => {
            c.pane = match c.pane {
                ChecklistPane::Runs => ChecklistPane::Items,
                ChecklistPane::Items => ChecklistPane::Runs,
            };
        }
        Action::PrevPage | Action::NextPage => {
            c.filter = cycle(&ItemFilter::ALL, c.filter, a == Action::NextPage);
            c.item_sel = 0;
        }
        Action::Activate => match c.pane {
            ChecklistPane::Runs => {
                // Jump to the run's first item.
                if let Some(run) = c.selected_run().map(|r| r.id) {
                    c.pane = ChecklistPane::Items;
                    if let Some(i) = c.visible().iter().position(|i| i.run == run) {
                        c.item_sel = i;
                    }
                }
            }
            ChecklistPane::Items => {
                let Some(item) = c.selected_item().cloned() else {
                    return true;
                };
                match item.screen.as_deref() {
                    Some("review") => {
                        let only = item.entity.as_ref().map(|e| e.id);
                        open_review(w, only, fx);
                    }
                    Some(other) => toast(s, &format!("{other}: {NOT_YET}"), Token::FgMuted),
                    None => match item.entity {
                        Some(e) => {
                            w.view = View::Main;
                            w.tab = Tab::Ego;
                            pivot(w, e, true, fx);
                            sync_inspect(w, fx);
                        }
                        None => toast(s, "this item has no entity", Token::FgMuted),
                    },
                }
            }
        },
        Action::ItemDone | Action::ItemSkip | Action::ItemNote => {
            let Some(item) = c.selected_item().cloned() else {
                toast(s, "no item selected", Token::FgMuted);
                return true;
            };
            let (title, label, hint, value, purpose) = match a {
                Action::ItemDone => (
                    "Mark done",
                    "note",
                    "optional · what you found",
                    item.note.clone().unwrap_or_default(),
                    Purpose::ItemDone(item.id),
                ),
                Action::ItemSkip => (
                    "Skip",
                    "reason",
                    "why it doesn't apply",
                    String::new(),
                    Purpose::ItemSkip(item.id),
                ),
                _ => (
                    "Note",
                    "note",
                    "saved with the item and the audit log",
                    item.note.clone().unwrap_or_default(),
                    Purpose::ItemNote(item.id),
                ),
            };
            let mut d = Dialog::input(
                title,
                vec![Field::Text {
                    label: label.into(),
                    value,
                    hint: hint.into(),
                }],
                purpose,
            );
            d.body = vec![crate::text::truncate(&item.text, 60)];
            s.overlay = Some(Overlay::Dialog(d));
        }
        Action::ItemOpen => {
            let Some(item) = c.selected_item().cloned() else {
                toast(s, "no item selected", Token::FgMuted);
                return true;
            };
            let Some(entity) = item.entity.clone() else {
                toast(s, "this item has no entity", Token::FgMuted);
                return true;
            };
            let ok: Vec<_> = item
                .launchers
                .iter()
                .filter(|l| l.disabled.is_none())
                .collect();
            match ok.len() {
                0 => {
                    if let Some(key) = &item.local_image {
                        let sha = key.trim_start_matches("sha256:").to_string();
                        fx.push(Effect::OpenBlob {
                            sha,
                            name: entity.value.clone(),
                            reveal: true,
                        });
                        toast(
                            s,
                            "no source URL · revealed the file to upload yourself",
                            Token::FgMuted,
                        );
                    } else if item.launchers.is_empty() {
                        toast(s, "this item has no launchers", Token::FgMuted);
                    } else {
                        let why = item.launchers[0].disabled.clone().unwrap_or_default();
                        toast(s, &why, Token::FgMuted);
                    }
                }
                1 => fx.push(Effect::Act {
                    what: ok[0].name.clone(),
                    action: EngineAction::OpenLauncher {
                        launcher: ok[0].id.clone(),
                        entity: entity.id,
                        item: Some(item.id),
                    },
                }),
                _ => {
                    let items = item
                        .launchers
                        .iter()
                        .map(|l| MenuItem {
                            label: l.name.clone(),
                            right: l.disabled.clone().unwrap_or_default(),
                            act: MenuAct::Launch {
                                launcher: l.id.clone(),
                                item: Some(item.id),
                            },
                            disabled: l.disabled.clone(),
                            submenu: false,
                            rule: false,
                        })
                        .collect();
                    let anchor = (s.size.0.saturating_sub(40), (s.size.1 / 3).max(4));
                    s.overlay = Some(Overlay::Menu(Menu {
                        anchor,
                        subtitle: format!("open {} ", glyphs.m(crate::glyph::Marker::Collapsed)),
                        target: entity,
                        edge: None,
                        items,
                        sel: 0,
                        sub: None,
                    }));
                }
            }
        }
        Action::ItemFinding => toast(s, &format!("finding from item: {NOT_YET}"), Token::FgMuted),
        Action::PlaybookOnSeed => {
            let Some(seed) = c.selected_run().and_then(|r| r.seed.clone()) else {
                toast(
                    s,
                    "no run selected · press p on an entity in the graph",
                    Token::FgMuted,
                );
                return true;
            };
            let playbooks = c.playbooks.clone();
            playbook_menu(s, seed, &playbooks);
        }
        Action::StopRun => match c.selected_run() {
            Some(r) if r.state == "running" => {
                let name = r.playbook.clone();
                s.overlay = Some(Overlay::Dialog(Dialog::confirm(
                    "Stop playbook",
                    vec![
                        format!("Stop {name} and the runs it started?"),
                        "Queued jobs are cancelled; finished results are kept.".into(),
                    ],
                    true,
                    Purpose::StopRun,
                )));
            }
            Some(_) => toast(s, "that run isn't running", Token::FgMuted),
            None => toast(s, "no run selected", Token::FgMuted),
        },
        _ => return false,
    }
    true
}

/// `Run playbook ▸`: the playbooks whose seed matches the entity.
pub fn playbook_menu(
    s: &mut AppState,
    target: spektr_engine::EntitySummary,
    playbooks: &[spektr_engine::PlaybookInfo],
) {
    let profile = s
        .workspace()
        .map(|w| w.profile().to_string())
        .unwrap_or_default();
    let items: Vec<MenuItem> = playbooks
        .iter()
        .filter(|p| p.seed.iter().any(|t| t == target.ty.as_str()))
        .map(|p| {
            let disabled = if !p.errors.is_empty() {
                Some(format!("{} problems", p.errors.len()))
            } else if !p.requires_profile.is_empty() && p.requires_profile != profile {
                Some(format!("needs {}", p.requires_profile))
            } else {
                None
            };
            MenuItem {
                label: p.name.clone(),
                right: disabled.clone().unwrap_or_else(|| {
                    if p.source == "built-in" {
                        "built-in".into()
                    } else {
                        "user".into()
                    }
                }),
                act: MenuAct::RunPlaybook(p.name.clone()),
                disabled,
                submenu: false,
                rule: false,
            }
        })
        .collect();
    if items.is_empty() {
        toast(
            s,
            &format!("no playbook starts from {}", target.ty.as_str()),
            Token::FgMuted,
        );
        return;
    }
    let anchor = (s.size.0 / 3, (s.size.1 / 3).max(4));
    s.overlay = Some(Overlay::Menu(Menu {
        anchor,
        subtitle: "run playbook".into(),
        target,
        edge: None,
        items,
        sel: 0,
        sub: None,
    }));
}

// ---- review

fn review_act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) -> bool {
    let Some(w) = s.workspace_mut() else {
        return false;
    };
    let r = &mut w.review;
    if let Some(n) = step(r.visible().len(), r.sel, a) {
        if n != r.sel {
            r.sel = n;
            compare(w, fx);
        }
        return true;
    }
    match a {
        Action::Escape => close(w, fx),
        Action::NextPane | Action::PrevPane | Action::PrevPage | Action::NextPage => {
            let fwd = matches!(a, Action::NextPane | Action::NextPage);
            r.filter = cycle(&ReviewFilter::ALL, r.filter, fwd);
            r.sel = 0;
            compare(w, fx);
        }
        Action::ReviewAccept | Action::ReviewReject | Action::ReviewLater => {
            let Some(item) = r.selected().cloned() else {
                toast(s, "nothing to review", Token::FgMuted);
                return true;
            };
            let decision = match a {
                Action::ReviewAccept => ReviewState::Accepted,
                Action::ReviewReject => ReviewState::Rejected,
                _ => ReviewState::Later,
            };
            if decision == ReviewState::Accepted && item.kind == "same_person" {
                let (Some(x), Some(y)) = (
                    serde_json_id(&item.payload["a"]),
                    serde_json_id(&item.payload["b"]),
                ) else {
                    toast(s, "the item names no entities", Token::Hostile);
                    return true;
                };
                let name = |k: &str| item.payload[k].as_str().unwrap_or("?").to_string();
                s.overlay = Some(Overlay::Dialog(Dialog::confirm(
                    "Merge",
                    vec![
                        format!("Merge {} into {}?", name("b_value"), name("a_value")),
                        "Both source records are kept; the merge can be undone.".into(),
                    ],
                    true,
                    Purpose::ReviewMerge {
                        id: item.id,
                        a: x,
                        b: y,
                    },
                )));
                return true;
            }
            fx.push(Effect::Act {
                what: format!("review {}", decision.as_str()),
                action: EngineAction::DecideReview {
                    id: item.id,
                    decision,
                },
            });
        }
        Action::OpenUrl => match r
            .selected()
            .and_then(|i| i.payload["detail"]["url"].as_str())
        {
            Some(u) => {
                let u = u.to_string();
                toast(s, &format!("opening {u}"), Token::FgMuted);
                fx.push(Effect::OpenUrl(u));
            }
            None => toast(s, "no URL to open", Token::FgMuted),
        },
        _ => return false,
    }
    true
}

/// Load both sides of a selected `same_person` item.
pub fn compare(w: &mut Workspace, fx: &mut Vec<Effect>) {
    let Some(item) = w.review.selected().cloned() else {
        w.review.compare = None;
        return;
    };
    if item.kind != "same_person" {
        w.review.compare = None;
        return;
    }
    if w.review.compare.as_ref().is_some_and(|c| c.0 == item.id) {
        return;
    }
    w.review.compare = Some((item.id, None, None));
    for (k, side) in [("a", true), ("b", false)] {
        if let Some(id) = serde_json_id(&item.payload[k]) {
            fx.push(Effect::Query(
                Tag::ReviewSide(item.id, side),
                Query::Detail(id),
            ));
        }
    }
}

/// Results for the screens; `false` when the tag isn't theirs.
pub fn result(w: &mut Workspace, tag: &Tag, r: QueryResult, fx: &mut Vec<Effect>) -> bool {
    match (tag, r) {
        (Tag::Runs, QueryResult::Runs(runs)) => {
            let c = &mut w.checklist;
            let keep = c.selected_run().map(|r| r.id);
            c.runs = ChecklistScreen::order(runs);
            c.run_sel = keep
                .and_then(|id| c.runs.iter().position(|r| r.id == id))
                .unwrap_or(0);
        }
        (Tag::Playbooks, QueryResult::Playbooks(p)) => w.checklist.playbooks = p,
        (Tag::Checklist, QueryResult::Checklist(mut items)) => {
            let c = &mut w.checklist;
            // Grouped by run (newest first) then step, as the screen lists them.
            let order: Vec<_> = c.runs.iter().map(|r| r.id).collect();
            items.sort_by_key(|i| {
                (
                    order.iter().position(|r| *r == i.run).unwrap_or(usize::MAX),
                    i.step,
                )
            });
            let keep = c.selected_item().map(|i| i.id);
            c.items = items;
            c.loaded = true;
            let want = c.want_item.take().or(keep);
            c.item_sel = want
                .and_then(|id| c.visible().iter().position(|i| i.id == id))
                .unwrap_or(0)
                .min(c.visible().len().saturating_sub(1));
        }
        (Tag::Review, QueryResult::Review(items)) => {
            let rv = &mut w.review;
            let keep = rv.selected().map(|i| i.id);
            rv.items = items;
            rv.loaded = true;
            rv.sel = keep
                .and_then(|id| rv.visible().iter().position(|i| i.id == id))
                .unwrap_or(rv.sel)
                .min(rv.visible().len().saturating_sub(1));
            compare(w, fx);
        }
        (Tag::ReviewSide(id, side), QueryResult::Detail(d)) => {
            if let Some((cur, a, b)) = &mut w.review.compare
                && cur == id
            {
                *(if *side { a } else { b }) = d.map(|b| *b);
            }
        }
        _ => return false,
    }
    true
}

/// Resolve a checklist dialog; `false` when the purpose isn't ours.
pub fn dialog_ok(p: &Purpose, value: String, fx: &mut Vec<Effect>) -> bool {
    let note = Some(value.clone()).filter(|v| !v.is_empty());
    match p {
        Purpose::ItemDone(item) => fx.push(Effect::Act {
            what: "done".into(),
            action: EngineAction::ResolveItem {
                item: *item,
                state: ItemState::Done,
                note,
            },
        }),
        Purpose::ItemSkip(item) => fx.push(Effect::Act {
            what: "skipped".into(),
            action: EngineAction::ResolveItem {
                item: *item,
                state: ItemState::Skipped,
                note,
            },
        }),
        Purpose::ItemNote(item) => fx.push(Effect::Act {
            what: "note".into(),
            action: EngineAction::ItemNote {
                item: *item,
                note: value,
            },
        }),
        Purpose::StopRun => fx.push(Effect::Act {
            what: "stop playbook".into(),
            action: EngineAction::StopPlaybook,
        }),
        Purpose::ReviewMerge { id, a, b } => {
            fx.push(Effect::Act {
                what: "merge".into(),
                action: EngineAction::Merge {
                    survivor: *a,
                    merged: *b,
                },
            });
            fx.push(Effect::Act {
                what: "review accepted".into(),
                action: EngineAction::DecideReview {
                    id: *id,
                    decision: ReviewState::Accepted,
                },
            });
        }
        _ => return false,
    }
    true
}
