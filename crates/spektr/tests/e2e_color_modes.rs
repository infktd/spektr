//! e2e_color_modes (QA.md §2): truecolor, 256-color (COLORTERM unset) and
//! NO_COLOR all render; cell attributes match TUI_SPEC §2.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use spektr_tui::theme::Token;
use support::Home;
use support::term::{Color, Screen, Size, Term};

const T: Duration = Duration::from_secs(15);

fn run(h: &Home, env: &[(&str, &str)]) -> Screen {
    let mut e = h.env();
    for (k, v) in env {
        e.retain(|(x, _)| x != k);
        e.push(((*k).into(), (*v).into()));
    }
    let mut t = Term::spawn(
        &["open", "demo.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &e,
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("has_account", T).unwrap();
    let s = t.settle(Duration::from_millis(200), T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
    s
}

fn rgb(t: Token) -> Color {
    let (r, g, b) = t.values().0;
    Color::Rgb(r, g, b)
}

/// The NORMAL pill's first cell and the graph pane's corner.
fn pill(s: &Screen) -> (u16, u16) {
    let (x, y) = s.find("NORMAL").unwrap();
    (x.saturating_sub(1), y)
}

#[test]
fn truecolor_256_and_no_color() {
    let h = Home::new();
    h.demo_case();

    let s = run(&h, &[]);
    let (px, py) = pill(&s);
    assert_eq!(s.bg_at(px, py), Some(rgb(Token::Accent)), "pill is accent");
    assert_eq!(s.bg_at(100, 30), Some(rgb(Token::Bg)), "ground is bg");
    assert_eq!(
        s.fg_at(38, 1),
        Some(rgb(Token::Accent)),
        "focused pane border"
    );
    assert_eq!(s.fg_at(0, 1), Some(rgb(Token::Border)), "idle pane border");
    let (cx, cy) = s.find("▌").unwrap();
    assert_eq!(
        s.bg_at(cx + 2, cy),
        Some(rgb(Token::Selection)),
        "cursor row background"
    );

    let s = run(&h, &[("COLORTERM", "")]);
    let (px, py) = pill(&s);
    assert_eq!(
        s.bg_at(px, py),
        Some(Color::Idx(Token::Accent.values().1)),
        "256-color pill"
    );
    assert_eq!(s.fg_at(0, 1), Some(Color::Idx(Token::Border.values().1)));

    let s = run(&h, &[("NO_COLOR", "1")]);
    let (px, py) = pill(&s);
    let c = s.cell(px, py).unwrap();
    assert!(c.inverse && c.bold, "mono pill is reverse + bold: {c:?}");
    let (cx, cy) = s.find("▌").unwrap();
    assert!(
        s.cell(cx + 2, cy).unwrap().inverse,
        "mono cursor row is reverse video"
    );
    for row in &s.cells {
        for c in row {
            assert!(
                matches!(c.fg, Color::Default) && matches!(c.bg, Color::Default),
                "no colors under NO_COLOR: {c:?}"
            );
        }
    }
    assert!(s.contains("◐ aboutme"), "glyphs kept under NO_COLOR");
}
