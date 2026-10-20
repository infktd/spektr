//! The `spektr` binary: CLI, config and wiring (docs/ARCHITECTURE.md §2, §10).

mod cli;
mod commands;
mod config;
mod fmt;
mod logging;
mod paths;
mod recent;
mod testenv;
mod tui_host;
mod wiring;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

/// Exit codes (ARCHITECTURE §10).
const EXIT_ERROR: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_VERIFY: u8 = 3;

/// Run the TUI (launcher or a case) until the user quits.
async fn tui(ctx: wiring::Ctx, start: spektr_tui::Start) -> anyhow::Result<ExitCode> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "spektr's TUI needs a terminal; for scripts use `spektr run`, `query` or `export`"
        );
    }
    let settings = tui_host::settings(&ctx);
    let host = std::sync::Arc::new(tui_host::TuiHost {
        ctx: std::sync::Arc::new(ctx),
        tutor: std::sync::atomic::AtomicBool::new(false),
    });
    let engine = spektr_tui::run(host, settings, start).await?;
    if let Some(h) = engine {
        h.store().flush().await?;
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = match wiring::Ctx::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("spektr: {e:#}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let _log = logging::init(&ctx.paths, ctx.secrets.values().cloned().collect());
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("spektr: starting the runtime: {e}");
            return ExitCode::from(EXIT_ERROR);
        }
    };
    let result: anyhow::Result<ExitCode> = rt.block_on(async {
        let Some(command) = cli.command else {
            return tui(ctx, spektr_tui::Start::Launcher).await;
        };
        match command {
            Command::Open(a) => {
                if !a.case.exists() {
                    anyhow::bail!("{} not found", a.case.display());
                }
                tui(ctx, spektr_tui::Start::Open(a.case)).await
            }
            Command::New(a) => commands::new::run(&ctx, a)
                .await
                .map(|()| ExitCode::SUCCESS),
            Command::Run(a) => commands::run::run(&ctx, a)
                .await
                .map(|()| ExitCode::SUCCESS),
            Command::Query(a) => commands::query::run(&ctx, a)
                .await
                .map(|()| ExitCode::SUCCESS),
            Command::Export(a) => commands::export::run(&ctx, a).map(|()| ExitCode::SUCCESS),
            Command::Verify(a) => commands::verify::run(&ctx, a).map(|ok| {
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(EXIT_VERIFY)
                }
            }),
            Command::Doctor(a) => commands::doctor::run(&ctx, a).map(|()| ExitCode::SUCCESS),
            Command::Playbook(cmd) => commands::playbook::run(&ctx, cmd).await.map(|ok| {
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(EXIT_ERROR)
                }
            }),
            Command::Capture(a) => commands::evidence::capture(&ctx, a)
                .await
                .map(|()| ExitCode::SUCCESS),
            Command::Attach(a) => commands::evidence::attach(&ctx, a)
                .await
                .map(|()| ExitCode::SUCCESS),
            Command::Tutor { reset } => tui(ctx, spektr_tui::Start::Tutor(reset)).await,
            Command::Module(cmd) => commands::module::run(&ctx, cmd).await.map(|ok| {
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(EXIT_ERROR)
                }
            }),
            Command::Setup(crate::cli::SetupCmd::Capture) => {
                commands::setup::capture(&ctx).map(|()| ExitCode::SUCCESS)
            }
        }
    });
    match result {
        Ok(code) => code,
        Err(e) => {
            tracing::error!("{e:#}");
            eprintln!("spektr: {e:#}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}
