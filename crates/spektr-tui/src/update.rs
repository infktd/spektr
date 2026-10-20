//! `update(&mut AppState, Msg) -> Vec<Effect>`: the only code that changes
//! state (ARCHITECTURE §3). Keys and mouse both resolve to registry
//! actions, so the two can't drift apart (TUI_SPEC §1).

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use spektr_engine::{
    ActionOutcome, EngineAction, EngineEvent, EntitySummary, EventKind, JobState, Query,
    QueryResult,
};
use spektr_model::{CoreType, EntityFlags};

use crate::ego::{self, Col, Item};
use crate::entities::{self, selectable};
use crate::glyph::{Marker, display_value};
use crate::host::Chain;
use crate::keymap::{Action, Ctx, KeyCodeOrd, KeyPress, Resolved};
use crate::layout;
use crate::menu;
use crate::msg::{Effect, Msg, Tag};
use crate::palette::{self, Cmd, NOT_YET};
use crate::state::{
    ACTIVITY_CAP, ActRow, AppState, Dialog, EntItem, Field, FilterState, Help, Hit, Menu, MenuAct,
    Overlay, Palette, Pane, Purpose, Screen, Stale, Tab, View, Workspace,
};
use crate::theme::Token;
use crate::{screens, tabs};

/// Double-click window (TUI_SPEC §8).
const DOUBLE_CLICK_MS: u64 = 400;
/// Wheel rows per notch.
const WHEEL: usize = 3;

/// The effects to run when the app starts.
pub fn init(s: &mut AppState) -> Vec<Effect> {
    match &s.screen {
        Screen::Opening(p) => vec![Effect::Open(p.clone()), Effect::LoadProfiles],
        _ => vec![Effect::LoadRecent, Effect::LoadDoctor, Effect::LoadProfiles],
    }
}

pub fn update(s: &mut AppState, msg: Msg) -> Vec<Effect> {
    // Handlers clear `dirty` when a message changed nothing; a render
    // already pending from an earlier message must survive that.
    let pending = s.dirty;
    let fx = handle(s, msg);
    s.dirty |= pending;
    fx
}

fn handle(s: &mut AppState, msg: Msg) -> Vec<Effect> {
    let mut fx = Vec::new();
    s.dirty = true;
    match msg {
        Msg::Key(k) => key(s, k, &mut fx),
        Msg::Mouse(m, ms) => {
            // With capture off a terminal sends no mouse input; ignore any
            // that still arrives.
            if s.mouse {
                mouse(s, m, ms, &mut fx);
            } else {
                s.dirty = false;
            }
        }
        Msg::Paste(t) => {
            for c in t.chars().filter(|c| !c.is_control()) {
                type_char(s, c, &mut fx);
            }
        }
        Msg::Resize(w, h) => s.size = (w, h),
        Msg::Tick(now) => {
            let second = |t: jiff::Timestamp| t.as_second();
            s.dirty = second(now) != second(s.now);
            s.now = now;
            if s.toast.as_ref().is_some_and(|(_, _, until)| *until <= now) {
                s.toast = None;
                s.dirty = true;
            }
        }
        Msg::Refresh => refresh(s, &mut fx),
        Msg::Engine(ev) => engine_event(s, *ev, &mut fx),
        Msg::Lagged => {
            if let Some(w) = s.workspace_mut() {
                w.stale = Stale::all();
                schedule(w, &mut fx);
            }
        }
        Msg::Recent(r) => {
            if let Screen::Launcher(l) = &mut s.screen {
                l.cursor = l.cursor.min(r.len().saturating_sub(1));
                l.recent = Some(r);
            }
        }
        Msg::Doctor(d) => {
            if let Screen::Launcher(l) = &mut s.screen {
                l.doctor = Some(d);
            }
        }
        Msg::Profiles(p) => s.profiles = p,
        Msg::Verified(path, chain) => {
            let text = match &chain {
                Chain::Ok { rows } => (format!("chain ✓ {rows} rows"), Token::Ok),
                Chain::Broken { at } => (format!("chain ✕ break at {at}"), Token::Hostile),
                Chain::Missing(e) => (e.clone(), Token::Hostile),
                Chain::Unknown => (String::new(), Token::FgMuted),
            };
            if s.verify_requested.as_ref() == Some(&path) {
                s.verify_requested = None;
                toast(s, &text.0, text.1);
            }
            match &mut s.screen {
                Screen::Launcher(l) => {
                    if let Some(r) = l
                        .recent
                        .as_mut()
                        .and_then(|r| r.iter_mut().find(|c| c.path == path))
                    {
                        r.chain = chain;
                    }
                }
                Screen::Workspace(w) if w.path == path => w.chain = chain,
                _ => {}
            }
        }
        Msg::Opened(path, Ok(())) => {
            s.overlay = None;
            s.screen = Screen::Workspace(Box::new(Workspace::new(path.clone())));
            fx.extend([
                Effect::Verify(path),
                Effect::Query(Tag::Status, Query::Status),
                Effect::Query(Tag::Modules, Query::Modules),
                Effect::Query(Tag::Playbooks, Query::Playbooks),
                Effect::Query(Tag::Entities, Query::Entities),
                Effect::Query(
                    Tag::Jobs,
                    Query::Jobs(vec![JobState::Done, JobState::Failed, JobState::Blocked]),
                ),
            ]);
        }
        Msg::Opened(path, Err(e)) => {
            let mut l = crate::state::Launcher {
                error: Some(format!("{}: {e}", path.display())),
                ..Default::default()
            };
            if let Screen::Launcher(old) = std::mem::replace(&mut s.screen, Screen::Opening(path)) {
                l.recent = old.recent;
                l.doctor = old.doctor;
                l.cursor = old.cursor;
            }
            s.screen = Screen::Launcher(l);
            fx.extend([Effect::LoadRecent, Effect::LoadDoctor]);
        }
        Msg::Created(Ok(path)) => {
            s.overlay = None;
            s.screen = Screen::Opening(path.clone());
            fx.push(Effect::Open(path));
        }
        Msg::Created(Err(e)) => {
            if let Some(Overlay::Dialog(d)) = &mut s.overlay {
                d.error = Some(e);
            } else {
                toast(s, &e, Token::Hostile);
            }
        }
        Msg::Query(tag, r) => query_result(s, tag, r, &mut fx),
        Msg::Acted { what, result } => acted(s, &what, result, &mut fx),
        Msg::WouldOpen(target) => {
            let now = s.now;
            if let Some(w) = s.workspace_mut() {
                push_row(
                    w,
                    ActRow {
                        at: now,
                        marker: Marker::Link,
                        token: Token::Accent,
                        module: "open".into(),
                        module_token: Token::Fg,
                        spans: vec![(format!("would open {target}"), Token::FgMuted)],
                    },
                );
            }
        }
        Msg::Image(key, r) => {
            let slot = match r {
                Ok(ready) => crate::images::Slot::Ready(ready),
                Err(e) => crate::images::Slot::Failed(e),
            };
            s.images.insert(key, slot);
        }
    }
    fx
}

pub fn toast(s: &mut AppState, text: &str, t: Token) {
    if text.is_empty() {
        return;
    }
    let until = s
        .now
        .checked_add(jiff::SignedDuration::from_secs(4))
        .unwrap_or(s.now);
    s.toast = Some((text.to_string(), t, until));
}

// ---- keys

fn key(s: &mut AppState, ev: KeyEvent, fx: &mut Vec<Effect>) {
    let Some(kp) = KeyPress::from_event(&ev) else {
        s.dirty = false;
        return;
    };
    let ctxs = s.contexts();
    if s.keymap.resolve(&[kp], &[Ctx::Global]) == Resolved::Action(Action::ForceQuit) {
        if s.force_quit_armed {
            s.quit = true;
            fx.push(Effect::Quit);
        } else {
            s.force_quit_armed = true;
            toast(s, "press ctrl-c again to quit", Token::Warn);
        }
        return;
    }
    s.force_quit_armed = false;
    let mut seq = std::mem::take(&mut s.pending);
    seq.push(kp);
    match s.keymap.resolve(&seq, &ctxs) {
        Resolved::Action(a) => act(s, a, fx),
        Resolved::Pending => s.pending = seq,
        Resolved::None => {
            if seq.len() > 1
                && let Resolved::Action(a) = s.keymap.resolve(&[kp], &ctxs)
            {
                act(s, a, fx);
                return;
            }
            if ctxs.contains(&Ctx::Text) {
                match kp.code {
                    KeyCodeOrd::Backspace => backspace(s, fx),
                    _ => {
                        if let Some(c) = kp.printable() {
                            type_char(s, c, fx);
                        }
                    }
                }
            }
        }
    }
}

fn type_char(s: &mut AppState, c: char, fx: &mut Vec<Effect>) {
    match &mut s.overlay {
        Some(Overlay::Palette(p)) => {
            p.input.push(c);
            p.error = None;
            refilter_palette(s);
        }
        Some(Overlay::Help(h)) if h.searching => {
            h.search.push(c);
            h.scroll = 0;
        }
        Some(Overlay::Dialog(d)) => {
            if let Some(Field::Text { value, .. }) = d.fields.get_mut(d.focus) {
                value.push(c);
                d.error = None;
            }
            refilter_picks(s);
        }
        Some(_) => {}
        None => {
            if let Some(w) = s.workspace_mut()
                && let Some(f) = &mut w.filter
                && f.editing
            {
                f.text.push(c);
                rebuild_entities(w);
                first_row(w);
                sync_inspect(w, fx);
            }
        }
    }
}

fn backspace(s: &mut AppState, fx: &mut Vec<Effect>) {
    match &mut s.overlay {
        Some(Overlay::Palette(p)) => {
            p.input.pop();
            p.error = None;
            refilter_palette(s);
        }
        Some(Overlay::Help(h)) if h.searching => {
            h.search.pop();
        }
        Some(Overlay::Dialog(d)) => {
            if let Some(Field::Text { value, .. }) = d.fields.get_mut(d.focus) {
                value.pop();
            }
            refilter_picks(s);
        }
        Some(_) => {}
        None => {
            if let Some(w) = s.workspace_mut()
                && let Some(f) = &mut w.filter
                && f.editing
            {
                f.text.pop();
                rebuild_entities(w);
                first_row(w);
                sync_inspect(w, fx);
            }
        }
    }
}

/// Perform a registry action in the current context.
pub fn act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) {
    match s.overlay.take() {
        Some(Overlay::Palette(p)) => palette_act(s, p, a, fx),
        Some(Overlay::Menu(m)) => menu_act(s, m, a, fx),
        Some(Overlay::Help(h)) => help_act(s, h, a, fx),
        Some(Overlay::Dialog(d)) => dialog_act(s, d, a, fx),
        None => match &s.screen {
            Screen::Launcher(_) | Screen::Opening(_) => launcher_act(s, a, fx),
            Screen::Workspace(_) => workspace_act(s, a, fx),
        },
    }
}

