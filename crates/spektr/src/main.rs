//! The `spektr` binary: CLI, config and wiring (docs/ARCHITECTURE.md §2, §10).

mod cli;
mod commands;
mod config;
mod fmt;
mod logging;
mod paths;
mod testenv;
mod wiring;

use std::process::ExitCode;

use clap::{CommandFactory, Parser};

use crate::cli::{Cli, Command};

/// Exit codes (ARCHITECTURE §10).
const EXIT_ERROR: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_VERIFY: u8 = 3;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        // The launcher arrives with the TUI (Phase 3).
        let _ = Cli::command().print_help();
        return ExitCode::SUCCESS;
    };
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
        match command {
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
