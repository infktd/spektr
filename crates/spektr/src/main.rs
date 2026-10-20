//! The `spektr` binary: CLI, config and wiring (docs/ARCHITECTURE.md §2, §10).

use clap::Parser;

/// Terminal OSINT workbench.
///
/// Phase 0 scaffold: no commands are implemented yet.
#[derive(Debug, Parser)]
#[command(name = "spektr", version, about)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    println!(
        "spektr {}: scaffold only; commands arrive in later phases",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}
