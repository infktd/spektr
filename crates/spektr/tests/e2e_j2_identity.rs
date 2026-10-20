//! e2e_j2_identity (QA.md §2, PRD J2 steps 1–7 against fixtures): seed,
//! accounts streaming into the ego view, the avatar correlation, the edge
//! comparison with the dHash distance, confirm with `v`, the after-last-seen
//! date in the inspector and the timeline, the profile capture, a finding in
//! "Advancing the Timeline", its checks, and the submission text export.

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

    // 7. capture the profile (c on the github account), then f: a finding
    //    in "Advancing the Timeline"
    t.keys("1 / text:jmarlow-wav ↵").unwrap();
    t.wait_for_text("2 match", T).unwrap();
    t.settle(Duration::from_millis(300), T).unwrap();
    t.keys("c").unwrap();
    let s = t.wait_for_text("Capture", T).unwrap();
    assert!(
        s.contains("https://github.com/jmarlow-wav"),
        "the profile URL is prefilled:\n{}",
        s.contents
    );
    t.keys("↵").unwrap();
    t.wait_for_text("captured", T).unwrap();
    t.keys("f").unwrap();
    t.wait_for_text("New finding", T).unwrap();
    // the category: Advancing the Timeline is the eighth in tracelabs
    t.keys("→ → → → → → →").unwrap();
    t.wait_for_text("‹ Advancing the Timeline ›", T).unwrap();
    t.keys("↓ text:Subject's space text:GitHub space text:account space text:was space text:active space text:on space text:2026-10-16. ↵").unwrap();
    let s = t.wait_for_text("FINDINGS", T).unwrap();
    let _ = s;
    t.wait_for_text("F-1 created", T).unwrap();
    let s = t.wait_for_text("Supporting entities", T).unwrap();
    assert!(s.contains("Advancing the Timeline"), "{}", s.contents);
    assert!(s.contains("github · jmarlow-wav"), "{}", s.contents);
    assert!(s.contains("✕ write the relevance"), "{}", s.contents);
    assert!(s.contains("! needs staff coach approval"), "{}", s.contents);
    // e: write the relevance in the inline editor
    t.keys("e").unwrap();
    t.wait_for_text("INSERT", T).unwrap();
    t.keys("↓ text:Same space text:avatar space text:as space text:Bluesky; space text:after space text:2026-09-28. ↵").unwrap();
    t.wait_for_text("F-1 saved", T).unwrap();
    let s = t.wait_for_text("✓ claim and relevance written", T).unwrap();
    assert!(
        s.contains("✓ 1 source URL") || s.contains("capture"),
        "{}",
        s.contents
    );
    assert!(
        s.contains("✓ no news or aggregator sources"),
        "{}",
        s.contents
    );
    assert!(!s.contains("✕ "), "every check passes:\n{}", s.contents);

    // 8. checks pass: mark it ready, export the submission text
    t.keys("r").unwrap();
    t.wait_for_text("F-1 ready", T).unwrap();
    t.keys("x").unwrap();
    t.wait_for_text("Submission text", T).unwrap();
    t.keys("↵").unwrap();
    t.wait_for_text("copied 1 submission", T).unwrap();
    t.keys("esc q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);
    assert_eq!(h.audit_action("tl-ctf-1101", "edge.confirm").len(), 1);
    assert_eq!(h.audit_action("tl-ctf-1101", "finding.add").len(), 1);
    assert!(
        h.audit_action("tl-ctf-1101", "finding.update")
            .iter()
            .any(|p| p.contains("\"ready\""))
    );
    assert!(
        !h.audit_action("tl-ctf-1101", "finding.evidence.add")
            .is_empty(),
        "the capture is the finding's evidence"
    );

    // the same text headless
    let out = h
        .cmd(&["export", "tl-ctf-1101.spektr", "--format", "tracelabs"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", support::stderr(&out));
    let text = support::stdout(&out);
    assert!(
        text.contains("Category: Advancing the Timeline (1,000 pts)"),
        "{text}"
    );
    assert!(
        text.contains("Claim: Subject's GitHub account was active on 2026-10-16."),
        "{text}"
    );
    assert!(
        text.contains("URL: https://github.com/jmarlow-wav"),
        "{text}"
    );
    assert!(text.contains("Note: needs staff coach approval"), "{text}");
}
