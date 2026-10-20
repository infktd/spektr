//! Development tasks for spektr. Run with `cargo xtask <task>`.

mod deps_check;
mod qa;
mod record;
mod schemas;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <task>

tasks:
  deps-check               enforce crate boundaries and banned dependencies (ARCHITECTURE §2)
  schemas [--check]        regenerate (or check) schemas/exec-protocol.v1.json and schemas/manifest.v1.json
  record <module> <input>  record live fixtures for one module (targets in qa/live-targets.toml only)
  qa [--live]              run the QA scenarios and save frames to qa/out/ (--live: approved targets only)
  qa-shots                 render qa/out frames and docs/design artboards to PNG with headless Chrome

`demos` arrives with the demo suite (Phase 5).";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("deps-check") => deps_check::run(&root),
        Some("schemas") => schemas::run(&root, rest.contains(&"--check")),
        Some("record") => record::run(&root, &rest),
        Some("qa") => qa::run(&root, rest.contains(&"--live")),
        Some("qa-shots") => qa::shots(&root),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("unknown task `{other}`\n\n{USAGE}");
            return ExitCode::from(2);
        }
        None => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root: xtask lives one level below it.
fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // `xtask/` always has a parent (the workspace root), so this cannot fail;
    // fall back to the manifest dir rather than panicking.
    manifest_dir.parent().unwrap_or(manifest_dir).to_path_buf()
}
