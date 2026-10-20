//! e2e_tutor (QA.md §2): the whole tutor, offline, driven by each step's
//! own keys from the lesson script (`spektr_tui::tutor::LESSONS`). Progress
//! is saved and resumes; launchers are recorded, not opened.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use spektr_tui::tutor::LESSONS;
use support::Home;
use support::term::{Size, Term};

const T: Duration = Duration::from_secs(30);

/// The coach strip's first words of a goal (they're truncated at 170 cols).
fn head(goal: &str) -> String {
    goal.chars().take(40).collect()
}

fn drive(t: &mut Term, keys: &str) {
    for seg in keys.split('~') {
        let seg = seg.trim();
        if !seg.is_empty() {
            t.keys(seg).unwrap();
        }
        std::thread::sleep(Duration::from_millis(600));
    }
}

#[test]
fn the_whole_tutor() {
    let h = Home::new();
    // J1 step 2: the launcher shows the doctor and points at the tutor
    let mut t = Term::spawn(
        &[],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    let s = t.wait_for_text("New to spektr?", T).unwrap();
    assert!(
        s.contains("Doctor") && s.contains("spektr tutor"),
        "{}",
        s.contents
    );
    t.keys("q").unwrap();
    let _ = t.wait_exit(T);

    let mut t = Term::spawn(
        &["tutor"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("Lesson 1/10", T).unwrap();
    let s = t.wait_for_text("TUTOR", T).unwrap();
    assert!(s.contains("Move and focus"), "{}", s.contents);
    for (li, lesson) in LESSONS.iter().enumerate() {
        for (si, step) in lesson.steps.iter().enumerate() {
            // A step without keys completes on its own (the sweep, the
            // playbook): the next step's wait covers it.
            if step.keys.trim().is_empty() {
                continue;
            }
            let tag = format!("Lesson {}/{} · {}", li + 1, LESSONS.len(), lesson.title);
            t.wait_for_text(&tag, T)
                .unwrap_or_else(|e| panic!("lesson {} step {}: {e:#}", li + 1, si + 1));
            t.wait_for_text(&head(step.goal), T)
                .unwrap_or_else(|e| panic!("lesson {} step {} goal: {e:#}", li + 1, si + 1));
            drive(&mut t, step.keys);
        }
    }
    let s = t.wait_for_text("Tutor complete", T).unwrap();
    assert!(
        s.contains("filed a finding and exported it"),
        "{}",
        s.contents
    );
    // the launcher in lesson 8 was recorded, not opened
    assert!(
        h.opened().contains("lens.google.com")
            || h.opened().contains("yandex")
            || h.opened().contains("tineye"),
        "{}",
        h.opened()
    );
    t.keys("esc esc q").unwrap();
    let _ = t.wait_exit(T);

    // progress was saved: a second run resumes at the end; --reset restarts
    let state =
        std::fs::read_to_string(h.dir.path().join("home/.local/state/spektr/tutor.toml")).unwrap();
    assert!(
        state.contains(&format!("lesson = {}", LESSONS.len())),
        "{state}"
    );
    let mut t = Term::spawn(
        &["tutor"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("Tutor complete", T).unwrap();
    t.keys("q").unwrap();
    let _ = t.wait_exit(T);
    let mut t = Term::spawn(
        &["tutor", "--reset"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("Lesson 1/10", T).unwrap();
    t.keys("q").unwrap();
    let _ = t.wait_exit(T);
}
