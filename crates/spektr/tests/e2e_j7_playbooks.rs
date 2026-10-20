//! e2e_j7_playbooks (QA.md §2, PRD J7 steps 1–7).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Screen, Size, Term};

const T: Duration = Duration::from_secs(30);

fn col_of(s: &Screen, text: &str) -> u16 {
    s.find(text)
        .unwrap_or_else(|| panic!("`{text}` not on screen:\n{}", s.contents))
        .0
}

#[test]
fn j7_playbooks() {
    let h = Home::new();
    let ok = |args: &[&str]| {
        let out = h.cmd(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", support::stderr(&out));
    };
    ok(&[
        "new",
        "tl",
        "--profile",
        "tracelabs",
        "--subject",
        "Jordan Marlow",
        "--last-seen",
        "2026-09-28",
    ]);
    // The subject has a known handle and email (seeds link to the subject).
    ok(&[
        "run",
        "tl.spektr",
        "--seed",
        "username:jmarlow.wav",
        "--seed",
        "email:jordan.marlow@mail.example",
    ]);

    let mut t = Term::spawn(
        &["open", "tl.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("● 0 running · 0 queued", T).unwrap();

    // 1. On ◎ Jordan Marlow, Run playbook ▸ tracelabs-kickoff.
    t.keys("/ text:Jordan ↵").unwrap();
    t.wait_for_text("/ Jordan ", T).unwrap();
    t.keys("p").unwrap();
    t.wait_for_text("Playbooks for ◎ person", T).unwrap();
    t.keys("↵").unwrap();
    // 2. The rail shows the run and its steps; nested runs appear.
    t.wait_for_text("playbook tracelabs-kickoff: started", T)
        .unwrap();
    t.keys("esc C").unwrap();
    t.wait_for_text("Playbooks 3 runs", T).unwrap();
    let s = t.wait_for_text("done 7/7", T).unwrap();
    let s = {
        let _ = s;
        t.wait_for_text("● 0 running · 0 queued", T).unwrap();
        t.settle(Duration::from_millis(300), T).unwrap()
    };
    let top = col_of(&s, "tracelabs-kickoff ");
    let user = col_of(&s, "person-from-username ");
    let email = col_of(&s, "person-from-email ");
    assert_eq!(user, top + 2, "nested runs are indented:\n{}", s.contents);
    assert_eq!(email, top + 2, "{}", s.contents);
    let (_, ty) = s.find("tracelabs-kickoff ").unwrap();
    let (_, uy) = s.find("person-from-username ").unwrap();
    assert!(uy > ty, "children come after the parent");

    // 3. Reverse-image items, one per avatar, and the category items.
    assert!(
        s.contains("Reverse image search  person-from-username · step 5"),
        "{}",
        s.contents
    );
    assert!(
        s.contains("Employment  tracelabs-kickoff · step 4"),
        "{}",
        s.contents
    );
    let avatars = s
        .contents
        .lines()
        .skip_while(|l| !l.contains("Reverse image search  person-from-username"))
        .skip(1)
        .take_while(|l| l.contains("○ ▨"))
        .count();
    assert!(avatars >= 3, "{avatars} avatar items:\n{}", s.contents);

    // 4. `o` on an avatar item: its launchers; Google Lens opens (test mode
    //    records it) and the open is audited.
    let (x, y) = s
        .find("Reverse image search  person-from-username")
        .unwrap();
    t.mouse_click(x + 4, y + 1).unwrap();
    t.keys("o").unwrap();
    t.wait_for_text("▌Google Lens", T).unwrap();
    let s = t.wait_for_text("TinEye", T).unwrap();
    assert!(s.contains("Yandex Images"));
    t.keys("↵").unwrap();
    t.wait_for_text("would open https://lens.google.com", T).unwrap();
    assert!(h.opened().contains("\"kind\":\"url\""), "{}", h.opened());
    assert!(h.opened().contains("lens.google.com"), "{}", h.opened());

    // 5. `d` marks it done with a note.
    t.keys("d").unwrap();
    t.wait_for_text("Mark done", T).unwrap();
    t.keys("text:same-photo-on-a-forum ↵").unwrap();
    t.wait_for_text("done ✓", T).unwrap();
    t.keys("]").unwrap(); // the done tab
    t.wait_for_text("[done]", T).unwrap();
    t.wait_for_text("· note", T).unwrap();
    // …and `f` turns the item into a finding: its entity is attached, its
    // note is the draft relevance.
    t.keys("f").unwrap();
    t.wait_for_text("New finding", T).unwrap();
    t.keys("↓ text:Avatar space text:found space text:elsewhere ↵")
        .unwrap();
    t.wait_for_text("F-1 created", T).unwrap();
    let s = t.wait_for_text("Supporting entities", T).unwrap();
    assert!(s.contains("FINDINGS"), "{}", s.contents);
    assert!(
        s.contains("same-photo-on-a-forum"),
        "the note is the relevance:\n{}",
        s.contents
    );
    assert!(
        s.contains("▨ "),
        "the avatar is a supporting entity:\n{}",
        s.contents
    );
    t.keys("esc C").unwrap();
    t.wait_for_text("CHECKLIST", T).unwrap();

    // 6. A user playbook appears in the context menu after :reload.
    let dir = h.config_dir().join("playbooks");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("forum-sweep.toml"),
        r#"[playbook]
name = "forum-sweep"
version = "0.1.0"
description = "Look for the handle on forums"
seed = ["username"]

[[step]]
title = "Sweep"
run = ["username-sweep"]
on = "seed"
"#,
    )
    .unwrap();
    t.keys(": text:reload ↵").unwrap();
    t.wait_for_text("reloaded", T).unwrap();
    t.wait_for_text("forum-sweep", T).unwrap(); // Available
    t.keys("esc / text:jmarlow.wav ↵ p").unwrap();
    let s = t.wait_for_text("Playbooks for ~ username", T).unwrap();
    assert!(s.contains("forum-sweep"), "{}", s.contents);
    assert!(s.contains("person-from-username"), "{}", s.contents);
    t.keys("esc").unwrap();
    t.wait_for_text("NORMAL", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    let started = h.audit_action("tl", "playbook.start");
    assert_eq!(
        started.len(),
        3,
        "the kickoff and two nested runs: {started:?}"
    );
    assert!(h.audit_action("tl", "playbook.step").len() >= 7);
    let opens = h.audit_action("tl", "launcher.open");
    assert_eq!(opens.len(), 1, "{opens:?}");
    assert!(opens[0].contains("lens"), "{opens:?}");
    let resolved = h.audit_action("tl", "checklist.resolve");
    assert!(
        resolved.iter().any(|p| p.contains("same-photo-on-a-forum")),
        "{resolved:?}"
    );

    // 7. `playbook check` rejects a playbook naming an unknown module.
    let bad = h.work().join("bad.toml");
    std::fs::write(
        &bad,
        "[playbook]\nname = \"bad\"\nversion = \"1.0.0\"\ndescription = \"x\"\nseed = [\"username\"]\n\n[[step]]\ntitle = \"x\"\nrun = [\"no-such-module\"]\non = \"seed\"\n",
    )
    .unwrap();
    let out = h.cmd(&["playbook", "check", "bad.toml"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        support::stdout(&out).contains("unknown module `no-such-module`"),
        "{}",
        support::stdout(&out)
    );
    let out = h
        .cmd(&[
            "playbook",
            "check",
            &dir.join("forum-sweep.toml").display().to_string(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", support::stdout(&out));
}