// ---- launcher

fn launcher_act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) {
    let Screen::Launcher(l) = &mut s.screen else {
        if a == Action::Quit {
            quit(s, fx);
        }
        return;
    };
    let n = l.recent.as_ref().map_or(0, Vec::len);
    let selected = l
        .recent
        .as_ref()
        .and_then(|r| r.get(l.cursor))
        .map(|c| c.path.clone());
    match a {
        Action::Down => l.cursor = (l.cursor + 1).min(n.saturating_sub(1)),
        Action::Up => l.cursor = l.cursor.saturating_sub(1),
        Action::Top => l.cursor = 0,
        Action::Bottom => l.cursor = n.saturating_sub(1),
        Action::LauncherOpen | Action::Activate => {
            if let Some(p) = selected {
                s.screen = Screen::Opening(p.clone());
                fx.push(Effect::Open(p));
            }
        }
        Action::LauncherVerify => {
            if let Some(p) = selected {
                toast(s, "verifying…", Token::FgMuted);
                s.verify_requested = Some(p.clone());
                fx.push(Effect::Verify(p));
            }
        }
        Action::LauncherNew => {
            let profiles = if s.profiles.is_empty() {
                vec!["default".to_string(), "tracelabs".to_string()]
            } else {
                s.profiles.clone()
            };
            s.overlay = Some(Overlay::Dialog(Dialog::input(
                "New case",
                vec![
                    Field::Text {
                        label: "name".into(),
                        value: String::new(),
                        hint: "letters, digits, . _ -".into(),
                    },
                    Field::Choice {
                        label: "rules".into(),
                        options: profiles,
                        idx: 0,
                    },
                    Field::Text {
                        label: "subject".into(),
                        value: String::new(),
                        hint: "optional · a person's name".into(),
                    },
                    Field::Text {
                        label: "last seen".into(),
                        value: String::new(),
                        hint: "optional · YYYY-MM-DD".into(),
                    },
                ],
                Purpose::NewCase,
            )));
        }
        Action::LauncherOpenPath => {
            s.overlay = Some(Overlay::Dialog(Dialog::input(
                "Open case",
                vec![Field::Text {
                    label: "path".into(),
                    value: String::new(),
                    hint: "path to a .spektr file".into(),
                }],
                Purpose::OpenPath,
            )));
        }
        Action::Help => s.overlay = Some(Overlay::Help(Help::default())),
        Action::Quit => quit(s, fx),
        Action::Palette => toast(s, "open a case first", Token::FgMuted),
        _ => s.dirty = false,
    }
}

fn quit(s: &mut AppState, fx: &mut Vec<Effect>) {
    let busy = s.workspace().map_or(0, |w| w.running + w.queued);
    if busy > 0 {
        s.overlay = Some(Overlay::Dialog(Dialog::confirm(
            "Quit",
            vec![
                format!("{busy} jobs are running or queued."),
                "Quitting cancels them; finished results are already saved.".into(),
            ],
            false,
            Purpose::Quit,
        )));
    } else {
        s.quit = true;
        fx.push(Effect::Quit);
    }
}

// ---- workspace

fn visible_panes(s: &AppState) -> Vec<Pane> {
    let wide = layout::widths(s.size.0).1 > 0;
    let insp = wide || s.workspace().is_some_and(|w| w.insp_overlay);
    let mut v = vec![Pane::Entities, Pane::Graph];
    if insp {
        v.push(Pane::Inspector);
    }
    v
}

fn workspace_act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) {
    let panes = visible_panes(s);
    let size = s.size;
    let glyphs = s.glyphs;
    let Some(w) = s.workspace_mut() else { return };
    let editing = w.filter.as_ref().is_some_and(|f| f.editing);
    if editing {
        match a {
            Action::SelectDown => move_ent(w, 1),
            Action::SelectUp => move_ent(w, -1),
            Action::Select => {
                if let Some(f) = &mut w.filter {
                    f.editing = false;
                }
                w.pane = Pane::Entities;
            }
            Action::Escape => {
                w.filter = None;
                rebuild_entities(w);
            }
            Action::NextPane | Action::PrevPane => {
                if let Some(f) = &mut w.filter {
                    f.editing = false;
                }
                cycle_pane(w, &panes, a == Action::NextPane);
            }
            _ => {}
        }
        sync_inspect(w, fx);
        return;
    }
    if w.view != View::Main {
        if screens::act(s, a, fx) {
            return;
        }
    } else if w.tab != Tab::Ego && tabs::act(w, a, fx) {
        return;
    }
    let Some(w) = s.workspace_mut() else { return };
    let half = (layout::entity_rows(&layout::workspace(size.0, size.1)) / 2).max(1) as isize;
    match a {
        Action::Quit => return quit(s, fx),
        Action::Help => s.overlay = Some(Overlay::Help(Help::default())),
        Action::Palette => open_palette(s),
        Action::Filter => {
            w.pane = Pane::Entities;
            let text = w.filter.take().map(|f| f.text).unwrap_or_default();
            w.filter = Some(FilterState {
                text,
                editing: true,
                matches: 0,
            });
            rebuild_entities(w);
            first_row(w);
        }
        Action::NextPane => cycle_pane(w, &panes, true),
        Action::PrevPane => cycle_pane(w, &panes, false),
        Action::TabEgo => {
            w.view = View::Main;
            w.tab = Tab::Ego;
            w.pane = Pane::Graph;
        }
        Action::TabTimeline => {
            w.view = View::Main;
            tabs::open_timeline(w, fx);
        }
        Action::TabPath => {
            w.view = View::Main;
            return tabs::open_path(s, fx);
        }
        Action::Findings => toast(s, &format!("findings: {NOT_YET}"), Token::FgMuted),
        Action::Review => screens::open_review(w, None, fx),
        Action::Checklist => screens::open_checklist(w, None, fx),
        Action::ReviewUnsure => {
            let only = w.cursor_entity().map(|e| e.id);
            screens::open_review(w, only, fx);
        }
        Action::Escape => {
            if w.filter.is_some() {
                w.filter = None;
                rebuild_entities(w);
            } else if w.insp_overlay {
                w.insp_overlay = false;
                if w.pane == Pane::Inspector {
                    w.pane = Pane::Graph;
                }
            } else {
                s.dirty = false;
            }
        }
        Action::Down => move_cursor(w, 1),
        Action::Up => move_cursor(w, -1),
        Action::HalfDown => move_cursor(w, half),
        Action::HalfUp => move_cursor(w, -half),
        Action::Top => move_cursor(w, isize::MIN / 2),
        Action::Bottom => move_cursor(w, isize::MAX / 2),
        Action::Left | Action::Right if w.pane == Pane::Graph => move_col(w, a == Action::Right),
        Action::Left | Action::Right => s.dirty = false,
        Action::PrevPage | Action::NextPage => page(w, a == Action::NextPage, fx),
        Action::Activate => activate(w, fx),
        Action::Back => back(w, fx),
        Action::Menu => open_menu(s, None, None),
        Action::RunModule => open_menu(s, None, Some(Action::RunModule)),
        Action::RunPlaybook => open_menu(s, None, Some(Action::RunPlaybook)),
        Action::Capture => open_capture(s),
        Action::OpenEvidence => open_evidence(s, fx),
        Action::EventFilter => toast(s, "e filters the timeline (2)", Token::FgMuted),
        Action::Copy => {
            if let Some(e) = w.cursor_entity() {
                let v = e.value.clone();
                fx.push(Effect::Copy(v));
                toast(s, "copied", Token::Ok);
            }
        }
        Action::OpenUrl => match w.cursor_entity().map(|e| capture_url(w, e)) {
            Some(Some(u)) => fx.push(Effect::OpenUrl(u)),
            // Otherwise its launchers.
            Some(None) => open_menu(s, None, Some(Action::OpenUrl)),
            None => toast(s, "no URL to open", Token::FgMuted),
        },
        Action::Confirm | Action::Unconfirm | Action::Reject => edge_act(s, a, fx),
        Action::Note | Action::Merge | Action::ScopeAdd | Action::ScopeExclude
            if w.cursor_entity().is_none() =>
        {
            toast(s, "no entity under the cursor", Token::FgMuted);
        }
        Action::Note => {
            if let Some(e) = w.cursor_entity() {
                let title = format!("Note on {}", display_value(e, glyphs));
                let id = e.id;
                s.overlay = Some(Overlay::Dialog(Dialog::input(
                    &title,
                    vec![Field::Text {
                        label: "note".into(),
                        value: String::new(),
                        hint: "saved to the case and the audit log".into(),
                    }],
                    Purpose::Note(id),
                )));
            }
        }
        Action::Merge => open_merge(s),
        Action::ScopeAdd | Action::ScopeExclude => {
            if let Some(e) = w.cursor_entity() {
                let rule = scope_rule(e);
                let rule = if a == Action::ScopeExclude {
                    format!("deny {rule}")
                } else {
                    rule
                };
                fx.push(Effect::Act {
                    action: EngineAction::AddScope(rule.clone()),
                    what: format!("scope {rule}"),
                });
            }
        }
        Action::ToggleInspector => {
            if layout::widths(size.0).1 == 0 {
                w.insp_overlay = !w.insp_overlay;
                if !w.insp_overlay && w.pane == Pane::Inspector {
                    w.pane = Pane::Graph;
                }
            } else {
                s.dirty = false;
            }
        }
        Action::Unmerge | Action::NewFinding | Action::Attach => {
            let name = a.name();
            toast(s, &format!("{name}: {NOT_YET}"), Token::FgMuted);
        }
        _ => s.dirty = false,
    }
    if let Some(w) = s.workspace_mut() {
        sync_inspect(w, fx);
    }
}

