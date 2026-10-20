//! The PTY harness itself: the real binary in a pseudo-terminal, screen
//! read back with colors, frames saved.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::term::{Size, Term};
use support::*;

#[test]
fn headless_run_in_a_pty_and_frames() {
    let h = Home::new();
    assert!(h.cmd(&["new", "pty"]).output().unwrap().status.success());
    let mut t = Term::spawn(
        &[
            "run",
            "pty.spektr",
            "--seed",
            "username:jmarlow.wav",
            "--module",
            "username-sweep",
        ],
        Size {
            cols: 120,
            rows: 36,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    assert_eq!(t.wait_exit(Duration::from_secs(30)).unwrap(), 0);
    let s = t
        .wait_for_text("chain   ✓", Duration::from_secs(5))
        .unwrap();
    assert!(
        s.contains("✓ username-sweep  307 sites · 7 found · 5 unsure · 295 none"),
        "{}",
        s.contents
    );
    let stem = h.work().join("frames/run");
    t.save_frame(&stem).unwrap();
    let html = std::fs::read_to_string(stem.with_extension("html")).unwrap();
    assert!(html.contains("JetBrains Mono") && html.contains("username-sweep"));
    assert!(
        std::fs::read_to_string(stem.with_extension("txt"))
            .unwrap()
            .contains("7 found")
    );
    assert!(
        !std::fs::read(stem.with_extension("ansi"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn key_dsl_and_resize_do_not_break_a_cli_command() {
    let h = Home::new();
    let mut t = Term::spawn(
        &["doctor"],
        Size {
            cols: 100,
            rows: 30,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.resize(120, 36).unwrap();
    t.keys("j k ↵ esc text:hello ctrl-l").unwrap();
    assert_eq!(t.wait_exit(Duration::from_secs(10)).unwrap(), 0);
    let s = t.wait_for_text("Doctor", Duration::from_secs(5)).unwrap();
    assert_eq!(s.cols, 120);
}
