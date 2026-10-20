//! Shared helpers for the binary's tests: a temp home with XDG dirs, and
//! the deterministic test environment (QA.md §1).

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

pub mod term;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

pub const CLOCK: &str = "2026-10-20T18:14:22Z";

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../spektr-modules/fixtures")
}

/// A scratch home: cases live in `work/`, XDG dirs under `home/`.
pub struct Home {
    pub dir: TempDir,
}

impl Home {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("work")).unwrap();
        Self { dir }
    }

    pub fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    /// `home/.config/spektr/config.toml`.
    pub fn write_config(&self, text: &str) {
        let dir = self.dir.path().join("home/.config/spektr");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), text).unwrap();
    }

    /// A generated case of about `n` entities in `work/<name>.spektr`
    /// (fictional data), recorded in recent.toml.
    pub fn synth_case(&self, name: &str, n: u64) -> spektr_store::synth::SynthCase {
        let path = self.case(name);
        let clock = spektr_model::Clock::Fixed(CLOCK.parse().unwrap());
        let opts = spektr_store::OpenOptions::new(clock, spektr_model::IdGen::seeded(1)).unwrap();
        let mut init = spektr_store::CaseInit::named(name);
        init.rules_profile = "tracelabs".into();
        let mut db = spektr_store::Db::create(&path, &init, opts).unwrap();
        db.write(|tx| spektr_store::synth::generate(tx, n, 7))
            .unwrap()
    }

    /// Break the audit chain of a case (row `seq`'s payload changes).
    pub fn tamper(&self, name: &str, seq: i64) {
        let c = rusqlite::Connection::open(self.case(name)).unwrap();
        c.execute(
            "UPDATE audit SET payload = payload || ' ' WHERE seq = ?1",
            [seq],
        )
        .unwrap();
    }

    /// The Phase 2 fixture case (`demo.spektr`): subject Jordan Marlow,
    /// seed username:jmarlow.wav run to quiescence against the fixtures.
    pub fn demo_case(&self) {
        let ok = |args: &[&str]| {
            assert!(
                self.cmd(args).output().unwrap().status.success(),
                "{args:?}"
            )
        };
        ok(&[
            "new",
            "demo",
            "--profile",
            "tracelabs",
            "--subject",
            "Jordan Marlow",
            "--last-seen",
            "2026-09-28",
        ]);
        ok(&["run", "demo.spektr", "--seed", "username:jmarlow.wav"]);
    }

    /// Copy `name.spektr` (and any -wal/-shm) from another home.
    pub fn copy_case(&self, from: &Home, name: &str) {
        for ext in ["spektr", "spektr-wal", "spektr-shm"] {
            let src = from.work().join(format!("{name}.{ext}"));
            if src.exists() {
                std::fs::copy(&src, self.work().join(format!("{name}.{ext}"))).unwrap();
            }
        }
    }

    /// recent.toml listing these cases (launcher tests).
    pub fn write_recent(&self, names: &[&str]) {
        let dir = self.dir.path().join("home/.local/state/spektr");
        std::fs::create_dir_all(&dir).unwrap();
        let mut t = String::new();
        for n in names {
            t.push_str(&format!(
                "[[case]]\npath = \"{}\"\nopened = \"{CLOCK}\"\n\n",
                self.case(n).display()
            ));
        }
        std::fs::write(dir.join("recent.toml"), t).unwrap();
    }

    pub fn case(&self, name: &str) -> PathBuf {
        self.work().join(format!("{name}.spektr"))
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let h = self.dir.path().join("home");
        vec![
            ("HOME".into(), h.display().to_string()),
            (
                "XDG_CONFIG_HOME".into(),
                h.join(".config").display().to_string(),
            ),
            (
                "XDG_STATE_HOME".into(),
                h.join(".local/state").display().to_string(),
            ),
            (
                "XDG_DATA_HOME".into(),
                h.join(".local/share").display().to_string(),
            ),
            ("SPEKTR_TEST_MODE".into(), "1".into()),
            (
                "SPEKTR_TRANSPORT".into(),
                format!("replay:{}", fixtures().display()),
            ),
            ("SPEKTR_CLOCK".into(), CLOCK.into()),
            ("SPEKTR_SEED".into(), "1".into()),
            ("NO_COLOR".into(), String::new()),
            ("COLORTERM".into(), "truecolor".into()),
            ("TERM".into(), "xterm-256color".into()),
        ]
    }

    /// `spektr <args>` in `work/` with the test environment.
    pub fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("spektr").unwrap();
        c.current_dir(self.work())
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default());
        for (k, v) in self.env() {
            if !v.is_empty() {
                c.env(k, v);
            }
        }
        c
    }
}

pub fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}
