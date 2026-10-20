//! tmux (QA.md §5): spektr runs inside a detached tmux session, driven with
//! `send-keys` and read back with `capture-pane -e -p`. Every binding works
//! there (no Kitty-protocol chords, TUI_SPEC §6), and the doctor warns when
//! tmux's mouse is off. Skipped when tmux isn't installed, unless
//! `SPEKTR_REQUIRE_TMUX=1` (CI sets it).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::process::Command;
use std::time::{Duration, Instant};

use support::Home;

struct Tmux {
    socket: String,
}

impl Tmux {
    fn cmd(&self, args: &[&str]) -> std::process::Output {
        Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .args(args)
            .output()
            .unwrap()
    }

    fn capture(&self) -> String {
        String::from_utf8_lossy(&self.cmd(&["capture-pane", "-e", "-p", "-t", "spk"]).stdout)
            .into_owned()
    }

    fn wait_for(&self, text: &str) -> String {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let s = self.capture();
            if s.contains(text) {
                return s;
            }
            assert!(
                Instant::now() < end,
                "waiting for `{text}` in tmux; pane:\n{s}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// One `send-keys` call. `Escape` must go alone: followed at once by
    /// another key it reads as Alt-<key>.
    fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", "spk"];
        args.extend_from_slice(keys);
        assert!(self.cmd(&args).status.success());
        std::thread::sleep(Duration::from_millis(250));
    }
}

impl Drop for Tmux {
    fn drop(&mut self) {
        let _ = self.cmd(&["kill-server"]);
    }
}

fn have_tmux() -> bool {
    let ok = Command::new("tmux")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        assert!(
            std::env::var("SPEKTR_REQUIRE_TMUX").as_deref() != Ok("1"),
            "SPEKTR_REQUIRE_TMUX=1 but tmux isn't installed"
        );
        eprintln!("skipped: tmux isn't installed");
    }
    ok
}

/// `env K=V … spektr <args>`, run in the work dir.
fn command_line(h: &Home, args: &[&str]) -> String {
    let bin = assert_cmd::cargo::cargo_bin("spektr");
    // tmux's own TERM and TMUX (the doctor's check reads it) pass through.
    let mut parts = vec![format!(
        "cd '{}' && exec env -i PATH=\"$PATH\" TERM=\"$TERM\" TMUX=\"$TMUX\"",
        h.work().display()
    )];
    for (k, v) in h.env() {
        if !v.is_empty() && k != "TERM" {
            parts.push(format!("{k}='{v}'"));
        }
    }
    parts.push(format!("'{}'", bin.display()));
    parts.extend(args.iter().map(|a| format!("'{a}'")));
    parts.join(" ")
}

#[test]
fn spektr_inside_tmux() {
    if !have_tmux() {
        return;
    }
    let h = Home::new();
    h.demo_case();
    let t = Tmux {
        socket: format!("spektr-test-{}", std::process::id()),
    };
    let line = command_line(&h, &["open", "demo.spektr"]);
    let out = t.cmd(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "spk",
        "-x",
        "170",
        "-y",
        "48",
        &line,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // mouse off (tmux's default): the doctor should warn
    let s = t.wait_for("has_account");
    assert!(s.contains("\x1b["), "capture-pane -e keeps the styles");
    assert!(s.contains("NORMAL"));
    // single keys, a two-key chord (`gg`), tab, the context menu, a screen
    t.keys(&["j", "j", "g", "g"]);
    t.keys(&["Space"]);
    t.wait_for("Pivot here");
    t.keys(&["Escape"]);
    t.keys(&["C"]);
    t.wait_for("CHECKLIST");
    t.keys(&["Escape"]);
    t.keys(&["2"]);
    t.wait_for("[2 timeline]");
    t.keys(&["1", ":"]);
    t.wait_for("COMMAND");
    t.keys(&["Escape"]);
    t.keys(&["q"]);
    let end = Instant::now() + Duration::from_secs(10);
    while t.cmd(&["has-session", "-t", "spk"]).status.success() {
        assert!(
            Instant::now() < end,
            "spektr didn't quit inside tmux:\n{}",
            t.capture()
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    // the doctor inside tmux, with tmux's mouse off
    let report = h.work().join("doctor.txt");
    let line = format!(
        "{} > '{}' 2>&1",
        command_line(&h, &["doctor"]),
        report.display()
    );
    let out = t.cmd(&[
        "-f",
        "/dev/null",
        "new-session",
        "-d",
        "-s",
        "spk",
        "-x",
        "120",
        "-y",
        "40",
        &line,
    ]);
    assert!(out.status.success());
    let end = Instant::now() + Duration::from_secs(20);
    while t.cmd(&["has-session", "-t", "spk"]).status.success() {
        assert!(Instant::now() < end, "doctor didn't finish");
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains("tmux"), "{text}");
    assert!(text.contains("tmux set -g mouse on"), "{text}");
}
