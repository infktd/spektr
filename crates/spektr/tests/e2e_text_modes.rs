//! e2e_text_modes (QA.md §2): in the palette, the filter and dialog inputs,
//! `q`, `F`, `1` and friends type characters instead of acting.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Size, Term};

const T: Duration = Duration::from_secs(15);
const KEYS: &str = "q F R C 1 2 3 ? : / j k space";
const TYPED: &str = "qFRC123?:/jk ";

#[test]
fn global_keys_type_in_text_inputs() {
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

    // COMMAND
    t.keys(":").unwrap();
    t.wait_for_text("COMMAND", T).unwrap();
    t.keys(KEYS).unwrap();
    let s = t
        .wait_for_text(&format!(": {}", TYPED.trim_end()), T)
        .unwrap();
    assert!(
        s.contains("COMMAND") && !s.contains("Navigation (lists"),
        "{}",
        s.contents
    );
    t.keys("esc").unwrap();
    t.wait_for(T, |s| !s.contains("COMMAND")).unwrap();

    // FILTER
    t.keys("/").unwrap();
    t.wait_for_text("FILTER", T).unwrap();
    t.keys(KEYS).unwrap();
    let s = t
        .wait_for_text(&format!("/ {}", TYPED.trim_end()), T)
        .unwrap();
    assert!(s.contains("FILTER"), "{}", s.contents);
    t.keys("esc").unwrap();
    t.wait_for(T, |s| !s.contains("FILTER")).unwrap();

    // A dialog's text field (the note dialog).
    t.keys("tab n").unwrap();
    t.wait_for_text("Note on", T).unwrap();
    t.keys(KEYS).unwrap();
    let s = t.wait_for_text(TYPED.trim_end(), T).unwrap();
    assert!(s.contains("Note on"), "{}", s.contents);
    t.keys("esc q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
}
