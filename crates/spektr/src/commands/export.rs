//! `spektr export <case>`: Trace Labs submission text, a Markdown report,
//! or the whole case as JSON (`spektr_engine::export`).

use std::io::Write;

use anyhow::{Context, Result, bail};
use spektr_engine::export::{Format, Options, render};
use spektr_store::Db;

use crate::cli::{ExportArgs, ExportFormat};
use crate::wiring::Ctx;

pub fn run(ctx: &Ctx, a: ExportArgs) -> Result<()> {
    let format = match a.format {
        ExportFormat::Json => Format::Json,
        ExportFormat::Md => Format::Md,
        ExportFormat::Tracelabs => Format::Tracelabs,
    };
    if a.with_blobs && format != Format::Json {
        bail!("--with-blobs is for --format json");
    }
    if !a.case.exists() {
        bail!("{} not found", a.case.display());
    }
    let mut db = Db::open_read_only(&a.case, ctx.store_options()?)?;
    let name = db
        .read(|r| r.meta("rules_profile"))?
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "default".into());
    let profiles = ctx.profiles()?;
    let profile = profiles
        .get(&name)
        .cloned()
        .with_context(|| format!("unknown rules profile `{name}`"))?;
    let opts = Options {
        findings: a.findings.clone(),
        with_blobs: a.with_blobs,
    };
    if !opts.findings.is_empty() {
        let known: Vec<i64> = db
            .read(|r| r.findings())?
            .into_iter()
            .map(|f| f.id)
            .collect();
        if let Some(missing) = opts.findings.iter().find(|f| !known.contains(f)) {
            bail!("no finding F-{missing}");
        }
    }
    let text = db.read(|r| render(r, &profile, format, &opts))?;
    match a.output {
        Some(p) => std::fs::write(&p, &text)?,
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(text.as_bytes())?;
        }
    }
    Ok(())
}
