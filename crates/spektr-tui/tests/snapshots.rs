//! Every screen and overlay at 170×48 and 120×36 (TUI_SPEC §11), rendered
//! with a `TestBackend`-sized buffer and snapshotted as text with `insta`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use spektr_tui::msg::Msg;
use spektr_tui::state::AppState;
use spektr_tui::theme::ColorMode;
use spektr_tui::update::update;

const SIZES: [(u16, u16); 2] = [(170, 48), (120, 36)];

fn text(s: &AppState) -> String {
    let area = Rect::new(0, 0, s.size.0, s.size.1);
    let mut buf = Buffer::empty(area);
    spektr_tui::view::render_buf(&mut buf, area, s);
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        let mut skip = 0;
        for x in 0..area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let sym = buf[(x, y)].symbol();
            skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
            line.push_str(sym);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn keys(s: &mut AppState, ks: &str) {
    for k in ks.split_whitespace() {
        let code = match k {
            "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "down" => KeyCode::Down,
            "right" => KeyCode::Right,
            "space" => KeyCode::Char(' '),
            k => KeyCode::Char(k.chars().next().unwrap()),
        };
        update(s, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        // Hit regions come from the last frame, as in the runtime.
        let area = Rect::new(0, 0, s.size.0, s.size.1);
        let mut buf = Buffer::empty(area);
        s.hits = spektr_tui::view::render_buf(&mut buf, area, s);
    }
}

fn snap(name: &str, build: impl Fn((u16, u16)) -> AppState) {
    for size in SIZES {
        let s = build(size);
        insta::assert_snapshot!(format!("{name}_{}x{}", size.0, size.1), text(&s));
    }
}

fn ws(size: (u16, u16)) -> AppState {
    support::workspace(ColorMode::TrueColor, size)
}

#[test]
fn launcher() {
    snap("launcher", support::launcher);
}

#[test]
fn launcher_new_case_dialog() {
    snap("launcher_new_case", |size| {
        let mut s = support::launcher(size);
        keys(&mut s, "n");
        s
    });
}

#[test]
fn workspace() {
    snap("workspace", ws);
}

#[test]
fn workspace_entities_focused_and_filtered() {
    snap("workspace_filter", |size| {
        let mut s = ws(size);
        keys(&mut s, "/ m a r");
        s
    });
}

#[test]
fn workspace_empty_case() {
    snap("workspace_empty", |size| {
        let mut s = AppState::new(support::settings(ColorMode::TrueColor), support::now());
        s.size = size;
        s.screen = spektr_tui::state::Screen::Workspace(Box::new(
            spektr_tui::state::Workspace::new("/home/jm/cases/new.spektr".into()),
        ));
        s
    });
}

#[test]
fn palette() {
    snap("palette", |size| {
        let mut s = ws(size);
        keys(&mut s, ": r u n");
        s
    });
}

#[test]
fn context_menu_and_submenu() {
    snap("menu", |size| {
        let mut s = ws(size);
        keys(&mut s, "space");
        s
    });
    snap("submenu", |size| {
        let mut s = ws(size);
        keys(&mut s, "r");
        s
    });
}

#[test]
fn help() {
    snap("help", |size| {
        let mut s = ws(size);
        keys(&mut s, "?");
        s
    });
}

#[test]
fn dialogs() {
    snap("dialog_reject", |size| {
        let mut s = ws(size);
        keys(&mut s, "x");
        s
    });
    snap("dialog_note", |size| {
        let mut s = ws(size);
        keys(&mut s, "n");
        s
    });
}

#[test]
fn narrow_inspector_overlay_and_size_floor() {
    let mut s = ws((120, 36));
    keys(&mut s, "i");
    insta::assert_snapshot!("inspector_overlay_120x36", text(&s));
    let s = ws((99, 30));
    insta::assert_snapshot!("too_small_99x30", text(&s));
}

fn pbs(size: (u16, u16)) -> AppState {
    let mut s = ws(size);
    support::playbook_screens(&mut s);
    s
}

#[test]
fn checklist_screen() {
    snap("checklist", |size| {
        let mut s = pbs(size);
        keys(&mut s, "C tab");
        s
    });
}

#[test]
fn review_screen() {
    snap("review", |size| {
        let mut s = pbs(size);
        keys(&mut s, "R");
        s
    });
}

#[test]
fn timeline_and_path_tabs() {
    snap("timeline", |size| {
        let mut s = pbs(size);
        keys(&mut s, "2 j j j");
        s
    });
    snap("path", |size| {
        let mut s = pbs(size);
        keys(&mut s, "3");
        s
    });
}

#[test]
fn inspector_edge_comparison() {
    snap("edge_compare", |size| {
        let mut s = pbs(size);
        if let spektr_tui::state::Screen::Workspace(w) = &mut s.screen {
            w.inspect_edge = w.edge.as_ref().map(|e| e.edge);
        }
        s
    });
}

fn fnd(size: (u16, u16)) -> AppState {
    let mut s = ws(size);
    support::findings_screen(&mut s);
    s
}

#[test]
fn findings_screen_and_editor() {
    snap("findings", |size| {
        let mut s = fnd(size);
        keys(&mut s, "F");
        s
    });
    snap("findings_editor", |size| {
        let mut s = fnd(size);
        keys(&mut s, "F e");
        s
    });
    snap("findings_new_dialog", |size| {
        let mut s = fnd(size);
        keys(&mut s, "F n");
        s
    });
}
