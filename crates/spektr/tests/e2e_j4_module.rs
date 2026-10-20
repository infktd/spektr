//! J4 (PRD §5): build a module. `module new` scaffolds a working exec
//! module; `module test` shows validated output (replaying its fixture, as
//! tests never touch the network); `module check` passes; `module install`
//! lists hosts and secrets and asks first; the module then appears in the
//! palette and the context menu. Plus pinning, enable and disable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::io::Write;
use std::time::Duration;

use support::term::{Size, Term};
use support::{Home, stdout};

const T: Duration = Duration::from_secs(20);

#[test]
fn j4_build_a_module() {
    let h = Home::new();
    let run = |args: &[&str]| h.cmd(args).output().unwrap();

    // 1. scaffold
    let out = run(&["module", "new", "lemmy", "--lang", "python"]);
    assert!(out.status.success(), "{}", support::stderr(&out));
    assert!(stdout(&out).contains("created lemmy (Python, standard library only)"));
    for f in [
        "module.toml",
        "main.py",
        "README.md",
        "fixtures/username_example.jsonl",
    ] {
        assert!(h.work().join("lemmy").join(f).exists(), "{f}");
    }

    // 2. validated output
    let out = run(&[
        "module",
        "test",
        "./lemmy",
        "--input",
        "username:example",
        "--fixture",
        "lemmy/fixtures/username_example.jsonl",
    ]);
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("✓ entity   account:lemmy:example"), "{text}");
    assert!(
        text.contains("✓ entity   url:https://example.com/u/example"),
        "{text}"
    );
    assert!(
        text.contains("✓ edge     username:example has_account account:lemmy:example"),
        "{text}"
    );
    assert!(text.contains("every message valid"), "{text}");
    // a live run refuses in test mode
    let out = run(&["module", "test", "./lemmy", "--input", "username:example"]);
    assert!(!out.status.success());

    // an invalid message is rejected and fails the test
    std::fs::write(
        h.work().join("bad.jsonl"),
        "{\"type\":\"entity\",\"job\":\"j\",\"ref\":\"a1\",\"entity\":{\"type\":\"account\",\"value\":\"lemmy:x\"},\"evidence\":[\"nope\"]}\n{\"type\":\"done\",\"job\":\"j\"}\n",
    )
    .unwrap();
    let out = run(&[
        "module",
        "test",
        "./lemmy",
        "--input",
        "username:x",
        "--fixture",
        "bad.jsonl",
    ]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(stdout(&out).contains("✕ rejected"), "{}", stdout(&out));

    // 3. check
    let out = run(&["module", "check", "./lemmy"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let text = stdout(&out);
    for line in ["✓ manifest", "✓ vocabulary", "✓ secrets", "✓ exit"] {
        assert!(text.contains(line), "{line}:\n{text}");
    }

    // 4. install: hosts and secrets shown; without a terminal it needs --yes
    let out = run(&["module", "install", "./lemmy"]);
    assert!(!out.status.success());
    assert!(
        support::stderr(&out).contains("needs confirmation"),
        "{}",
        support::stderr(&out)
    );
    let out = run(&["module", "install", "./lemmy", "--yes"]);
    assert!(out.status.success(), "{}", support::stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("hosts     example.com"), "{text}");
    assert!(text.contains("secrets   none"), "{text}");
    assert!(
        text.contains("installing one means trusting its code"),
        "{text}"
    );
    assert!(text.contains("✓ installed lemmy"), "{text}");
    let out = run(&["module", "list"]);
    assert!(
        stdout(&out).contains("lemmy                0.1.0    exec      passive  enabled"),
        "{}",
        stdout(&out)
    );

    // 5. the module is in the palette and the context menu
    let out = run(&["new", "j4", "--profile", "tracelabs"]);
    assert!(out.status.success());
    let mut t = Term::spawn(
        &["open", "j4.spektr"],
        Size {
            cols: 170,
            rows: 48,
        },
        &h.env(),
        &h.work(),
    )
    .unwrap();
    t.wait_for_text("NORMAL", T).unwrap();
    t.keys(": text:seed space text:username:example ↵").unwrap();
    t.wait_for_text("~ username", T).unwrap();
    t.keys("h : text:run space text:lem").unwrap();
    t.wait_for_text("run lemmy", T).unwrap();
    t.keys("esc space").unwrap();
    t.wait_for_text("Run module", T).unwrap();
    t.keys("r").unwrap();
    let s = t.wait_for_text("Modules for", T).unwrap();
    assert!(s.contains("lemmy"), "{}", s.contents);
    t.keys("esc").unwrap();
    t.keys("esc").unwrap();
    t.wait_for_text("NORMAL", T).unwrap();
    t.keys("q").unwrap();
    assert_eq!(t.wait_exit(T).unwrap(), 0);

    // pinning: a changed file disables it until re-approved
    let main = h.config_dir().join("modules/lemmy/main.py");
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&main)
        .unwrap();
    writeln!(f, "# changed").unwrap();
    let out = run(&["module", "list"]);
    assert!(
        stdout(&out).contains("changed · needs install --update"),
        "{}",
        stdout(&out)
    );
    assert!(
        run(&["module", "install", "--update", "lemmy", "--yes"])
            .status
            .success()
    );
    assert!(
        stdout(&run(&["module", "list"]))
            .contains("lemmy                0.1.0    exec      passive  enabled")
    );

    // enable / disable, built-ins too
    assert!(run(&["module", "disable", "wayback"]).status.success());
    assert!(
        stdout(&run(&["module", "list"]))
            .contains("wayback              1.0.0    built-in  passive  disabled")
    );
    assert!(run(&["module", "enable", "wayback"]).status.success());
    assert!(!run(&["module", "disable", "nope"]).status.success());
    let out = run(&["module", "info", "lemmy"]);
    assert!(
        stdout(&out).contains("command   python3 main.py"),
        "{}",
        stdout(&out)
    );
}
