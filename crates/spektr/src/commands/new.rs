//! `spektr new <name>`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use spektr_engine::{CaseInit, EngineAction, EngineHandle, Profiles};

use crate::cli::NewArgs;
use crate::wiring::{Ctx, Modules};

pub async fn run(ctx: &Ctx, a: NewArgs) -> Result<()> {
    let (path, init, _) = create(ctx, &a, Modules::None).await?;
    println!("created {}", path.display());
    println!(
        "  rules {} · depth {} · region {}{}",
        init.rules_profile,
        init.default_depth,
        init.default_region,
        init.subject
            .as_deref()
            .map(|s| format!(" · subject ◎ {s}"))
            .unwrap_or_default()
    );
    Ok(())
}

/// Create the case file (shared with the TUI's new-case dialog) and record
/// it in recent.toml.
pub async fn create(
    ctx: &Ctx,
    a: &NewArgs,
    modules: Modules<'_>,
) -> Result<(PathBuf, CaseInit, EngineHandle)> {
    if a.name.is_empty()
        || !a
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        bail!(
            "case name `{}` may only use letters, digits, `.`, `_` and `-`",
            a.name
        );
    }
    let mut profiles = Profiles::builtin()?;
    for w in profiles.load_dir(&ctx.paths.config.join("profiles"))? {
        eprintln!("warning: {w}");
    }
    let profile = profiles.get(&a.profile).with_context(|| {
        format!(
            "unknown rules profile `{}` (available: {})",
            a.profile,
            profiles.names().collect::<Vec<_>>().join(", ")
        )
    })?;
    let last_seen = a
        .last_seen
        .as_deref()
        .map(|d| {
            d.parse::<jiff::civil::Date>()
                .with_context(|| format!("--last-seen `{d}` is not YYYY-MM-DD"))
        })
        .transpose()?;
    let region = a.region.trim().to_ascii_uppercase();
    if region.len() != 2 || !region.bytes().all(|b| b.is_ascii_uppercase()) {
        bail!("--region `{}` must be a two-letter country code", a.region);
    }
    let dir = match &a.dir {
        Some(d) => d.clone(),
        None => std::env::current_dir()?,
    };
    let path = dir.join(format!("{}.spektr", a.name));
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    let init = CaseInit {
        name: a.name.clone(),
        subject: a.subject.clone(),
        last_seen,
        rules_profile: profile.name.clone(),
        default_depth: a.depth.unwrap_or(profile.default_depth),
        default_region: region,
        hub_threshold: 50,
    };
    let handle = ctx.create(&path, &init, modules).await?;
    for rule in &a.scope {
        handle
            .act(EngineAction::AddScope(rule.clone()))
            .await
            .with_context(|| format!("--scope `{rule}`"))?;
    }
    handle.store().flush().await?;
    crate::recent::touch(&ctx.paths, &path, ctx.now());
    Ok((path, init, handle))
}
