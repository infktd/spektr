//! e2e_launcher (QA.md §2): launch with no case; recent list; doctor panel;
//! `n` creates a case; quit.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Size, Term};

const SIZE: Size = Size {
    cols: 132,
    rows: 40,
};
const T: Duration = Duration::from_secs(15);

#[test]
fn launcher_new_case_and_recent_list() {
    let h = Home::new();
    // Empty: no recent cases yet; the doctor panel is there.
    let mut t = Term::spawn(&[], SIZE, &h.env(), &h.work()).unwrap();
    let s = t.wait_for_text("no cases yet", T).unwrap();
    assert!(s.contains("s p e k t r"));
    t.wait_for_text("✓ config", T).unwrap();
    let s = t.wait_for_text("terminal", T).unwrap();
    assert!(s.contains("truecolor · 132×40"), "{}", s.contents);
    assert!(s.contains("↵ open   n new case   o open path   v verify chain   q quit"));

    // `n`: the dialog; type a name, ↵ creates and opens the case.
    t.keys("n").unwrap();
    t.wait_for_text("New case", T).unwrap();
    t.keys("text:kestrel-recon ↵").unwrap();
    let s = t.wait_for_text("NORMAL", T).unwrap();
    assert!(s.contains("kestrel-recon"), "{}", s.contents);
    assert!(h.case("kestrel-recon").exists());
    t.wait_for_text("no entities yet", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    // Relaunch: the case is listed with its counts and a verified chain.
    let mut t = Term::spawn(&[], SIZE, &h.env(), &h.work()).unwrap();
    let s = t
        .wait_for(T, |s| s.contains("kestrel-recon") && s.contains("✓"))
        .unwrap();
    let row = (0..s.rows)
        .find(|r| s.row_text(*r).contains("kestrel-recon"))
        .unwrap();
    assert!(s.row_text(row).contains("just now"), "{}", s.row_text(row));
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
}
