//! The Findings screen (`F`, artboard 09), the inline editor (`e`, mode
//! INSERT), `$EDITOR` editing (`E`), new findings (from an entity, a
//! checklist item or the screen), attaching, and exports. Rendering is in
//! `view::findings`.

use spektr_engine::export::Format;
use spektr_engine::findings::{FindingDetail, FindingEdit};
use spektr_engine::{EngineAction, EntitySummary, FindingStatus, Query, QueryResult};
use spektr_model::EntityId;

use crate::keymap::Action;
use crate::msg::{Effect, Tag};
use crate::state::{
    AppState, Dialog, Editor, Field, FindingFilter, FindingRowItem, FindingsPane, Menu, MenuAct,
    MenuItem, Overlay, Purpose, Tab, View, Workspace,
};
use crate::theme::Token;
use crate::update::{pivot, sync_inspect, toast};

pub fn queries(w: &Workspace) -> Vec<Effect> {
    let mut fx = vec![Effect::Query(Tag::Findings, Query::Findings)];
    if w.view == View::Findings
        && let Some(id) = w.findings.selected_finding()
    {
        fx.push(Effect::Query(Tag::Finding(id), Query::Finding(id)));
    }
    fx
}

pub fn open(w: &mut Workspace, want: Option<i64>, fx: &mut Vec<Effect>) {
    w.view = View::Findings;
    if want.is_some() {
        w.findings.want = want;
        w.findings.pane = FindingsPane::List;
    }
    fx.extend(queries(w));
}

fn close(w: &mut Workspace, fx: &mut Vec<Effect>) {
    w.view = View::Main;
    sync_inspect(w, fx);
}

/// Load the selected finding's detail.
fn select(w: &mut Workspace, fx: &mut Vec<Effect>) {
    w.findings.dsel = 0;
    match w.findings.selected_finding() {
        Some(id) => {
            w.findings.current = Some(id);
            if w.findings.detail.as_ref().is_none_or(|d| d.id != id) {
                fx.push(Effect::Query(Tag::Finding(id), Query::Finding(id)));
            }
        }
        None => w.findings.detail = None,
    }
}

fn detail_rows(d: &FindingDetail) -> usize {
    d.entities.len() + d.evidence.len()
}

