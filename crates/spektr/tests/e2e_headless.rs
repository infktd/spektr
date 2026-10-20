//! J6 (Phase 2 variant, PRD §5): `spektr run` with a username seed at depth
//! 2, then `spektr export --format json`, in a script with no TTY. Plus
//! `query --json` and the exit codes (ARCHITECTURE §10).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use serde_json::Value;
use support::*;

#[test]
fn j6_headless_run_then_export() {
    let h = Home::new();
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
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("created"), "{}", stdout(&out));

    let out = h
        .cmd(&[
            "run",
            "tl-ctf-1101.spektr",
            "--seed",
            "username:jmarlow.wav",
            "--depth",
            "2",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}\n{}",
        stdout(&out),
        stderr(&out)
    );
    let log = stderr(&out);
    assert!(
        log.contains("✓ username-sweep  307 sites · 7 found · 5 unsure · 295 none"),
        "{log}"
    );
    let summary = stdout(&out);
    assert!(summary.contains("review  5 pending"), "{summary}");
    assert!(summary.contains("chain   ✓"), "{summary}");

    let out = h
        .cmd(&["export", "tl-ctf-1101.spektr", "--format", "json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(doc["format"], "spektr-case/1");
    assert_eq!(doc["meta"]["rules_profile"], "tracelabs");
    assert_eq!(doc["meta"]["last_seen"], "2026-09-28");
    let accounts: Vec<&str> = doc["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "account")
        .map(|e| e["key"].as_str().unwrap())
        .collect();
    assert_eq!(accounts.len(), 8, "{accounts:?}");
    let has_account: Vec<&Value> = doc["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["predicate"] == "has_account")
        .collect();
    assert_eq!(has_account.len(), 8);
    assert!(
        has_account
            .iter()
            .all(|e| e["strength"] == "single" && !e["evidence"].as_array().unwrap().is_empty())
    );
    assert_eq!(doc["review_items"].as_array().unwrap().len(), 5);
    assert!(
        doc["observations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o.get("blob_b64").is_none())
    );

    // --with-blobs embeds evidence bytes
    let out = h
        .cmd(&[
            "export",
            "tl-ctf-1101.spektr",
            "--format",
            "json",
            "--with-blobs",
            "-o",
            "case.json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(h.work().join("case.json")).unwrap())
            .unwrap();
    assert!(
        doc["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o.get("blob_b64").is_some())
    );

    // re-running reuses cached results: the sweep's observation among them
    let out = h
        .cmd(&[
            "run",
            "tl-ctf-1101.spektr",
            "--seed",
            "username:jmarlow.wav",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("failed 0"), "{}", stdout(&out));
    assert!(!stdout(&out).contains("cached 0"), "{}", stdout(&out));
    assert!(
        stderr(&out).contains("✓ username-sweep  cached"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        h.cmd(&["verify", "tl-ctf-1101.spektr"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
}

#[test]
fn query_json_lines_parse() {
    let h = Home::new();
    assert!(h.cmd(&["new", "q"]).output().unwrap().status.success());
    assert!(
        h.cmd(&[
            "run",
            "q.spektr",
            "--seed",
            "username:jmarlow.wav",
            "--module",
            "username-sweep"
        ])
        .output()
        .unwrap()
        .status
        .success()
    );
    let out = h
        .cmd(&["query", "q.spektr", "--type", "account", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let lines: Vec<Value> = stdout(&out)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 7);
    assert!(lines.iter().all(|l| l["type"] == "account"));
    let out = h
        .cmd(&[
            "query",
            "q.spektr",
            "--neighbors",
            "username:jmarlow.wav",
            "--predicate",
            "has_account",
            "--json",
        ])
        .output()
        .unwrap();
    let rows: Vec<Value> = stdout(&out)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 7);
    assert!(
        rows.iter()
            .all(|r| r["strength"] == "single" && r["direction"] == "out")
    );
    let out = h
        .cmd(&[
            "query",
            "q.spektr",
            "--path",
            "username:jmarlow.wav",
            "domain:bsky.app",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(stdout(&out).lines().count(), 4, "{}", stdout(&out));
    let out = h
        .cmd(&["query", "q.spektr", "--match", "mastdn"])
        .output()
        .unwrap();
    assert!(
        stdout(&out).lines().next().unwrap().contains("mastodon"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn exit_codes() {
    let h = Home::new();
    // 2: usage
    assert_eq!(
        h.cmd(&["frobnicate"]).output().unwrap().status.code(),
        Some(2)
    );
    assert_eq!(h.cmd(&["run"]).output().unwrap().status.code(), Some(2));
    // 1: errors
    let out = h
        .cmd(&["run", "missing.spektr", "--seed", "username:x"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("missing.spektr not found"),
        "{}",
        stderr(&out)
    );
    assert!(h.cmd(&["new", "e"]).output().unwrap().status.success());
    let out = h.cmd(&["new", "e"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "case exists");
    let out = h
        .cmd(&["run", "e.spektr", "--seed", "phone"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = h
        .cmd(&["run", "e.spektr", "--seed", "email:not-an-email"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("invalid email"), "{}", stderr(&out));
    let out = h.cmd(&["new", "x", "--profile", "nope"]).output().unwrap();
    assert!(
        stderr(&out).contains("unknown rules profile `nope`"),
        "{}",
        stderr(&out)
    );
    // 0
    assert_eq!(
        h.cmd(&["verify", "e.spektr"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        h.cmd(&["doctor", "--json"]).output().unwrap().status.code(),
        Some(0)
    );
}

#[test]
fn testing_variables_need_test_mode() {
    let h = Home::new();
    let out = h
        .cmd(&["doctor"])
        .env_remove("SPEKTR_TEST_MODE")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("needs SPEKTR_TEST_MODE=1"),
        "{}",
        stderr(&out)
    );
    let out = h.cmd(&["--help"]).output().unwrap();
    assert!(stdout(&out).contains("Testing:"), "{}", stdout(&out));
}
