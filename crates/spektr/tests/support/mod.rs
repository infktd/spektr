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
