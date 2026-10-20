//! e2e_mouse (QA.md §2): click focus, double-click pivot, wheel scroll,
//! right-click menu, submenu, click outside to close; tabs and the pill.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use spektr_tui::theme::Token;
use support::Home;
use support::term::{Color, Screen, Size, Term};

const T: Duration = Duration::from_secs(15);

fn accent() -> Color {
    let (r, g, b) = Token::Accent.values().0;
    Color::Rgb(r, g, b)
}

fn find(s: &Screen, text: &str) -> (u16, u16) {
    s.find(text)
        .unwrap_or_else(|| panic!("`{text}` not on screen:\n{}", s.contents))
}

fn settle(t: &Term) -> Screen {
    t.settle(Duration::from_millis(200), T).unwrap()
}

#[test]
fn mouse_maps_onto_the_same_actions() {
    let h = Home::new();
    h.demo_case();
    let mut t = Term::spawn(
        &["open", "demo.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("has_account", T).unwrap();
    let s = settle(&t);
    // The graph pane starts focused: its top-left corner is accent.
    assert_eq!(s.fg_at(38, 1), Some(accent()));
    assert_ne!(s.fg_at(0, 1), Some(accent()));

    // Click a row in the entities pane: focus moves there.
    let (x, y) = find(&s, "Jordan Marlow    ");
    t.mouse_click(x + 2, y).unwrap();
    let s = t.wait_changed(&s, T).unwrap();
    let s2 = settle(&t);
    assert_eq!(
        s2.fg_at(0, 1),
        Some(accent()),
        "entities border is accent after a click"
    );
    let _ = s;

    // Double-click an outbound row: pivot.
    let (x, y) = find(&s2, "bluesky   jmarlow");
    t.double_click(x, y).unwrap();
    let s = t.wait_for_text("›  ◐ bluesky", T).unwrap();

    // Wheel over the graph moves the cursor (back on the username first).
    t.keys("⌫").unwrap();
    t.wait_for_text("8 has_account", T).unwrap();
    let s = {
        let _ = s;
        settle(&t)
    };
    let (x, y) = find(&s, "gitlab    jmarlow");
    t.wheel(x, y, true).unwrap();
    let s = t.wait_changed(&s, T).unwrap();

    // Right-click a row: the context menu, anchored at the click.
    let s = {
        let _ = s;
        settle(&t)
    };
    let (x, y) = find(&s, "gitlab    jmarlow");
    t.right_click(x, y).unwrap();
    let s = t.wait_for_text("Pivot here", T).unwrap();
    let (mx, my) = find(&s, "Pivot here");
    assert!(my > y, "menu opens below the click");
    let _ = mx;

    // Click `Run module`: the submenu.
    let (rx, ry) = find(&s, "Run module");
    t.mouse_click(rx, ry).unwrap();
    t.wait_for_text("Modules for", T).unwrap();

    // Click outside: both close.
    t.mouse_click(2, 44).unwrap();
    let s = t.wait_for(T, |s| !s.contains("Pivot here")).unwrap();

    // Click a tab label, then the mode pill (help).
    let (tx, ty) = find(&s, "2 timeline");
    t.mouse_click(tx, ty).unwrap();
    t.wait_for_text("[2 timeline]", T).unwrap();
    let (px, py) = find(&settle(&t), "NORMAL");
    t.mouse_click(px, py).unwrap();
    t.wait_for_text("Navigation (lists and ego view)", T)
        .unwrap();
    t.keys("esc q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
}

#[test]
fn mouse_off_disables_capture() {
    let h = Home::new();
    h.demo_case();
    h.write_config("mouse = false\n");
    let mut t = Term::spawn(
        &["open", "demo.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("has_account", T).unwrap();
    let s = settle(&t);
    let (x, y) = find(&s, "bluesky ");
    t.double_click(x, y).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        !t.screen().contains("›  ◐ bluesky"),
        "no pivot with the mouse off"
    );
    // `:mouse on` turns it back on.
    t.keys(": text:mouse space text:on ↵").unwrap();
    t.wait_for_text("mouse on", T).unwrap();
    t.double_click(x, y).unwrap();
    t.wait_for_text("›  ◐ bluesky", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
}
