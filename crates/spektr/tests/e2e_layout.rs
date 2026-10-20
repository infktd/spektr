//! e2e_layout (QA.md §2): the breakpoints (TUI_SPEC §4.1), the size floor,
//! and a live resize mid-session.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Screen, Size, Term};

const T: Duration = Duration::from_secs(15);

/// Column where a pane's top border starts (its `╭`).
fn corners(s: &Screen) -> Vec<u16> {
    (0..s.cols)
        .filter(|c| s.cell(*c, 1).is_some_and(|x| x.text == "╭"))
        .collect()
}

#[test]
fn breakpoints_and_live_resize() {
    let h = Home::new();
    h.demo_case();
    // (size, entities x, graph x, inspector x or none)
    let cases: [((u16, u16), Vec<u16>); 5] = [
        ((200, 60), vec![0, 38, 152]),
        ((170, 48), vec![0, 38, 122]),
        ((130, 40), vec![0, 32, 88]),
        ((120, 36), vec![0, 30]),
        ((100, 30), vec![0, 30]),
    ];
    for ((w, rows), want) in cases {
        let mut t = Term::spawn(
            &["open", "demo.spektr"],
            Size { cols: w, rows },
            &h.env(),
            &h.work(),
        )
        .unwrap();
        t.wait_for_text("has_account", T).unwrap();
        let s = t.settle(Duration::from_millis(200), T).unwrap();
        assert_eq!(corners(&s), want, "{w}×{rows}:\n{}", s.contents);
        assert!(
            s.row_text(rows - 1).contains("UTC"),
            "status line on the last row at {w}×{rows}"
        );
        assert!(
            s.row_text(rows - 9).contains("Activity"),
            "activity rail at H-9 at {w}×{rows}"
        );
        if want.len() == 2 {
            assert!(!s.contains("Inspector"));
            t.keys("i").unwrap();
            t.wait_for_text("Inspector", T).unwrap();
        }
        t.keys("q").unwrap();
        assert_eq!(t.wait_exit(T).unwrap(), 0);
    }

    let mut t = Term::spawn(
        &["open", "demo.spektr"],
        Size { cols: 99, rows: 30 },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("spektr needs at least 100×30 (now 99×30)", T)
        .unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    // Live resize: full layout → too small → narrow layout.
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
    t.resize(99, 30).unwrap();
    t.wait_for_text("(now 99×30)", T).unwrap();
    t.resize(120, 36).unwrap();
    let s = t
        .wait_for(T, |s| {
            s.contains("has_account") && !s.contains("needs at least")
        })
        .unwrap();
    assert_eq!(corners(&s), vec![0, 30]);
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
}