/// `n`, `f` on an entity, `f` on a checklist item: category and claim.
pub fn new_dialog(
    s: &mut AppState,
    entities: Vec<EntityId>,
    relevance: String,
    about: Option<String>,
) {
    let Some(w) = s.workspace() else { return };
    let cats: Vec<(String, String)> = w
        .findings
        .view
        .as_ref()
        .map(|v| {
            v.categories
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    if cats.is_empty() {
        toast(s, "loading the rules profile's categories…", Token::FgMuted);
        return;
    }
    let mut d = Dialog::input(
        "New finding",
        vec![
            Field::Choice {
                label: "category".into(),
                options: cats.iter().map(|c| c.1.clone()).collect(),
                idx: 0,
            },
            Field::Text {
                label: "claim".into(),
                value: String::new(),
                hint: "what you found, in one sentence".into(),
            },
        ],
        Purpose::NewFinding {
            entities,
            relevance,
        },
    );
    d.body = vec![match about {
        Some(a) => format!("From {a}. ← → picks the category; e edits the rest later."),
        None => "← → picks the category; e edits the rest later.".into(),
    }];
    s.overlay = Some(Overlay::Dialog(d));
}

/// The category id for a dialog choice index.
fn category_id(w: &Workspace, idx: usize) -> Option<String> {
    w.findings
        .view
        .as_ref()
        .and_then(|v| v.categories.get(idx))
        .map(|c| c.id.clone())
}

/// `a` on an entity: add it to the current finding, or pick one.
pub fn attach_entity(s: &mut AppState, entity: &EntitySummary, fx: &mut Vec<Effect>) {
    let Some(w) = s.workspace() else { return };
    if let Some(id) = w.findings.current {
        fx.push(Effect::Act {
            what: format!("attached to F-{id}"),
            action: EngineAction::FindingEntity {
                id,
                entity: entity.id,
                attach: true,
            },
        });
        return;
    }
    let list: Vec<(i64, String)> = w
        .findings
        .view
        .as_ref()
        .map(|v| {
            v.findings
                .iter()
                .map(|f| (f.id, format!("F-{} {}", f.id, f.title)))
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        toast(
            s,
            "no findings yet · f creates one from this entity",
            Token::FgMuted,
        );
        return;
    }
    let mut d = Dialog::input(
        "Attach to finding",
        vec![Field::Choice {
            label: "finding".into(),
            options: list.iter().map(|f| f.1.clone()).collect(),
            idx: 0,
        }],
        Purpose::AttachTo(entity.id),
    );
    d.body = vec![format!("Add {} as a supporting entity.", entity.value)];
    s.overlay = Some(Overlay::Dialog(d));
}

/// Keys on the Findings screen; `false` when the action isn't the screen's.
pub fn act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) -> bool {
    let Some(w) = s.workspace_mut() else {
        return false;
    };
    let f = &mut w.findings;
    let step = |len: usize, cur: usize| -> Option<usize> {
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
    };
    match f.pane {
        FindingsPane::List => {
            if let Some(n) = step(f.rows().len(), f.sel) {
                if n != f.sel {
                    f.sel = n;
                    select(w, fx);
                }
                return true;
            }
        }
        FindingsPane::Detail => {
            let len = f.detail.as_ref().map_or(0, detail_rows);
            if let Some(n) = step(len, f.dsel) {
                f.dsel = n;
                return true;
            }
        }
    }
    let current = f.selected_finding();
    match a {
        Action::Escape => match f.pane {
            FindingsPane::Detail => f.pane = FindingsPane::List,
            FindingsPane::List => close(w, fx),
        },
        Action::NextPane | Action::PrevPane => {
            f.pane = match f.pane {
                FindingsPane::List if f.detail.is_some() => FindingsPane::Detail,
                _ => FindingsPane::List,
            };
        }
        Action::PrevPage | Action::NextPage => {
            let all = FindingFilter::ALL;
            let i = all.iter().position(|x| *x == f.filter).unwrap_or(0);
            let n = all.len();
            f.filter = all[if a == Action::NextPage {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            }];
            f.sel = 0;
            select(w, fx);
        }
        Action::Activate => match f.pane {
            FindingsPane::List => match f.selected_row() {
                Some(FindingRowItem::Category { id, open }) => {
                    f.open.insert(id, !open);
                }
                Some(FindingRowItem::Finding(_)) if f.detail.is_some() => {
                    f.pane = FindingsPane::Detail
                }
                _ => {}
            },
            FindingsPane::Detail => {
                let Some(d) = f.detail.clone() else {
                    return true;
                };
                if let Some((e, _)) = d.entities.get(f.dsel) {
                    w.view = View::Main;
                    w.tab = Tab::Ego;
                    pivot(w, e.clone(), true, fx);
                    sync_inspect(w, fx);
                } else if let Some(ev) = d.evidence.get(f.dsel - d.entities.len()) {
                    fx.push(Effect::OpenEvidence(ev.observation.id));
                }
            }
        },
        Action::FindingNew => new_dialog(s, Vec::new(), String::new(), None),
        Action::FindingEdit => match f.detail.as_ref().filter(|d| Some(d.id) == current) {
            Some(d) => {
                let ed = Editor::new(d.id, &d.title, &d.claim, &d.relevance);
                s.overlay = Some(Overlay::Editor(Box::new(ed)));
            }
            None => toast(s, "select a finding", Token::FgMuted),
        },
        Action::FindingEditExternal => match f.detail.as_ref().filter(|d| Some(d.id) == current) {
            Some(d) => fx.push(Effect::EditExternal(d.id, external_text(d))),
            None => toast(s, "select a finding", Token::FgMuted),
        },
        Action::FindingAttach => match current {
            Some(id) => fx.push(Effect::Query(
                Tag::Candidates(id),
                Query::FindingCandidates(id),
            )),
            None => toast(s, "select a finding", Token::FgMuted),
        },
        Action::FindingReady | Action::FindingSent => match current {
            Some(id) => {
                let (status, what) = if a == Action::FindingReady {
                    (FindingStatus::Ready, "ready")
                } else {
                    (FindingStatus::Sent, "sent")
                };
                fx.push(Effect::Act {
                    what: format!("F-{id} {what}"),
                    action: EngineAction::SetFindingStatus { id, status },
                });
            }
            None => toast(s, "select a finding", Token::FgMuted),
        },
        Action::FindingDelete => match current {
            Some(id) => {
                s.overlay = Some(Overlay::Dialog(Dialog::confirm(
                    "Delete finding",
                    vec![
                        format!("Delete F-{id}? Its entities and evidence stay in the case."),
                        "The audit log keeps the record. Press y to delete.".into(),
                    ],
                    true,
                    Purpose::DeleteFinding(id),
                )));
            }
            None => toast(s, "select a finding", Token::FgMuted),
        },
        Action::FindingExport => {
            let ids: Vec<i64> = current.into_iter().collect();
            let what = if ids.is_empty() {
                "all findings".to_string()
            } else {
                format!("F-{}", ids[0])
            };
            let item = |label: &str, format: Format, right: &str| MenuItem {
                label: label.into(),
                right: right.into(),
                act: MenuAct::Export {
                    format,
                    findings: ids.clone(),
                },
                disabled: None,
                submenu: false,
                rule: false,
            };
            let target = EntitySummary {
                id: EntityId::from_bytes([0; 16]),
                ty: spektr_model::EntityType::Core(spektr_model::CoreType::Url),
                key: String::new(),
                value: format!("export {what}"),
                flags: spektr_model::EntityFlags::NONE,
                depth: 0,
            };
            s.overlay = Some(Overlay::Menu(Menu {
                anchor: (s.size.0.saturating_sub(48), s.size.1.saturating_sub(10)),
                target,
                edge: None,
                subtitle: "export".into(),
                items: vec![
                    item("Submission text", Format::Tracelabs, "clipboard"),
                    item("Markdown report", Format::Md, "file"),
                    item("JSON", Format::Json, "file"),
                ],
                sel: 0,
                sub: None,
            }));
        }
        _ => return false,
    }
    true
}

/// The text `E` hands to `$EDITOR`.
pub fn external_text(d: &FindingDetail) -> String {
    format!(
        "# F-{} · edit the title, claim and relevance; lines starting with # are ignored.\n\
         # Saving an unchanged file changes nothing.\n\
         title: {}\n\n[claim]\n{}\n\n[relevance]\n{}\n",
        d.id,
        d.title,
        d.claim.trim(),
        d.relevance.trim()
    )
}

/// Parse `$EDITOR`'s text back into an edit.
pub fn parse_external(text: &str) -> FindingEdit {
    let mut title = None;
    let mut section = None::<&str>;
    let (mut claim, mut rel) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim_start().starts_with('#') {
            continue;
        }
        if let Some(v) = t.strip_prefix("title:") {
            title = Some(v.trim().to_string());
            continue;
        }
        match t.trim() {
            "[claim]" => section = Some("claim"),
            "[relevance]" => section = Some("relevance"),
            _ => match section {
                Some("claim") => claim.push(t.to_string()),
                Some("relevance") => rel.push(t.to_string()),
                _ => {}
            },
        }
    }
    let join = |v: Vec<String>| v.join("\n").trim().to_string();
    FindingEdit {
        title,
        category: None,
        claim: Some(join(claim)),
        relevance: Some(join(rel)),
    }
}

/// Keys in the inline editor: ↵ saves, esc cancels, ↑ ↓ tab move between
/// fields, ← → move the cursor; typing goes to the field.
pub fn editor_act(s: &mut AppState, mut ed: Box<Editor>, a: Action, fx: &mut Vec<Effect>) {
    use ratatui_textarea::CursorMove;
    let n = ed.fields.len();
    match a {
        Action::Escape => return,
        Action::Select => {
            let edit = FindingEdit {
                title: Some(ed.value(0)),
                category: None,
                claim: Some(ed.value(1)),
                relevance: Some(ed.value(2)),
            };
            fx.push(Effect::Act {
                what: format!("F-{} saved", ed.id),
                action: EngineAction::EditFinding { id: ed.id, edit },
            });
            return;
        }
        Action::SelectDown | Action::Complete => ed.focus = (ed.focus + 1) % n,
        Action::SelectUp => ed.focus = (ed.focus + n - 1) % n,
        Action::SubmenuBack => ed.fields[ed.focus].move_cursor(CursorMove::Back),
        Action::SubmenuOpen => ed.fields[ed.focus].move_cursor(CursorMove::Forward),
        _ => {}
    }
    s.overlay = Some(Overlay::Editor(ed));
}

/// Typing into the editor.
pub fn editor_char(ed: &mut Editor, c: char) {
    if let Some(f) = ed.fields.get_mut(ed.focus) {
        f.insert_char(c);
    }
}

pub fn editor_backspace(ed: &mut Editor) {
    if let Some(f) = ed.fields.get_mut(ed.focus) {
        f.delete_char();
    }
}

/// Results for the screen; `false` when the tag isn't its.
pub fn result(s: &mut AppState, tag: &Tag, r: QueryResult, fx: &mut Vec<Effect>) -> bool {
    let Some(w) = s.workspace_mut() else {
        return false;
    };
    match (tag, r) {
        (Tag::Findings, QueryResult::Findings(v)) => {
            let f = &mut w.findings;
            let keep = f.want.take().or_else(|| f.selected_finding());
            f.view = Some(v);
            if let Some(id) = keep {
                if let Some(cat) = f
                    .view
                    .as_ref()
                    .and_then(|v| v.findings.iter().find(|x| x.id == id))
                    .map(|x| x.category.clone())
                {
                    f.open.insert(cat, true);
                }
                if let Some(i) = f
                    .rows()
                    .iter()
                    .position(|r| *r == FindingRowItem::Finding(id))
                {
                    f.sel = i;
                }
            } else if f.sel == 0
                && let Some(i) = f
                    .rows()
                    .iter()
                    .position(|r| matches!(r, FindingRowItem::Finding(_)))
            {
                // Open on the first finding, not a category.
                f.sel = i;
            }
            f.sel = f.sel.min(f.rows().len().saturating_sub(1));
            if w.view == View::Findings {
                let before = w.findings.detail.as_ref().map(|d| d.id);
                select(w, fx);
                // A refresh re-reads the open detail too.
                if let Some(id) = before
                    && before == w.findings.selected_finding()
                {
                    fx.push(Effect::Query(Tag::Finding(id), Query::Finding(id)));
                }
            }
        }
        (Tag::Finding(id), QueryResult::Finding(d)) => {
            if w.findings.selected_finding() == Some(*id) || w.findings.want == Some(*id) {
                w.findings.detail = d.map(|b| *b);
                let n = w.findings.detail.as_ref().map_or(0, detail_rows);
                w.findings.dsel = w.findings.dsel.min(n.saturating_sub(1));
            }
        }
        (Tag::Candidates(id), QueryResult::Observations(obs)) => {
            if obs.is_empty() {
                toast(
                    s,
                    "nothing more to attach · add supporting entities with a in the graph",
                    Token::FgMuted,
                );
                return true;
            }
            let label = |o: &spektr_engine::ObservationView| {
                format!(
                    "{} {} · {} · {}",
                    o.module,
                    o.module_version,
                    o.source_url.as_deref().unwrap_or(&o.kind),
                    o.created_at.strftime("%H:%M")
                )
            };
            let d = Dialog::input(
                "Attach evidence",
                vec![Field::Choice {
                    label: "evidence".into(),
                    options: obs.iter().map(label).collect(),
                    idx: 0,
                }],
                Purpose::AttachEvidence(*id, obs.iter().map(|o| o.id).collect()),
            );
            s.overlay = Some(Overlay::Dialog(d));
        }
        (Tag::Export(path), QueryResult::Export(text)) => match path {
            None => {
                let n = text.matches("Category:").count();
                fx.push(Effect::Copy(text));
                toast(
                    s,
                    &format!("copied {n} submission{}", if n == 1 { "" } else { "s" }),
                    Token::Ok,
                );
            }
            Some(p) => fx.push(Effect::WriteFile(p.clone(), text)),
        },
        _ => return false,
    }
    true
}

/// The export menu's choice.
pub fn export(s: &mut AppState, format: Format, findings: Vec<i64>, fx: &mut Vec<Effect>) {
    if format == Format::Tracelabs {
        fx.push(Effect::Query(
            Tag::Export(None),
            Query::Export { format, findings },
        ));
        return;
    }
    let Some(w) = s.workspace() else { return };
    let stem = w
        .path
        .file_stem()
        .map_or_else(|| "case".into(), |s| s.to_string_lossy().into_owned());
    let dir = w
        .path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let file = dir.join(format!("{stem}-findings.{}", format.extension()));
    let d = Dialog::input(
        "Export",
        vec![Field::Text {
            label: "file".into(),
            value: crate::text::tilde(&file, s.settings.home.as_deref()),
            hint: "where to write it".into(),
        }],
        Purpose::ExportTo { format, findings },
    );
    s.overlay = Some(Overlay::Dialog(d));
}

/// Resolve a findings dialog; `false` when the purpose isn't ours.
pub fn dialog_ok(s: &mut AppState, d: &Dialog, fx: &mut Vec<Effect>) -> Result<bool, String> {
    let choice = |i: usize| match d.fields.get(i) {
        Some(Field::Choice { idx, .. }) => *idx,
        _ => 0,
    };
    let text = |i: usize| {
        d.fields
            .get(i)
            .map(Field::value)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let Some(w) = s.workspace() else {
        return Ok(false);
    };
    match &d.purpose {
        Purpose::NewFinding {
            entities,
            relevance,
        } => {
            let category = category_id(w, choice(0)).ok_or("no category")?;
            fx.push(Effect::Act {
                what: "finding".into(),
                action: EngineAction::CreateFinding {
                    category,
                    claim: text(1),
                    relevance: relevance.clone(),
                    title: None,
                    entities: entities.clone(),
                    evidence: Vec::new(),
                },
            });
        }
        Purpose::AttachTo(entity) => {
            let id = w
                .findings
                .view
                .as_ref()
                .and_then(|v| v.findings.get(choice(0)))
                .map(|f| f.id)
                .ok_or("no finding")?;
            fx.push(Effect::Act {
                what: format!("attached to F-{id}"),
                action: EngineAction::FindingEntity {
                    id,
                    entity: *entity,
                    attach: true,
                },
            });
            if let Some(w) = s.workspace_mut() {
                w.findings.current = Some(id);
            }
        }
        Purpose::AttachEvidence(id, obs) => {
            let o = *obs.get(choice(0)).ok_or("nothing chosen")?;
            fx.push(Effect::Act {
                what: format!("evidence added to F-{id}"),
                action: EngineAction::FindingEvidence {
                    id: *id,
                    observation: o,
                    attach: true,
                },
            });
        }
        Purpose::DeleteFinding(id) => fx.push(Effect::Act {
            what: format!("F-{id} deleted"),
            action: EngineAction::DeleteFinding(*id),
        }),
        Purpose::ExportTo { format, findings } => {
            let p = text(0);
            if p.is_empty() {
                return Err("enter a file path".into());
            }
            let path = crate::update::expand_home(&p, s.settings.home.as_deref());
            fx.push(Effect::Query(
                Tag::Export(Some(path)),
                Query::Export {
                    format: *format,
                    findings: findings.clone(),
                },
            ));
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_round_trip() {
        let d = FindingDetail {
            id: 3,
            title: "GitHub activity after last seen".into(),
            category: "timeline".into(),
            claim: "Active on 2026-10-16.".into(),
            relevance: "After the last-seen date.\nSecond line.".into(),
            status: "draft".into(),
            created_at: "2026-10-20T18:00:00Z".parse().unwrap(),
            updated_at: "2026-10-20T18:00:00Z".parse().unwrap(),
            entities: Vec::new(),
            evidence: Vec::new(),
            checks: Vec::new(),
        };
        let e = parse_external(&external_text(&d));
        assert_eq!(e.title.as_deref(), Some("GitHub activity after last seen"));
        assert_eq!(e.claim.as_deref(), Some("Active on 2026-10-16."));
        assert_eq!(
            e.relevance.as_deref(),
            Some("After the last-seen date.\nSecond line.")
        );
    }
}
