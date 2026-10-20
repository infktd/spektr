//! e2e_j3_rules (QA.md §2, PRD J3): under `tracelabs`, a user-installed
//! exec module of class `probing` never runs; the rail shows `⊘ rules …
//! blocked`, and the audit log has the reason.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::Duration;

use support::Home;
use support::term::{Size, Term};

const T: Duration = Duration::from_secs(20);

#[test]
fn probing_module_never_runs_under_tracelabs() {
    let h = Home::new();
    // A probing module that would leave a marker if it ever ran.
    let marker = h.work().join("probe-ran");
    let dir = h.config_dir().join("modules/email-account-probe");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("module.toml"),
        format!(
            r#"[module]
name        = "email-account-probe"
version     = "0.1.0"
description = "Submits signup forms to see whether an email has accounts (probing; J3 test only)"
runtime     = "exec"
command     = ["sh", "-c", "touch '{}'"]
technique   = "probing"
auto        = true
consumes    = ["email"]
produces    = [["has_email", "email"]]
"#,
            marker.display()
        ),
    )
    .unwrap();
    let out = h
        .cmd(&[
            "new",
            "tl-ctf-1101",
            "--profile",
            "tracelabs",
            "--subject",
            "Jordan Marlow",
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
    t.keys(": text:seed space text:email:jordan.marlow@mail.example ↵")
        .unwrap();
    let s = t
        .wait_for_text("email-account-probe blocked · tracelabs forbids probing", T)
        .unwrap();
    // the row's glyph and its `rules` source
    let row = s
        .contents
        .lines()
        .find(|l| l.contains("email-account-probe blocked"))
        .unwrap()
        .to_string();
    assert!(row.contains("⊘ rules"), "{row}");

    // and run by hand from the palette: still blocked
    t.keys("h : text:run space text:email-account-probe")
        .unwrap();
    let s = t.wait_for_text("tracelabs forbids probing", T).unwrap();
    assert!(s.contains("run email-account-probe"), "{}", s.contents);
    t.keys("esc").unwrap();
    t.wait_for_text("● 0 running · 0 queued", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    assert!(!marker.exists(), "the probing module ran");
    let audit = h.audit_like("tl-ctf-1101", "%forbids probing%");
    assert!(!audit.is_empty(), "no audit row with the reason");
    assert!(
        audit.iter().all(|a| a.contains("email-account-probe")),
        "{audit:?}"
    );
}
