//! Development tasks for spektr. Run with `cargo xtask <task>`.

mod deps_check;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <task>

tasks:
  deps-check    enforce crate boundaries and banned dependencies (ARCHITECTURE §2)

Further tasks (schemas, record, demos, qa, qa-shots) arrive with the phases that need them.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("deps-check") => deps_check::run(&workspace_root()),
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
