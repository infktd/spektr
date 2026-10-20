//! `spektr playbook list | info | check | new` (PLAYBOOKS §5).

use anyhow::{Context, Result, bail};
use spektr_engine::{CaseInit, EngineHandle, PlaybookInfo, Query, QueryResult};

use crate::cli::PlaybookCmd;
use crate::wiring::{Ctx, Modules};

/// A throwaway case to validate against the installed modules, launchers
/// and playbooks.
async fn scratch(ctx: &Ctx) -> Result<(EngineHandle, std::path::PathBuf)> {
    let path = std::env::temp_dir().join(format!("spektr-playbook-{}.spektr", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let h = ctx
        .create(&path, &CaseInit::named("playbook-check"), Modules::All)
        .await?;
    Ok((h, path))
}

fn cleanup(path: &std::path::Path) {
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
}

async fn infos(h: &EngineHandle) -> Result<Vec<PlaybookInfo>> {
    match h.query(Query::Playbooks).await? {
        QueryResult::Playbooks(p) => Ok(p),
        _ => bail!("unexpected reply"),
    }
}

/// Returns the exit status: false when a checked file has errors.
pub async fn run(ctx: &Ctx, cmd: PlaybookCmd) -> Result<bool> {
    match cmd {
        PlaybookCmd::List => {
            let (h, path) = scratch(ctx).await?;
            let list = infos(&h).await;
            cleanup(&path);
            for p in list? {
                let src = if p.source == "built-in" {
                    "built-in".to_string()
                } else {
                    crate::paths::tilde(std::path::Path::new(&p.source))
                };
                let profile = if p.requires_profile.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", p.requires_profile)
                };
                let problems = if p.errors.is_empty() {
                    String::new()
                } else {
                    format!(
                        "  ✕ {} problem{}",
                        p.errors.len(),
                        if p.errors.len() == 1 { "" } else { "s" }
                    )
                };
                println!(
                    "{:<22} {:<14} {src}{profile}{problems}",
                    p.name,
                    p.seed.join(",")
                );
            }
            Ok(true)
        }
        PlaybookCmd::Info { name } => {
            let (h, path) = scratch(ctx).await?;
            let list = infos(&h).await;
            cleanup(&path);
            let Some(p) = list?.into_iter().find(|p| p.name == name) else {
                bail!("unknown playbook `{name}`");
            };
            println!("{} {} · {}", p.name, p.version, p.description);
            println!(
                "  seed {}{}",
                p.seed.join(", "),
                if p.requires_profile.is_empty() {
                    String::new()
                } else {
                    format!(" · requires the {} profile", p.requires_profile)
                }
            );
            for (i, s) in p.steps.iter().enumerate() {
                println!(
                    "  {} {:<9} {:<34} on {}",
                    i + 1,
                    s.kind,
                    crate::fmt::trunc(&s.title, 34),
                    s.on
                );
                println!("              {}", crate::fmt::trunc(&s.detail, 90));
            }
            for e in &p.errors {
                println!("  ✕ {e}");
            }
            Ok(true)
        }
        PlaybookCmd::Check { file } => {
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let parsed = match spektr_protocol::PlaybookFile::from_toml(&text) {
                Ok(p) => p,
                Err(e) => {
                    println!("✕ {}: {e}", file.display());
                    return Ok(false);
                }
            };
            let (h, path) = scratch(ctx).await?;
            let problems = h.check_playbook(&parsed);
            cleanup(&path);
            for w in &problems.warnings {
                println!("! {w}");
            }
            for e in &problems.errors {
                println!("✕ {e}");
            }
            if problems.errors.is_empty() {
                println!(
                    "✓ {} {} · {} steps",
                    parsed.playbook.name,
                    parsed.playbook.version,
                    parsed.steps.len()
                );
            }
            Ok(problems.errors.is_empty())
        }
        PlaybookCmd::New { name, seed } => {
            let dir = ctx.paths.config.join("playbooks");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{name}.toml"));
            if path.exists() {
                bail!("{} already exists", path.display());
            }
            let text = format!(
                r#"[playbook]
name = "{name}"
version = "0.1.0"
description = "What this playbook finds, in one line"
seed = ["{seed}"]

# Run modules on the seed.
[[step]]
title = "First pass"
run = ["dns"]
on = "seed"

# A manual step: one checklist item for the seed, with launchers.
[[step]]
title = "Search by hand"
check = "Describe what to look for and where."
open = ["google-exact"]
on = "seed"
"#
            );
            spektr_protocol::PlaybookFile::from_toml(&text).context("the scaffold should parse")?;
            std::fs::write(&path, text)?;
            println!("created {}", path.display());
            println!("  edit it, then `spektr playbook check {}`", path.display());
            Ok(true)
        }
    }
}
