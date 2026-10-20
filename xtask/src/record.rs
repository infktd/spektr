//! `cargo xtask record <module> <input>`: run one module live with the
//! Record transport (MODULES §10). Explicit opt-in: the input must be listed
//! in `qa/live-targets.toml` (DISPATCH rule 11), and recordings land in
//! `target/recordings/`, never in the fixture tree. Fixtures stay fictional;
//! rewrite a recording before committing anything from it.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::qa::LiveTargets;

pub fn run(root: &Path, args: &[&str]) -> Result<()> {
    let [module, input] = args else {
        bail!("usage: cargo xtask record <module> <type>:<value>");
    };
    let targets = LiveTargets::load(root)?;
    if !targets.allows(input) {
        bail!("{input} is not in qa/live-targets.toml; only approved targets are ever queried");
    }
    let out = root.join("target/recordings");
    let work = root.join("target/recordings/cases");
    std::fs::create_dir_all(&work)?;
    let case = work.join(format!("record-{}.spektr", std::process::id()));
    let bin = build_spektr(root)?;
    let status = Command::new(&bin)
        .args([
            "new",
            case.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("record"),
            "--profile",
            "tracelabs",
            "--dir",
        ])
        .arg(&work)
        .status()?;
    if !status.success() {
        bail!("spektr new failed");
    }
    let status = Command::new(&bin)
        .arg("run")
        .arg(&case)
        .args(["--seed", input, "--module", module, "--depth", "0"])
        .env("SPEKTR_TEST_MODE", "1")
        .env("SPEKTR_TRANSPORT", format!("record:{}", out.display()))
        .status()?;
    if !status.success() {
        bail!("spektr run failed");
    }
    println!(
        "recorded to {}",
        out.join(module).join("recorded.jsonl").display()
    );
    println!(
        "rewrite it with fictional data before adding anything to crates/spektr-modules/fixtures/"
    );
    Ok(())
}

pub fn build_spektr(root: &Path) -> Result<std::path::PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(root)
        .args(["build", "-q", "-p", "spektr"])
        .status()
        .context("cargo build")?;
    if !status.success() {
        bail!("building spektr failed");
    }
    Ok(root.join("target/debug/spektr"))
}