/// The scope rule an entity suggests (ARCHITECTURE §10 syntax).
pub fn scope_rule(e: &EntitySummary) -> String {
    match e.ty.core() {
        Some(CoreType::Domain | CoreType::Subdomain) => format!("domain:{}", e.key),
        Some(CoreType::Ip) if e.key.contains(':') => format!("cidr:{}/128", e.key),
        Some(CoreType::Ip) => format!("cidr:{}/32", e.key),
        Some(CoreType::Account) => match e.key.split_once(':') {
            Some((p, _)) => format!("platform:{p}"),
            None => format!("regex:^{}$", regex_escape(&e.key)),
        },
        _ => format!("regex:^{}$", regex_escape(&e.key)),
    }
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn cycle_pane(w: &mut Workspace, panes: &[Pane], fwd: bool) {
    let i = panes.iter().position(|p| *p == w.pane).unwrap_or(0);
    let n = panes.len();
    w.pane = panes[if fwd { (i + 1) % n } else { (i + n - 1) % n }];
}

fn move_cursor(w: &mut Workspace, d: isize) {
    match w.pane {
        Pane::Entities => move_ent(w, d),
        Pane::Graph => {
            if let Some(ego) = &w.ego {
                let n = ego::column_items(ego, w.ego_col).len();
                if n > 0 {
                    w.ego_idx = (w.ego_idx as isize + d).clamp(0, n as isize - 1) as usize;
                }
            }
        }
        Pane::Inspector => {
            let n = w.detail.as_ref().map_or(0, |d| d.observations.len());
            if n > 0 {
                w.insp_cursor = (w.insp_cursor as isize + d).clamp(0, n as isize - 1) as usize;
            }
        }
    }
}

fn move_ent(w: &mut Workspace, d: isize) {
    let n = w.ent_items.len() as isize;
    if n == 0 {
        return;
    }
    let step = d.signum();
    let mut i = w.ent_cursor as isize;
    let mut left = d.unsigned_abs();
    let mut last_ok = i;
    while left > 0 {
        let next = i + step;
        if next < 0 || next >= n {
            break;
        }
        i = next;
        if selectable(&w.ent_items[i as usize]) {
            last_ok = i;
            left -= 1;
        }
    }
    w.ent_cursor = last_ok as usize;
}

fn first_row(w: &mut Workspace) {
    w.ent_cursor = w
        .ent_items
        .iter()
        .position(|i| matches!(i, EntItem::Row(_)))
        .or_else(|| w.ent_items.iter().position(selectable))
        .unwrap_or(0);
}

fn move_col(w: &mut Workspace, right: bool) {
    let Some(ego) = &w.ego else { return };
    let has = |c: Col| !ego::column_items(ego, c).is_empty();
    let order = [Col::Left, Col::Card, Col::Right];
    let mut i = order.iter().position(|c| *c == w.ego_col).unwrap_or(1) as isize;
    loop {
        i += if right { 1 } else { -1 };
        if !(0..3).contains(&i) {
            return;
        }
        let c = order[i as usize];
        if c == Col::Card || has(c) {
            w.ego_col = c;
            let n = ego::column_items(ego, c).len();
            w.ego_idx = w.ego_idx.min(n.saturating_sub(1));
            return;
        }
    }
}

fn page(w: &mut Workspace, next: bool, fx: &mut Vec<Effect>) {
    if w.pane != Pane::Graph {
        return;
    }
    let (Some(ego), Some(item)) = (&w.ego, w.ego_item()) else {
        return;
    };
    let gi = match item {
        Item::Row { group, .. } | Item::More { group } => group,
    };
    let Some(g) = ego.groups.get(gi) else { return };
    let p = if next {
        (g.page + 1).min(g.pages.saturating_sub(1))
    } else {
        g.page.saturating_sub(1)
    };
    if p == g.page {
        return;
    }
    w.ego_params.pages.insert(g.key.clone(), p);
    if let Some(f) = w.focus {
        fx.push(Effect::Query(
            Tag::Ego {
                focus: f,
                generation: w.ego_gen,
            },
            Query::Ego {
                focus: f,
                params: w.ego_params.clone(),
            },
        ));
    }
}

fn activate(w: &mut Workspace, fx: &mut Vec<Effect>) {
    match w.pane {
        Pane::Entities => match w.ent_items.get(w.ent_cursor).cloned() {
            Some(EntItem::Header { ty, expanded, .. }) => {
                w.group_open.insert(ty.as_str().to_string(), !expanded);
                rebuild_entities(w);
            }
            Some(EntItem::More { ty, .. }) => {
                w.group_all.insert(ty.as_str().to_string());
                rebuild_entities(w);
            }
            Some(EntItem::Row(i)) => {
                if let Some(e) = w.entities.get(i).cloned() {
                    pivot(w, e, true, fx);
                }
            }
            _ => {}
        },
        Pane::Graph => match w.ego_item() {
            Some(Item::More { .. }) => page(w, true, fx),
            Some(it @ Item::Row { .. }) if w.ego_col != Col::Card => {
                let e = w
                    .ego
                    .as_ref()
                    .and_then(|ego| ego::row(ego, it))
                    .map(|(_, r)| r.entity.clone());
                if let Some(e) = e {
                    pivot(w, e, true, fx);
                }
            }
            _ => {}
        },
        Pane::Inspector => {}
    }
}

/// Make `e` the ego focus; `push` adds it to the pivot history.
pub fn pivot(w: &mut Workspace, e: EntitySummary, push: bool, fx: &mut Vec<Effect>) {
    if w.focus == Some(e.id) {
        return;
    }
    if push {
        w.crumbs.push(e.clone());
    }
    w.focus = Some(e.id);
    w.fresh.remove(&e.id);
    w.ego_gen += 1;
    w.ego_params = Default::default();
    w.ego_col = Col::Right;
    w.ego_idx = 0;
    w.pane = Pane::Graph;
    rebuild_entities(w);
    fx.push(Effect::Query(
        Tag::Ego {
            focus: e.id,
            generation: w.ego_gen,
        },
        Query::Ego {
            focus: e.id,
            params: w.ego_params.clone(),
        },
    ));
}

fn back(w: &mut Workspace, fx: &mut Vec<Effect>) {
    if w.crumbs.len() < 2 {
        return;
    }
    w.crumbs.pop();
    if let Some(prev) = w.crumbs.last().cloned() {
        w.focus = None;
        pivot(w, prev, false, fx);
    }
}

/// Keep the inspector on the cursor entity (TUI_SPEC §4.4), comparing the
/// edge when the cursor is on a correlated row.
pub fn sync_inspect(w: &mut Workspace, fx: &mut Vec<Effect>) {
    if w.pane == Pane::Inspector || w.view != View::Main {
        return;
    }
    let corr = w.cursor_correlation();
    if corr != w.inspect_edge {
        w.inspect_edge = corr;
        w.edge = None;
        if let Some(e) = corr {
            fx.push(Effect::Query(Tag::Edge(e), Query::Edge(e)));
        }
    }
    let Some(id) = w.cursor_entity().map(|e| e.id) else {
        return;
    };
    w.fresh.remove(&id);
    if w.inspect != Some(id) {
        w.inspect = Some(id);
        w.insp_cursor = 0;
        fx.push(Effect::Query(Tag::Detail(id), Query::Detail(id)));
    }
}

pub fn rebuild_entities(w: &mut Workspace) {
    // Keep the cursor on the same entity (or its group header).
    let before = match w.ent_items.get(w.ent_cursor) {
        Some(EntItem::Row(i)) => w.entities.get(*i).map(|e| (Some(e.id), e.ty.clone())),
        Some(EntItem::Header { ty, .. } | EntItem::More { ty, .. }) => Some((None, ty.clone())),
        _ => None,
    };
    let filter = w.filter.as_ref().map(|f| f.text.as_str());
    let (items, matches) = entities::build(&entities::Build {
        entities: &w.entities,
        filter,
        open: &w.group_open,
        all: &w.group_all,
    });
    w.ent_items = items;
    if let Some(f) = &mut w.filter {
        f.matches = matches;
    }
    if let Some((id, ty)) = before {
        let row = id.and_then(|id| {
            w.ent_items
                .iter()
                .position(|it| matches!(it, EntItem::Row(i) if w.entities.get(*i).is_some_and(|e| e.id == id)))
        });
        let header = || {
            w.ent_items
                .iter()
                .position(|it| matches!(it, EntItem::Header { ty: t, .. } if *t == ty))
        };
        if let Some(i) = row.or_else(header) {
            w.ent_cursor = i;
        }
    }
    w.ent_cursor = w.ent_cursor.min(w.ent_items.len().saturating_sub(1));
    if !w.ent_items.get(w.ent_cursor).is_some_and(selectable) {
        move_ent(w, -1);
    }
}

fn edge_act(s: &mut AppState, a: Action, fx: &mut Vec<Effect>) {
    let Some(w) = s.workspace() else { return };
    let Some(edge) = w.cursor_edge() else {
        toast(s, "no edge under the cursor", Token::FgMuted);
        return;
    };
    match a {
        Action::Confirm => fx.push(Effect::Act {
            action: EngineAction::ConfirmEdge(edge),
            what: "confirm".into(),
        }),
        Action::Unconfirm => fx.push(Effect::Act {
            action: EngineAction::UnconfirmEdge(edge),
            what: "unconfirm".into(),
        }),
        _ => {
            let label = w
                .ego_cursor_entity()
                .map(|e| display_value(e, s.glyphs))
                .unwrap_or_default();
            s.overlay = Some(Overlay::Dialog(Dialog::confirm(
                "Reject edge",
                vec![
                    format!("Reject the edge to {label}?"),
                    "It's hidden from the graph and excluded from strength. Press y to reject."
                        .into(),
                ],
                true,
                Purpose::RejectEdge(edge),
            )));
        }
    }
}

fn open_merge(s: &mut AppState) {
    let glyphs = s.glyphs;
    let Some(w) = s.workspace() else { return };
    let Some(e) = w.cursor_entity().cloned() else {
        return;
    };
    let mut d = Dialog::input(
        &format!("Merge {} with…", display_value(&e, glyphs)),
        vec![Field::Text {
            label: "find".into(),
            value: String::new(),
            hint: format!("another {}", e.ty.as_str()),
        }],
        Purpose::Merge { into: e.id },
    );
    d.picks = w
        .entities
        .iter()
        .filter(|o| o.ty == e.ty && o.id != e.id)
        .cloned()
        .collect();
    d.body = vec![format!(
        "The other entity is merged into {}; unmerge restores it.",
        display_value(&e, glyphs)
    )];
    s.overlay = Some(Overlay::Dialog(d));
}

fn refilter_picks(s: &mut AppState) {
    let Some(w) = s.workspace() else { return };
    let Some(Overlay::Dialog(d)) = &s.overlay else {
        return;
    };
    let q = d.fields.first().map(Field::value).unwrap_or_default();
    let candidates: Vec<EntitySummary> = match d.purpose {
        Purpose::Merge { into } => {
            let Some(target) = w.entity(into).cloned() else {
                return;
            };
            w.entities
                .iter()
                .filter(|o| o.ty == target.ty && o.id != into)
                .cloned()
                .collect()
        }
        Purpose::PathTo => w
            .entities
            .iter()
            .filter(|o| Some(o.id) != w.focus)
            .cloned()
            .collect(),
        _ => return,
    };
    let keep = entities::matching(&candidates, &q);
    let picks: Vec<EntitySummary> = keep.into_iter().map(|i| candidates[i].clone()).collect();
    if let Some(Overlay::Dialog(d)) = &mut s.overlay {
        d.pick_sel = d.pick_sel.min(picks.len().saturating_sub(1));
        d.picks = picks;
    }
}

/// The URL `c` captures for an entity: a url's own, an account's profile,
/// an image's source.
fn capture_url(w: &Workspace, e: &EntitySummary) -> Option<String> {
    if e.ty.is(CoreType::Url) {
        return Some(e.key.clone());
    }
    let d = w.detail.as_ref().filter(|d| d.entity.summary.id == e.id)?;
    let attr = if e.ty.is(CoreType::Image) {
        "source_url"
    } else {
        "profile_url"
    };
    d.entity
        .attrs
        .get(attr)?
        .iter()
        .find_map(|a| match &a.value {
            spektr_model::AttrValue::Url(u) | spektr_model::AttrValue::Text(u) => Some(u.clone()),
            _ => None,
        })
}

/// `c`: confirm (or type) the URL to capture for the cursor entity.
fn open_capture(s: &mut AppState) {
    let glyphs = s.glyphs;
    let Some(w) = s.workspace() else { return };
    let target = w.cursor_entity().cloned();
    let url = target.as_ref().and_then(|e| capture_url(w, e));
    let mut d = Dialog::input(
        "Capture",
        vec![Field::Text {
            label: "url".into(),
            value: url.unwrap_or_default(),
            hint: "HTML, headers and a screenshot".into(),
        }],
        Purpose::Capture(target.as_ref().map(|e| e.id)),
    );
    if let Some(e) = &target {
        d.body = vec![format!(
            "Saved as evidence for {} {}.",
            glyphs.entity(&e.ty),
            display_value(e, glyphs)
        )];
    }
    s.overlay = Some(Overlay::Dialog(d));
}

/// `o`: the selected evidence (inspector), else the entity's newest: a raw
/// response in the pager, a capture's screenshot or an image in the viewer.
fn open_evidence(s: &mut AppState, fx: &mut Vec<Effect>) {
    let Some(w) = s.workspace() else { return };
    let Some(e) = w.cursor_entity().cloned() else {
        return toast(s, "no entity under the cursor", Token::FgMuted);
    };
    let Some(d) = w.detail.as_ref().filter(|d| d.entity.summary.id == e.id) else {
        return toast(s, "loading…", Token::FgMuted);
    };
    if w.pane != Pane::Inspector
        && let Some(sha) = e.key.strip_prefix("sha256:")
    {
        fx.push(Effect::OpenBlob {
            sha: sha.to_string(),
            name: e.value.clone(),
            reveal: false,
        });
        return;
    }
    let obs = if w.pane == Pane::Inspector {
        d.observations.get(w.insp_cursor)
    } else {
        d.observations.iter().max_by_key(|o| o.created_at)
    };
    match obs {
        Some(o) => fx.push(Effect::OpenEvidence(o.id)),
        None => toast(s, "no evidence", Token::FgMuted),
    }
}

// ---- palette

fn open_palette(s: &mut AppState) {
    let target = s.workspace().and_then(|w| w.cursor_entity().cloned());
    s.overlay = Some(Overlay::Palette(Palette {
        target,
        ..Default::default()
    }));
    refilter_palette(s);
}

fn hhmm(t: jiff::Timestamp) -> String {
    t.strftime("%H:%M").to_string()
}

fn refilter_palette(s: &mut AppState) {
    let glyphs = s.glyphs;
    let history = s.history.clone();
    let Some(Overlay::Palette(p)) = &s.overlay else {
        return;
    };
    let target = p.target.clone();
    let input = p.input.clone();
    let (modules, last) = match s.workspace() {
        Some(w) => (w.modules.clone(), w.last_run.clone()),
        None => (Vec::new(), Default::default()),
    };
    let tid = target.as_ref().map(|t| t.id);
    let last_run = |m: &str| {
        tid.and_then(|id| last.get(&(id, m.to_string())).copied())
            .map(hhmm)
    };
    let all = palette::candidates(&palette::Candidates {
        modules: &modules,
        target: target.as_ref(),
        history: &history,
        glyphs,
        last_run: &last_run,
    });
    let total = all.len();
    let items = palette::filter(all, &input);
    if let Some(Overlay::Palette(p)) = &mut s.overlay {
        p.sel = p.sel.min(items.len().saturating_sub(1));
        p.items = items;
        p.total = total;
    }
}

fn palette_act(s: &mut AppState, mut p: Palette, a: Action, fx: &mut Vec<Effect>) {
    match a {
        Action::Escape => return,
        Action::SelectDown => p.sel = (p.sel + 1).min(p.items.len().saturating_sub(1)),
        Action::SelectUp => p.sel = p.sel.saturating_sub(1),
        Action::Complete => {
            if let Some(i) = p.items.get(p.sel) {
                p.input = i.line.clone();
                p.sel = 0;
                s.overlay = Some(Overlay::Palette(p));
                refilter_palette(s);
                return;
            }
        }
        Action::Select => {
            let item = p.items.get(p.sel).cloned();
            let line = match &item {
                Some(i)
                    if p.input.trim_start().starts_with(i.line.trim_end())
                        && p.input.trim() != i.line.trim() =>
                {
                    p.input.trim().to_string()
                }
                Some(i) if i.disabled.is_some() => {
                    p.error = i.disabled.clone();
                    s.overlay = Some(Overlay::Palette(p));
                    return;
                }
                Some(i) if i.line.ends_with(' ') => {
                    p.input = i.line.clone();
                    s.overlay = Some(Overlay::Palette(p));
                    refilter_palette(s);
                    return;
                }
                Some(i) => i.line.clone(),
                None => p.input.trim().to_string(),
            };
            match palette::parse(&line) {
                Ok(cmd) => {
                    s.history.retain(|h| h != &line);
                    s.history.insert(0, line);
                    s.history.truncate(50);
                    run_cmd(s, cmd, p.target.as_ref(), fx);
                    return;
                }
                Err(e) => p.error = Some(e),
            }
        }
        _ => {}
    }
    s.overlay = Some(Overlay::Palette(p));
}

fn run_cmd(s: &mut AppState, cmd: Cmd, target: Option<&EntitySummary>, fx: &mut Vec<Effect>) {
    let need_target = |s: &mut AppState| {
        if target.is_none() {
            toast(s, "no entity under the cursor", Token::FgMuted);
        }
        target.map(|t| t.id)
    };
    match cmd {
        Cmd::Seed(r) => fx.push(Effect::Act {
            what: format!("seed {}:{}", r.ty.as_str(), r.value),
            action: EngineAction::Seed { entity: r },
        }),
        Cmd::Run { module, rerun } => {
            if let Some(id) = need_target(s) {
                fx.push(Effect::Act {
                    what: format!("run {module}"),
                    action: EngineAction::Run {
                        module,
                        entity: id,
                        rerun,
                    },
                });
            }
        }
        Cmd::Expand => {
            if let Some(id) = need_target(s) {
                fx.push(Effect::Act {
                    what: "expand".into(),
                    action: EngineAction::Expand { entity: id },
                });
            }
        }
        Cmd::Depth(d) => fx.push(Effect::Act {
            what: format!("depth {d}"),
            action: EngineAction::SetDepth(d),
        }),
        Cmd::ScopeAdd(rule) => fx.push(Effect::Act {
            what: format!("scope add {rule}"),
            action: EngineAction::AddScope(rule),
        }),
        Cmd::ScopeList => fx.push(Effect::Query(Tag::Find, Query::Scope)),
        Cmd::Rules(p) => fx.push(Effect::Act {
            what: format!("rules {p}"),
            action: EngineAction::SetProfile(p),
        }),
        Cmd::Region(r) => fx.push(Effect::Act {
            what: format!("region {r}"),
            action: EngineAction::SetRegion(r),
        }),
        Cmd::Merge => open_merge(s),
        Cmd::Confirm => edge_act(s, Action::Confirm, fx),
        Cmd::Reject => edge_act(s, Action::Reject, fx),
        Cmd::Note(body) => {
            if let Some(id) = need_target(s) {
                fx.push(Effect::Act {
                    what: "note".into(),
                    action: EngineAction::AddNote { entity: id, body },
                });
            }
        }
        Cmd::Verify => {
            if let Some(w) = s.workspace() {
                let p = w.path.clone();
                s.verify_requested = Some(p.clone());
                fx.push(Effect::Verify(p));
                toast(s, "verifying…", Token::FgMuted);
            }
        }
        Cmd::Mouse(on) => {
            s.mouse = on;
            fx.push(Effect::SetMouse(on));
            toast(s, if on { "mouse on" } else { "mouse off" }, Token::FgMuted);
        }
        Cmd::Glyphs(g) => s.glyphs = g,
        Cmd::Help => s.overlay = Some(Overlay::Help(Help::default())),
        Cmd::Path(r) => fx.push(Effect::Query(Tag::FindPath, Query::Find(r))),
        Cmd::Capture(None) => open_capture(s),
        Cmd::Capture(Some(url)) => fx.push(Effect::Act {
            what: "capture".into(),
            action: EngineAction::Capture {
                entity: target.map(|t| t.id),
                url,
                screenshot: true,
            },
        }),
        Cmd::Attach(path) => fx.push(Effect::AttachFile {
            path: expand_home(&path, s.settings.home.as_deref()),
            entity: target.map(|t| t.id),
        }),
        Cmd::Open(launcher) => {
            if let Some(id) = need_target(s) {
                fx.push(Effect::Act {
                    what: launcher.clone(),
                    action: EngineAction::OpenLauncher {
                        launcher,
                        entity: id,
                        item: None,
                    },
                });
            }
        }
        Cmd::Playbook(name) => {
            if let Some(id) = need_target(s) {
                fx.push(Effect::Act {
                    what: format!("playbook {name}"),
                    action: EngineAction::StartPlaybook { name, entity: id },
                });
            }
        }
        Cmd::PlaybookStop => fx.push(Effect::Act {
            what: "playbook stop".into(),
            action: EngineAction::StopPlaybook,
        }),
        Cmd::Checklist => {
            if let Some(w) = s.workspace_mut() {
                screens::open_checklist(w, None, fx);
            }
        }
        Cmd::Reload => fx.push(Effect::Act {
            what: "reload".into(),
            action: EngineAction::Reload,
        }),
        Cmd::Images(m) => {
            fx.push(Effect::SetImages(m));
            toast(s, &format!("images {}", m.as_str()), Token::FgMuted);
        }
    }
}

// ---- context menu

/// Open the context menu on the cursor entity; `sub` opens that item's
/// submenu (`Run module ▸`, `Run playbook ▸`) straight away. `at` is the
/// click cell for a right-click.
pub fn open_menu(s: &mut AppState, at: Option<(u16, u16)>, sub: Option<Action>) {
    let glyphs = s.glyphs;
    let size = s.size;
    let Some(w) = s.workspace() else { return };
    let Some(target) = w.cursor_entity().cloned() else {
        toast(s, "no entity under the cursor", Token::FgMuted);
        return;
    };
    let edge = w.cursor_edge();
    let obs = w
        .detail
        .as_ref()
        .filter(|d| d.entity.summary.id == target.id)
        .map(|d| d.observations.len());
    let subtitle = match obs {
        Some(n) => format!(
            "{} {} {n} observation{}",
            target.ty.as_str(),
            glyphs.m(Marker::Dot),
            if n == 1 { "" } else { "s" }
        ),
        None => target.ty.as_str().to_string(),
    };
    let detail = w
        .detail
        .as_ref()
        .filter(|d| d.entity.summary.id == target.id);
    let url = capture_url(w, &target);
    let items = menu::build(
        &s.keymap,
        &target,
        &menu::Info {
            edge,
            is_focus: w.focus == Some(target.id),
            launchers: detail.map(|d| d.launchers.as_slice()),
            playbooks: &w.checklist.playbooks,
            observations: obs.unwrap_or(0),
            url: url.as_deref(),
        },
    );
    let anchor = at.unwrap_or_else(|| row_anchor(w, &s.hits, size));
    let mut m = Menu {
        anchor,
        target,
        edge,
        subtitle,
        items,
        sel: 0,
        sub: None,
    };
    if let Some(a) = sub {
        match m.items.iter().position(|i| i.act == MenuAct::Key(a)) {
            Some(i) if m.items[i].submenu => {
                m.sel = i;
                open_sub(w, &s.keymap, &mut m);
            }
            Some(i) => {
                let why = m.items[i].disabled.clone().unwrap_or_default();
                return toast(s, &why, Token::FgMuted);
            }
            None => {}
        }
    }
    s.overlay = Some(Overlay::Menu(m));
}

/// The right end of the cursor row, for keyboard-opened menus: from the
/// last frame's hit regions, else an estimate.
fn row_anchor(
    w: &Workspace,
    hits: &[(ratatui::layout::Rect, Hit)],
    size: (u16, u16),
) -> (u16, u16) {
    let want = match w.pane {
        Pane::Entities => Some(Hit::EntRow(w.ent_cursor)),
        Pane::Graph if w.ego_col == Col::Card => Some(Hit::Card),
        Pane::Graph => Some(Hit::Ego(w.ego_col, w.ego_idx)),
        Pane::Inspector => Some(Hit::InspRow(w.insp_cursor)),
    };
    if let Some(r) = want.and_then(|h| hits.iter().rev().find(|(_, x)| *x == h).map(|(r, _)| *r)) {
        return (r.right().saturating_sub(2), r.y);
    }
    let l = layout::workspace(size.0, size.1);
    match w.pane {
        Pane::Entities => {
            let inner = layout::inner(l.entities);
            let y = inner.y + 2 + w.ent_cursor as u16;
            (
                inner.right().saturating_sub(4),
                y.min(inner.bottom().saturating_sub(1)),
            )
        }
        _ => {
            let inner = layout::inner(l.graph);
            (inner.x + inner.width / 3, inner.y + 4)
        }
    }
}

fn open_sub(w: &Workspace, km: &crate::keymap::Keymap, m: &mut Menu) {
    let target = m.target.clone();
    let last_run = |name: &str| {
        w.last_run
            .get(&(target.id, name.to_string()))
            .copied()
            .map(hhmm)
    };
    let launchers = w
        .detail
        .as_ref()
        .filter(|d| d.entity.summary.id == target.id)
        .map(|d| d.launchers.clone())
        .unwrap_or_default();
    m.sub = Some(match m.items.get(m.sel).map(|i| &i.act) {
        Some(MenuAct::Key(Action::RunPlaybook)) => {
            menu::playbook_submenu(&target, &w.checklist.playbooks, w.profile())
        }
        Some(MenuAct::Key(Action::OpenUrl)) => {
            let url = capture_url(w, &target);
            menu::launcher_submenu(km, &target, url.as_deref(), &launchers)
        }
        _ => menu::run_submenu(km, &target, &w.modules, &last_run),
    });
}

fn menu_act(s: &mut AppState, mut m: Menu, a: Action, fx: &mut Vec<Effect>) {
    match a {
        Action::Escape => return,
        Action::SelectDown | Action::SelectUp => {
            let d: isize = if a == Action::SelectDown { 1 } else { -1 };
            match &mut m.sub {
                Some(sub) => {
                    sub.sel = (sub.sel as isize + d).clamp(0, sub.items.len() as isize - 1) as usize
                }
                None => m.sel = (m.sel as isize + d).clamp(0, m.items.len() as isize - 1) as usize,
            }
        }
        Action::SubmenuOpen => {
            if m.sub.is_none()
                && m.items.get(m.sel).is_some_and(|i| i.submenu)
                && let Some(w) = s.workspace()
            {
                open_sub(w, &s.keymap, &mut m);
            }
        }
        Action::SubmenuBack => m.sub = None,
        Action::RunAll => {
            if m.sub.is_some() {
                return run_menu_act(s, &m, MenuAct::RunAll, fx);
            }
        }
        Action::Select => {
            let item = match &m.sub {
                Some(sub) => sub.items.get(sub.sel).cloned(),
                None => m.items.get(m.sel).cloned(),
            };
            if let Some(item) = item {
                if let Some(why) = &item.disabled {
                    toast(s, why, Token::FgMuted);
                } else if item.submenu && m.sub.is_none() {
                    if let Some(w) = s.workspace() {
                        open_sub(w, &s.keymap, &mut m);
                    }
                } else {
                    return run_menu_act(s, &m, item.act, fx);
                }
            }
        }
        other => {
            // A submenu item's hotkey opens that submenu.
            if let Some(i) = m
                .items
                .iter()
                .position(|it| it.act == MenuAct::Key(other) && it.submenu)
                && let Some(w) = s.workspace()
            {
                m.sel = i;
                open_sub(w, &s.keymap, &mut m);
            } else {
                // Entity hotkeys and global keys work while the menu is open.
                return act(s, other, fx);
            }
        }
    }
    s.overlay = Some(Overlay::Menu(m));
}

fn run_menu_act(s: &mut AppState, m: &Menu, a: MenuAct, fx: &mut Vec<Effect>) {
    match a {
        MenuAct::RunModule(module) => fx.push(Effect::Act {
            what: format!("run {module}"),
            action: EngineAction::Run {
                module,
                entity: m.target.id,
                rerun: true,
            },
        }),
        MenuAct::RunAll => fx.push(Effect::Act {
            what: "run all eligible".into(),
            action: EngineAction::RunAll {
                entity: m.target.id,
            },
        }),
        MenuAct::Url(u) => fx.push(Effect::OpenUrl(u)),
        MenuAct::RunPlaybook(name) => fx.push(Effect::Act {
            what: format!("playbook {name}"),
            action: EngineAction::StartPlaybook {
                name,
                entity: m.target.id,
            },
        }),
        MenuAct::Launch { launcher, item } => fx.push(Effect::Act {
            what: launcher.clone(),
            action: EngineAction::OpenLauncher {
                launcher,
                entity: m.target.id,
                item,
            },
        }),
        MenuAct::Key(Action::Activate) => {
            if let Some(w) = s.workspace_mut() {
                pivot(w, m.target.clone(), true, fx);
                sync_inspect(w, fx);
            }
        }
        MenuAct::Key(a) => act(s, a, fx),
    }
}

// ---- help

fn help_act(s: &mut AppState, mut h: Help, a: Action, fx: &mut Vec<Effect>) {
    match a {
        Action::Escape if h.searching => h.searching = false,
        Action::Escape | Action::Help => return,
        Action::HelpSearch => h.searching = true,
        Action::Down => h.scroll += 1,
        Action::Up => h.scroll = h.scroll.saturating_sub(1),
        Action::HalfDown => h.scroll += 10,
        Action::HalfUp => h.scroll = h.scroll.saturating_sub(10),
        Action::Top => h.scroll = 0,
        Action::Bottom => h.scroll = usize::MAX / 2,
        Action::Quit => return quit(s, fx),
        _ => {}
    }
    s.overlay = Some(Overlay::Help(h));
}

// ---- dialogs

fn dialog_act(s: &mut AppState, mut d: Dialog, a: Action, fx: &mut Vec<Effect>) {
    let n = d.fields.len();
    match a {
        Action::Escape => return,
        Action::SelectDown if !d.picks.is_empty() => {
            d.pick_sel = (d.pick_sel + 1).min(d.picks.len() - 1)
        }
        Action::SelectUp if !d.picks.is_empty() => d.pick_sel = d.pick_sel.saturating_sub(1),
        Action::SelectDown if n > 0 => d.focus = (d.focus + 1) % n,
        Action::SelectUp if n > 0 => d.focus = (d.focus + n - 1) % n,
        Action::Complete if n > 0 => d.focus = (d.focus + 1) % n,
        Action::SubmenuOpen | Action::SubmenuBack => {
            if let Some(Field::Choice { options, idx, .. }) = d.fields.get_mut(d.focus)
                && !options.is_empty()
            {
                let k = options.len();
                *idx = if a == Action::SubmenuOpen {
                    (*idx + 1) % k
                } else {
                    (*idx + k - 1) % k
                };
            }
        }
        Action::Yes if d.destructive || d.fields.is_empty() => return dialog_ok(s, d, fx),
        Action::Select if !d.destructive => return dialog_ok(s, d, fx),
        Action::Select => d.error = Some("press y to confirm, esc to cancel".into()),
        _ => {}
    }
    s.overlay = Some(Overlay::Dialog(d));
}

fn dialog_ok(s: &mut AppState, mut d: Dialog, fx: &mut Vec<Effect>) {
    let val = |i: usize| {
        d.fields
            .get(i)
            .map(Field::value)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    match d.purpose.clone() {
        Purpose::Quit => {
            s.quit = true;
            fx.push(Effect::Quit);
        }
        Purpose::RejectEdge(e) => fx.push(Effect::Act {
            action: EngineAction::RejectEdge(e),
            what: "reject".into(),
        }),
        Purpose::Note(id) => {
            let body = val(0);
            if body.is_empty() {
                d.error = Some("the note is empty".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            fx.push(Effect::Act {
                action: EngineAction::AddNote { entity: id, body },
                what: "note".into(),
            });
        }
        Purpose::OpenPath => {
            let p = val(0);
            if p.is_empty() {
                d.error = Some("enter a path".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            let path = expand_home(&p, s.settings.home.as_deref());
            s.screen = Screen::Opening(path.clone());
            fx.push(Effect::Open(path));
        }
        Purpose::NewCase => {
            let name = val(0);
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
            {
                d.error = Some("the name may only use letters, digits, . _ -".into());
                d.focus = 0;
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            let last_seen = Some(val(3)).filter(|v| !v.is_empty());
            if let Some(ls) = &last_seen
                && ls.parse::<jiff::civil::Date>().is_err()
            {
                d.error = Some(format!("last seen `{ls}` is not YYYY-MM-DD"));
                d.focus = 3;
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            fx.push(Effect::Create(crate::host::NewCase {
                name,
                profile: val(1),
                subject: Some(val(2)).filter(|v| !v.is_empty()),
                last_seen,
            }));
            // Keep the dialog until the case exists, to show errors.
            d.error = None;
            s.overlay = Some(Overlay::Dialog(d));
        }
        p @ (Purpose::ItemDone(_)
        | Purpose::ItemSkip(_)
        | Purpose::ItemNote(_)
        | Purpose::StopRun
        | Purpose::ReviewMerge { .. }) => {
            if matches!(p, Purpose::ItemSkip(_)) && val(0).is_empty() {
                d.error = Some("give a reason".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            screens::dialog_ok(&p, val(0), fx);
        }
        Purpose::PathTo => {
            let Some(to) = d.picks.get(d.pick_sel).cloned() else {
                d.error = Some("no match".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            };
            if let Some(w) = s.workspace_mut() {
                tabs::path_to(w, to, fx);
            }
        }
        Purpose::Capture(entity) => {
            let url = val(0);
            if url.is_empty() {
                d.error = Some("enter a URL".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            toast(s, "capturing…", Token::FgMuted);
            fx.push(Effect::Act {
                what: "capture".into(),
                action: EngineAction::Capture {
                    entity,
                    url,
                    screenshot: true,
                },
            });
        }
        Purpose::Attach(entity) => {
            let p = val(0);
            if p.is_empty() {
                d.error = Some("enter a path".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            }
            fx.push(Effect::AttachFile {
                path: expand_home(&p, s.settings.home.as_deref()),
                entity,
            });
        }
        Purpose::Merge { into } => {
            let Some(other) = d.picks.get(d.pick_sel).cloned() else {
                d.error = Some("nothing to merge with".into());
                s.overlay = Some(Overlay::Dialog(d));
                return;
            };
            fx.push(Effect::Act {
                action: EngineAction::Merge {
                    survivor: into,
                    merged: other.id,
                },
                what: "merge".into(),
            });
        }
    }
}

fn expand_home(p: &str, home: Option<&std::path::Path>) -> std::path::PathBuf {
    match (p.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => std::path::PathBuf::from(p),
    }
}

// ---- mouse

fn hit_at(s: &AppState, x: u16, y: u16) -> Option<Hit> {
    s.hits
        .iter()
        .rev()
        .find(|(r, _)| x >= r.x && x < r.right() && y >= r.y && y < r.bottom())
        .map(|(_, h)| *h)
}

fn is_overlay_hit(h: Hit) -> bool {
    matches!(
        h,
        Hit::PaletteItem(_)
            | Hit::MenuItem(_)
            | Hit::SubItem(_)
            | Hit::DialogItem(_)
            | Hit::Overlay
    )
}

fn mouse(s: &mut AppState, m: MouseEvent, ms: u64, fx: &mut Vec<Effect>) {
    let hit = hit_at(s, m.column, m.row);
    match m.kind {
        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
            s.dirty = hit != s.hover;
            s.hover = hit;
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if s.overlay.is_some() && !hit.is_some_and(is_overlay_hit) {
                s.overlay = None;
                return;
            }
            let Some(h) = hit else { return };
            let double = s
                .last_click
                .is_some_and(|(t, prev)| prev == h && ms.saturating_sub(t) <= DOUBLE_CLICK_MS);
            s.last_click = if double { None } else { Some((ms, h)) };
            click(s, h, double, fx);
        }
        MouseEventKind::Down(MouseButton::Right) => {
            if s.overlay.is_some() {
                s.overlay = None;
            }
            let Some(h) = hit else { return };
            if matches!(h, Hit::EntRow(_) | Hit::Ego(..) | Hit::Card) {
                click(s, h, false, fx);
                open_menu(s, Some((m.column, m.row)), None);
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let d = if m.kind == MouseEventKind::ScrollDown {
                WHEEL as isize
            } else {
                -(WHEEL as isize)
            };
            wheel(s, hit, d, fx);
        }
        _ => s.dirty = false,
    }
}

fn wheel(s: &mut AppState, hit: Option<Hit>, d: isize, fx: &mut Vec<Effect>) {
    if let Some(Overlay::Help(h)) = &mut s.overlay {
        h.scroll = (h.scroll as isize + d).max(0) as usize;
        return;
    }
    if s.overlay.is_some() {
        return;
    }
    if let Screen::Launcher(l) = &mut s.screen {
        let n = l.recent.as_ref().map_or(0, Vec::len) as isize;
        l.cursor = (l.cursor as isize + d.signum()).clamp(0, (n - 1).max(0)) as usize;
        return;
    }
    let Some(w) = s.workspace_mut() else { return };
    if w.view != View::Main {
        if let Some(Hit::RunRow(_)) = hit {
            w.checklist.pane = crate::state::ChecklistPane::Runs;
        } else if let Some(Hit::ItemRow(_)) = hit {
            w.checklist.pane = crate::state::ChecklistPane::Items;
        }
        let a = if d > 0 { Action::Down } else { Action::Up };
        for _ in 0..d.unsigned_abs() {
            screens::act(s, a, fx);
        }
        return;
    }
    if w.tab != Tab::Ego && matches!(hit, Some(Hit::TimeRow(_) | Hit::Pane(Pane::Graph))) {
        w.pane = Pane::Graph;
        let a = if d > 0 { Action::Down } else { Action::Up };
        for _ in 0..d.unsigned_abs() {
            tabs::act(w, a, fx);
        }
        return;
    }
    let pane = match hit {
        Some(Hit::EntRow(_) | Hit::Pane(Pane::Entities)) => Some(Pane::Entities),
        Some(Hit::Ego(..) | Hit::Card | Hit::Crumb(_) | Hit::Pane(Pane::Graph)) => {
            Some(Pane::Graph)
        }
        Some(Hit::InspRow(_) | Hit::Pane(Pane::Inspector)) => Some(Pane::Inspector),
        Some(Hit::Activity) => {
            let max = w.activity.len().saturating_sub(1) as isize;
            w.act_scroll = (w.act_scroll as isize + d).clamp(0, max.max(0)) as usize;
            return;
        }
        _ => None,
    };
    if let Some(p) = pane {
        w.pane = p;
        move_cursor(w, d);
        sync_inspect(w, fx);
    }
}

fn click(s: &mut AppState, h: Hit, double: bool, fx: &mut Vec<Effect>) {
    match h {
        Hit::Pill => {
            s.overlay = Some(Overlay::Help(Help::default()));
            return;
        }
        Hit::LauncherRow(i) => {
            if let Screen::Launcher(l) = &mut s.screen {
                l.cursor = i;
            }
            if double {
                act(s, Action::LauncherOpen, fx);
            }
            return;
        }
        Hit::PaletteItem(i) => {
            if let Some(Overlay::Palette(p)) = &mut s.overlay {
                p.sel = i;
            }
            act(s, Action::Select, fx);
            return;
        }
        Hit::MenuItem(i) => {
            if let Some(Overlay::Menu(m)) = &mut s.overlay {
                m.sel = i;
                m.sub = None;
            }
            act(s, Action::Select, fx);
            return;
        }
        Hit::SubItem(i) => {
            if let Some(Overlay::Menu(m)) = &mut s.overlay
                && let Some(sub) = &mut m.sub
            {
                sub.sel = i;
            }
            act(s, Action::Select, fx);
            return;
        }
        Hit::DialogItem(i) => {
            if let Some(Overlay::Dialog(d)) = &mut s.overlay {
                if d.picks.is_empty() {
                    d.focus = i.min(d.fields.len().saturating_sub(1));
                } else {
                    d.pick_sel = i;
                    if double {
                        act(s, Action::Select, fx);
                    }
                }
            }
            return;
        }
        Hit::Overlay => return,
        _ => {}
    }
    let Some(w) = s.workspace_mut() else { return };
    match h {
        Hit::Pane(p) => w.pane = p,
        Hit::EntRow(i) => {
            w.pane = Pane::Entities;
            if w.ent_items.get(i).is_some_and(selectable) {
                w.ent_cursor = i;
                if double {
                    activate(w, fx);
                }
            }
        }
        Hit::Ego(col, i) => {
            w.pane = Pane::Graph;
            w.ego_col = col;
            w.ego_idx = i;
            if double {
                activate(w, fx);
            }
        }
        Hit::Card => {
            w.pane = Pane::Graph;
            w.ego_col = Col::Card;
        }
        Hit::Crumb(i) => {
            if i + 1 < w.crumbs.len() {
                w.crumbs.truncate(i + 1);
                if let Some(e) = w.crumbs.last().cloned() {
                    w.focus = None;
                    pivot(w, e, false, fx);
                }
            }
        }
        Hit::InspRow(i) => {
            w.pane = Pane::Inspector;
            w.insp_cursor = i;
        }
        Hit::Tab(t) => {
            let a = match t {
                Tab::Ego => Action::TabEgo,
                Tab::Timeline => Action::TabTimeline,
                Tab::Path => Action::TabPath,
            };
            return act(s, a, fx);
        }
        Hit::RunRow(i) => {
            w.checklist.pane = crate::state::ChecklistPane::Runs;
            w.checklist.run_sel = i;
            if double {
                return act(s, Action::Activate, fx);
            }
        }
        Hit::ItemRow(i) => {
            w.checklist.pane = crate::state::ChecklistPane::Items;
            w.checklist.item_sel = i;
            if double {
                return act(s, Action::Activate, fx);
            }
        }
        Hit::ItemFilterTab(f) => {
            w.checklist.filter = f;
            w.checklist.item_sel = 0;
        }
        Hit::ReviewRow(i) => {
            w.review.sel = i;
            screens::compare(w, fx);
        }
        Hit::ReviewFilterTab(f) => {
            w.review.filter = f;
            w.review.sel = 0;
            screens::compare(w, fx);
        }
        Hit::TimeRow(i) => {
            w.pane = Pane::Graph;
            match w.tab {
                Tab::Timeline => w.timeline.cursor = i,
                _ => w.path_tab.cursor = i,
            }
            if double {
                return act(s, Action::Activate, fx);
            }
        }
        _ => {}
    }
    sync_inspect(w, fx);
}

// ---- engine

pub fn schedule(w: &mut Workspace, fx: &mut Vec<Effect>) {
    if !w.refresh_scheduled {
        w.refresh_scheduled = true;
        fx.push(Effect::ScheduleRefresh);
    }
}

fn refresh(s: &mut AppState, fx: &mut Vec<Effect>) {
    let Some(w) = s.workspace_mut() else { return };
    w.refresh_scheduled = false;
    let st = std::mem::take(&mut w.stale);
    if st.status {
        fx.push(Effect::Query(Tag::Status, Query::Status));
    }
    if st.entities {
        fx.push(Effect::Query(Tag::Entities, Query::Entities));
    }
    if st.ego
        && let Some(f) = w.focus
    {
        fx.push(Effect::Query(
            Tag::Ego {
                focus: f,
                generation: w.ego_gen,
            },
            Query::Ego {
                focus: f,
                params: w.ego_params.clone(),
            },
        ));
    }
    if st.detail
        && let Some(id) = w.inspect
    {
        fx.push(Effect::Query(Tag::Detail(id), Query::Detail(id)));
    }
    if st.detail
        && let Some(e) = w.inspect_edge
    {
        fx.push(Effect::Query(Tag::Edge(e), Query::Edge(e)));
    }
    if st.screen {
        fx.extend(screens::queries(w));
        if w.view == View::Main {
            fx.extend(tabs::queries(w));
        }
    }
}

fn ent_span(e: &EntitySummary, g: crate::glyph::GlyphSet) -> Vec<(String, Token)> {
    vec![
        (format!("{} ", g.entity(&e.ty)), Token::FgMuted),
        (display_value(e, g), Token::Fg),
    ]
}

fn push_row(w: &mut Workspace, row: ActRow) {
    w.activity.push_front(row);
    w.activity.truncate(ACTIVITY_CAP);
    if w.act_scroll > 0 {
        w.act_scroll += 1;
    }
}

fn engine_event(s: &mut AppState, ev: EngineEvent, fx: &mut Vec<Effect>) {
    let g = s.glyphs;
    let Some(w) = s.workspace_mut() else { return };
    let at = ev.at;
    let row = |marker, token, module: &str, module_token, spans| ActRow {
        at,
        marker,
        token,
        module: module.to_string(),
        module_token,
        spans,
    };
    w.stale.status = true;
    // Screens and tabs re-query on anything that changes what they list.
    if !matches!(
        ev.kind,
        EventKind::JobProgress { .. } | EventKind::JobQueued { .. } | EventKind::Budget { .. }
    ) && (w.view != View::Main || w.tab != Tab::Ego)
    {
        w.stale.screen = true;
    }
    match ev.kind {
        EventKind::JobQueued { .. } => {}
        EventKind::JobStarted { module, entity, .. } => {
            let mut spans = vec![
                ("running  ".to_string(), Token::FgMuted),
                ("on ".into(), Token::FgMuted),
            ];
            spans.extend(ent_span(&entity, g));
            push_row(
                w,
                row(Marker::Running, Token::Accent, &module, Token::Fg, spans),
            );
        }
        EventKind::JobProgress { .. } => w.stale.status = false,
        EventKind::JobFinished {
            module,
            entity,
            state,
            found,
            note,
            error,
            retry_in_secs,
            ..
        } => {
            match state {
                JobState::Done => {
                    let mut spans: Vec<(String, Token)> = Vec::new();
                    if let Some(n) = note {
                        spans.push((n, Token::Fg));
                    } else if found.is_empty() {
                        spans.push(("nothing new".into(), Token::FgMuted));
                    } else {
                        for (ty, n) in &found {
                            let glyph = spektr_model::CoreType::parse(ty)
                                .map_or(g.m(Marker::Dot).to_string(), |c| {
                                    g.entity(&c.into()).to_string()
                                });
                            spans.push((format!("+{n} "), Token::Fg));
                            spans.push((format!("{glyph} {ty}  "), Token::FgMuted));
                        }
                    }
                    spans.push(("  ← ".into(), Token::FgMuted));
                    spans.extend(ent_span(&entity, g));
                    push_row(w, row(Marker::Ok, Token::Ok, &module, Token::Fg, spans));
                    w.last_run.insert((entity.id, module.clone()), at);
                }
                JobState::Failed => {
                    let mut spans =
                        vec![(error.unwrap_or_else(|| "failed".into()), Token::Hostile)];
                    if let Some(secs) = retry_in_secs {
                        spans.push((format!("  retry in {secs}s"), Token::FgMuted));
                    }
                    push_row(
                        w,
                        row(
                            Marker::Failed,
                            Token::Hostile,
                            &module,
                            Token::Hostile,
                            spans,
                        ),
                    );
                }
                other => {
                    let mut spans = vec![(format!("{other}  ← "), Token::FgMuted)];
                    spans.extend(ent_span(&entity, g));
                    push_row(
                        w,
                        row(Marker::Blocked, Token::FgMuted, &module, Token::Fg, spans),
                    );
                }
            }
            w.stale.ego = true;
            w.stale.detail = true;
        }
        EventKind::JobBlocked { module, reason, .. } => {
            let spans = vec![(
                format!("{module} blocked {} {reason}", g.m(Marker::Dot)),
                Token::FgMuted,
            )];
            push_row(
                w,
                row(Marker::Blocked, Token::FgMuted, "rules", Token::Fg, spans),
            );
        }
        EventKind::JobCached { module, entity, .. } => {
            let mut spans = vec![("cached  ← ".to_string(), Token::FgMuted)];
            spans.extend(ent_span(&entity, g));
            push_row(
                w,
                row(Marker::Ok, Token::FgMuted, &module, Token::Fg, spans),
            );
        }
        EventKind::EntityAdded { entity, .. } => {
            w.fresh.insert(entity.id);
            w.stale.entities = true;
            w.stale.ego = true;
        }
        EventKind::EdgeAdded { .. } | EventKind::EdgeChanged { .. } => {
            w.stale.ego = true;
            w.stale.detail = true;
        }
        EventKind::Correlation { artifact, a, b, .. } => {
            let mut spans = vec![(format!("{}  ", artifact.label()), Token::Accent)];
            spans.extend(ent_span(&a, g));
            spans.push((format!(" {} ", g.m(Marker::Sym)), Token::FgMuted));
            spans.extend(ent_span(&b, g));
            push_row(
                w,
                row(
                    Marker::Correlated,
                    Token::Accent,
                    "correlate",
                    Token::Accent,
                    spans,
                ),
            );
            w.stale.ego = true;
        }
        EventKind::HubSuppressed { entity, .. } => {
            let mut spans = ent_span(&entity, g);
            spans.push(("  auto-expansion suppressed".into(), Token::FgMuted));
            push_row(w, row(Marker::Hub, Token::Warn, "hub", Token::Warn, spans));
            w.stale.entities = true;
        }
        EventKind::ReviewAdded { .. } | EventKind::Budget { .. } => {}
        EventKind::ModuleLog { module, level, msg } => {
            if matches!(
                level,
                spektr_engine::LogLevel::Warn | spektr_engine::LogLevel::Error
            ) {
                push_row(
                    w,
                    row(
                        Marker::Attention,
                        Token::Warn,
                        &module,
                        Token::Fg,
                        vec![(msg, Token::FgMuted)],
                    ),
                );
            }
        }
        EventKind::MessageRejected { module, reason, .. } => {
            push_row(
                w,
                row(
                    Marker::Attention,
                    Token::Warn,
                    &module,
                    Token::Fg,
                    vec![(format!("message rejected: {reason}"), Token::FgMuted)],
                ),
            );
        }
        EventKind::IngestFailed { message } => {
            push_row(
                w,
                row(
                    Marker::Failed,
                    Token::Hostile,
                    "ingest",
                    Token::Hostile,
                    vec![(message, Token::Hostile)],
                ),
            );
        }
        EventKind::CaseChanged { .. } => w.stale = Stale::all(),
        EventKind::PlaybookStarted {
            run,
            playbook,
            seed,
            parent,
        } => {
            if parent.is_none() {
                w.top_run = Some(run);
            }
            let mut spans = vec![
                (format!("{playbook}  "), Token::Fg),
                ("on ".into(), Token::FgMuted),
            ];
            spans.extend(ent_span(&seed, g));
            if parent.is_some() {
                spans.push(("  (nested)".into(), Token::FgMuted));
            }
            push_row(
                w,
                row(
                    Marker::Running,
                    Token::Accent,
                    "playbook",
                    Token::Accent,
                    spans,
                ),
            );
            w.stale.entities = true;
        }
        EventKind::PlaybookStep {
            playbook,
            step,
            of,
            title,
            state,
            ..
        } => {
            if state == "running" {
                w.playbook_line = Some(format!("{playbook} {} step {step}/{of}", g.m(Marker::Dot)));
            } else {
                let (m, t) = if state == "done" {
                    (Marker::Ok, Token::Ok)
                } else {
                    (Marker::Blocked, Token::FgMuted)
                };
                let spans = vec![
                    (
                        format!("{playbook} {} step {step}/{of}  ", g.m(Marker::Dot)),
                        Token::Fg,
                    ),
                    (title, Token::FgMuted),
                ];
                push_row(w, row(m, t, "playbook", Token::Fg, spans));
            }
            w.stale.ego = true;
        }
        EventKind::PlaybookFinished {
            run,
            playbook,
            state,
        } => {
            if w.top_run.is_none_or(|t| t == run) {
                w.top_run = None;
                w.playbook_line = None;
            }
            let (m, t) = match state.as_str() {
                "done" => (Marker::Ok, Token::Ok),
                "failed" => (Marker::Failed, Token::Hostile),
                _ => (Marker::Blocked, Token::FgMuted),
            };
            push_row(
                w,
                row(
                    m,
                    t,
                    "playbook",
                    Token::Fg,
                    vec![(format!("{playbook}  {state}"), Token::Fg)],
                ),
            );
        }
        EventKind::ChecklistAdded { count, .. } => {
            let spans = vec![(
                format!(
                    "+{count} checklist item{}",
                    if count == 1 { "" } else { "s" }
                ),
                Token::Fg,
            )];
            push_row(
                w,
                row(
                    Marker::Attention,
                    Token::Warn,
                    "checklist",
                    Token::Fg,
                    spans,
                ),
            );
            w.stale.detail = true;
        }
        EventKind::LauncherOpened {
            launcher, entity, ..
        } => {
            let mut spans = vec![(format!("{launcher}  "), Token::Fg)];
            spans.extend(ent_span(&entity, g));
            spans.push(("  (recorded)".into(), Token::FgMuted));
            push_row(
                w,
                row(Marker::Link, Token::Accent, "launcher", Token::Fg, spans),
            );
        }
    }
    if w.stale.any() {
        schedule(w, fx);
    }
}

fn query_result(s: &mut AppState, tag: Tag, r: Result<QueryResult, String>, fx: &mut Vec<Effect>) {
    let r = match r {
        Ok(r) => r,
        Err(e) => {
            toast(s, &e, Token::Hostile);
            return;
        }
    };
    if let (Tag::Find, QueryResult::Scope(rules)) = (&tag, &r) {
        let text = if rules.is_empty() {
            "no scope rules: everything not denied is in scope".to_string()
        } else {
            rules.join(" · ")
        };
        toast(s, &text, Token::Fg);
        return;
    }
    let s_glyphs = s.glyphs;
    let Some(w) = s.workspace_mut() else { return };
    match &tag {
        Tag::Runs | Tag::Playbooks | Tag::Checklist | Tag::Review | Tag::ReviewSide(..) => {
            if !screens::result(w, &tag, r, fx) {
                s.dirty = false;
            }
            return;
        }
        Tag::Timeline | Tag::Path(..) => {
            if !tabs::result(w, &tag, r, fx) {
                s.dirty = false;
            }
            return;
        }
        Tag::Edge(id) => {
            if let QueryResult::Edge(e) = r
                && w.inspect_edge == Some(*id)
            {
                w.edge = e.map(|b| *b);
            }
            return;
        }
        Tag::FindPath => {
            match r {
                QueryResult::Found(Some(e)) => tabs::path_to(w, e, fx),
                _ => toast(s, "not in the case", Token::FgMuted),
            }
            return;
        }
        _ => {}
    }
    match (tag, r) {
        (Tag::Status, QueryResult::Status(st)) => {
            let n = |k: &str| st.jobs.get(k).copied().unwrap_or(0);
            w.running = n("running");
            w.queued = n("queued");
            w.done = n("done");
            w.failed = n("failed");
            w.status = Some(*st);
        }
        (Tag::Modules, QueryResult::Modules(m)) => w.modules = m,
        (Tag::Jobs, QueryResult::Jobs(jobs)) => {
            w.past_jobs = Some(jobs.clone());
            seed_activity(w, s_glyphs);
            for j in jobs.into_iter().filter(|j| j.state == JobState::Done) {
                if let Some(t) = j.finished_at {
                    let k = (j.input, j.module.clone());
                    let e = w.last_run.entry(k).or_insert(t);
                    if *e < t {
                        *e = t;
                    }
                }
            }
        }
        (Tag::Entities, QueryResult::Entities(mut list)) => {
            entities::sort(&mut list);
            let keep = w.ent_cursor_entity().map(|e| e.id);
            w.entities = list;
            rebuild_entities(w);
            if let Some(id) = keep
                && let Some(i) = w
                    .ent_items
                    .iter()
                    .position(|it| matches!(it, EntItem::Row(i) if w.entities[*i].id == id))
            {
                w.ent_cursor = i;
            }
            seed_activity(w, s_glyphs);
            if w.focus.is_none() {
                initial_focus(w, fx);
            }
            if w.inspect.is_none() {
                sync_inspect(w, fx);
            }
        }
        (Tag::Ego { focus, generation }, QueryResult::Ego(view)) => {
            if w.focus != Some(focus) || w.ego_gen != generation {
                return;
            }
            // Keep the cursor on the same row (group and entity) when rows
            // re-sort, e.g. a confirmed edge moving to the top.
            let keep = w
                .ego
                .as_ref()
                .zip(w.ego_item())
                .and_then(|(e, it)| ego::row(e, it))
                .map(|(g, r)| (g.key.clone(), r.entity.id));
            w.ego = Some(view);
            if let (Some((key, id)), Some(e)) = (keep, &w.ego) {
                let at = ego::column_items(e, w.ego_col).into_iter().position(|it| {
                    ego::row(e, it).is_some_and(|(g, r)| g.key == key && r.entity.id == id)
                });
                if let Some(i) = at {
                    w.ego_idx = i;
                }
            }
            fix_ego_cursor(w);
            sync_inspect(w, fx);
        }
        (Tag::Detail(id), QueryResult::Detail(d)) => {
            // A menu opened before the detail arrived gets its count now.
            if let (Some(Overlay::Menu(m)), Some(d)) = (&mut s.overlay, &d)
                && m.target.id == id
            {
                let n = d.observations.len();
                m.subtitle = format!(
                    "{} {} {n} observation{}",
                    m.target.ty.as_str(),
                    s.glyphs.m(Marker::Dot),
                    if n == 1 { "" } else { "s" }
                );
            }
            let Some(w) = s.workspace_mut() else { return };
            if w.inspect == Some(id) {
                w.detail = d.map(|b| *b);
                let n = w.detail.as_ref().map_or(0, |d| d.observations.len());
                w.insp_cursor = w.insp_cursor.min(n.saturating_sub(1));
            }
        }
        (Tag::Find, QueryResult::Found(Some(e))) => {
            pivot(w, e, true, fx);
        }
        _ => s.dirty = false,
    }
}

/// Fill the rail with jobs from before this session, once both the jobs
/// and the entities they ran on are loaded.
fn seed_activity(w: &mut Workspace, g: crate::glyph::GlyphSet) {
    if w.entities.is_empty() {
        return;
    }
    let Some(mut jobs) = w.past_jobs.take() else {
        return;
    };
    jobs.sort_by_key(|j| std::cmp::Reverse(j.finished_at.unwrap_or(j.queued_at)));
    let mut rows = Vec::new();
    for j in jobs.iter().take(50) {
        let ent = w.entity(j.input).cloned();
        let at = j.finished_at.unwrap_or(j.queued_at);
        let mut target = Vec::new();
        if let Some(e) = &ent {
            target.push(("  ← ".to_string(), Token::FgMuted));
            target.extend(ent_span(e, g));
        }
        let row = match j.state {
            JobState::Done => {
                let mut spans = vec![("done".to_string(), Token::FgMuted)];
                spans.extend(target);
                ActRow {
                    at,
                    marker: Marker::Ok,
                    token: Token::Ok,
                    module: j.module.clone(),
                    module_token: Token::Fg,
                    spans,
                }
            }
            JobState::Failed => ActRow {
                at,
                marker: Marker::Failed,
                token: Token::Hostile,
                module: j.module.clone(),
                module_token: Token::Hostile,
                spans: vec![(
                    j.error.clone().unwrap_or_else(|| "failed".into()),
                    Token::Hostile,
                )],
            },
            _ => ActRow {
                at,
                marker: Marker::Blocked,
                token: Token::FgMuted,
                module: "rules".into(),
                module_token: Token::Fg,
                spans: vec![(
                    format!(
                        "{} blocked {} {}",
                        j.module,
                        g.m(Marker::Dot),
                        j.reason.clone().unwrap_or_default()
                    ),
                    Token::FgMuted,
                )],
            },
        };
        rows.push(row);
    }
    // Live rows (newest) stay on top of history.
    w.activity.extend(rows);
    w.activity.truncate(ACTIVITY_CAP);
}

/// The case's subject and first seed: `◎ Jordan Marlow › ~ jmarlow.wav`.
fn initial_focus(w: &mut Workspace, fx: &mut Vec<Effect>) {
    let subject = w
        .entities
        .iter()
        .find(|e| e.flags.contains(EntityFlags::SUBJECT))
        .cloned();
    let seed = w
        .entities
        .iter()
        .filter(|e| e.flags.contains(EntityFlags::SEED))
        .min_by_key(|e| e.depth)
        .cloned();
    match (subject, seed) {
        (Some(sub), Some(seed)) => {
            w.crumbs.push(sub);
            pivot(w, seed, true, fx);
        }
        (Some(e), None) | (None, Some(e)) => pivot(w, e, true, fx),
        (None, None) => {
            if let Some(e) = w.entities.first().cloned() {
                pivot(w, e, true, fx);
            }
        }
    }
    w.pane = Pane::Graph;
    if let Some(f) = w.focus
        && let Some(i) = w
            .ent_items
            .iter()
            .position(|it| matches!(it, EntItem::Row(i) if w.entities[*i].id == f))
    {
        w.ent_cursor = i;
    }
}

fn fix_ego_cursor(w: &mut Workspace) {
    let Some(ego) = &w.ego else { return };
    let n = |c| ego::column_items(ego, c).len();
    if w.ego_col != Col::Card && n(w.ego_col) == 0 {
        w.ego_col = if n(Col::Right) > 0 {
            Col::Right
        } else if n(Col::Left) > 0 {
            Col::Left
        } else {
            Col::Card
        };
        w.ego_idx = 0;
    }
    w.ego_idx = w.ego_idx.min(n(w.ego_col).saturating_sub(1));
}

fn acted(s: &mut AppState, what: &str, r: Result<ActionOutcome, String>, fx: &mut Vec<Effect>) {
    match r {
        Err(e) => {
            if let Some(Overlay::Dialog(d)) = &mut s.overlay {
                d.error = Some(e);
            } else {
                toast(s, &format!("{what}: {e}"), Token::Hostile);
            }
        }
        Ok(out) => {
            if matches!(s.overlay, Some(Overlay::Dialog(_))) {
                s.overlay = None;
            }
            let msg = match &out {
                ActionOutcome::Seeded {
                    entity,
                    created,
                    jobs,
                } => {
                    let e = entity.clone();
                    if let Some(w) = s.workspace_mut() {
                        w.stale = Stale::all();
                        schedule(w, fx);
                        pivot(w, e, true, fx);
                    }
                    format!(
                        "seeded{} · {} jobs",
                        if *created { "" } else { " (exists)" },
                        jobs.len()
                    )
                }
                ActionOutcome::Planned { jobs } => {
                    let run = jobs
                        .iter()
                        .filter(|j| {
                            matches!(
                                j.result,
                                spektr_engine::PlanResult::Queued { .. }
                                    | spektr_engine::PlanResult::Joined { .. }
                            )
                        })
                        .count();
                    if jobs.is_empty() {
                        format!("{what}: nothing eligible")
                    } else {
                        format!("{what}: {run} queued · {} total", jobs.len())
                    }
                }
                ActionOutcome::Merged { .. } => "merged".into(),
                ActionOutcome::PlaybookStarted { .. } => format!("{what}: started"),
                ActionOutcome::Launch { url } => {
                    fx.push(Effect::OpenUrl(url.clone()));
                    format!("{what}: opened in the browser")
                }
                ActionOutcome::Reloaded { warnings } => {
                    if warnings.is_empty() {
                        "reloaded".into()
                    } else {
                        format!("reloaded · {}", warnings.join(" · "))
                    }
                }
                ActionOutcome::Captured { .. } => "captured".into(),
                ActionOutcome::Attached { image, .. } => {
                    if image.is_some() {
                        "attached · image added".into()
                    } else {
                        "attached".into()
                    }
                }
                ActionOutcome::Decided { added } => match added {
                    Some(a) => format!("accepted · {} added", display_value(a, s.glyphs)),
                    None => format!("{what} ✓"),
                },
                ActionOutcome::Done => format!("{what} ✓"),
            };
            toast(s, &msg, Token::Fg);
            if let Some(w) = s.workspace_mut() {
                w.stale = Stale::all();
                schedule(w, fx);
            }
        }
    }
}
