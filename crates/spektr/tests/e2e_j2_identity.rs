//! e2e_j2_identity (QA.md §2, PRD J2 steps 1–7 against fixtures): seed,
//! accounts streaming into the ego view, the avatar correlation, the edge
//! comparison with the dHash distance, confirm with `v`, the after-last-seen
//! date in the inspector and the timeline, and a capture. Step 7's finding
//! and step 8's export wait for Phase 5.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Size, Term};

const T: Duration = Duration::from_secs(30);

#[test]
fn j2_identity_case() {
    let h = Home::new();
    // 1. the case
    let out = h
        .cmd(&[
            "new",
            "tl-ctf-1101",
            "--profile",
            "tracelabs",
            "--subject",
            "Jordan Marlow",
            "--last-seen",
            "2026-09-28",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", support::stderr(&out));
    let mut t = Term::spawn(
        &["open", "tl-ctf-1101.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("NORMAL", T).unwrap();

    // 2–3. seed; the sweep's accounts stream into the ego view
    t.keys(": text:seed space text:username:jmarlow.wav ↵")
        .unwrap();
    t.wait_for_text("has_account", T).unwrap();
    t.wait_for_text("◐ github", T).unwrap();

    // 4. avatar hashes correlate three accounts
    let s = t.wait_for_text("≈ same_avatar ↔ 3", T).unwrap();
    let _ = s;
    let s = t.wait_for_text("● 0 running · 0 queued", T).unwrap();
    let s = {
        let _ = s;
        t.settle(Duration::from_millis(300), T).unwrap()
    };

    // 5. the cursor on a correlated row: the inspector compares the edge
    let (x, y) = s.find("≈ same_avatar ↔ 3").unwrap();
    t.mouse_click(x + 4, y + 1).unwrap();
    let s = t.wait_for_text("dHash distance", T).unwrap();
    let insp = s.contents.clone();
    assert!(insp.contains("≈ same_avatar"), "{insp}");
    assert!(insp.contains("Strength"), "{insp}");
    assert!(!s.contains("confirmed by the analyst"));
    t.keys("v").unwrap();
    t.wait_for_text("confirmed by the analyst", T).unwrap();
    t.wait_for_text("▰▰▰ confirmed", T).unwrap();

    // 6. GitHub's last-active date is after last seen: the inspector…
    t.keys("/ text:jmarlow-wav ↵").unwrap();
    t.wait_for_text("▲ after last-seen date", T).unwrap();
    // …and the timeline
    t.keys("esc 2").unwrap();
    let s = t.wait_for_text("─ last seen 2026-09-28 ─", T).unwrap();
    let after = s
        .contents
        .lines()
        .skip_while(|l| !l.contains("─ last seen 2026-09-28 ─"))
        .skip(1)
        .find(|l| l.contains("last active") && l.contains("github"))
        .unwrap_or_else(|| {
            panic!(
                "github's last active isn't after the divider:\n{}",
                s.contents
            )
        })
        .to_string();
    assert!(after.contains("▲"), "{after}");

    // 7. capture the profile's site (the finding waits for Phase 5)
    t.keys("1 : text:capture space text:https://jmarlow.example/ ↵")
        .unwrap();
    t.wait_for_text("captured", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
    assert_eq!(h.audit_action("tl-ctf-1101", "edge.confirm").len(), 1);
    assert_eq!(
        h.audit_action("tl-ctf-1101", "observation.add")
            .iter()
            .filter(|p| p.contains("\"capture\""))
            .count(),
        1
    );
}
