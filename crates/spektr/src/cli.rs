//! Command-line interface (ARCHITECTURE §10).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// spektr — a terminal workbench for open-source investigations.
#[derive(Debug, Parser)]
#[command(name = "spektr", version, about, long_about = None, after_long_help = crate::testenv::HELP)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create <name>.spektr.
    New(NewArgs),
    /// Seed and expand, run to quiescence, print a summary.
    Run(RunArgs),
    /// Search entities, neighbours and paths.
    Query(QueryArgs),
    /// Export the case.
    Export(ExportArgs),
    /// Re-hash the audit chain and every evidence blob. Exits 3 on failure.
    Verify(VerifyArgs),
    /// Check config, secrets, binaries, terminal and DNS.
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
pub struct NewArgs {
    pub name: String,
    #[arg(long, default_value = "default")]
    pub profile: String,
    /// The case subject's name (creates a `person` flagged as the subject).
    #[arg(long)]
    pub subject: Option<String>,
    /// YYYY-MM-DD.
    #[arg(long = "last-seen")]
    pub last_seen: Option<String>,
    /// Default region for phone numbers without +.
    #[arg(long, default_value = "US")]
    pub region: String,
    /// Default: the profile's default_depth.
    #[arg(long)]
    pub depth: Option<u32>,
    /// Scope rule, e.g. `domain:*.kestrel.example` or `deny cidr:203.0.113.0/24` (repeatable).
    #[arg(long = "scope")]
    pub scope: Vec<String>,
    /// Directory for the case file (default: current directory).
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    pub case: PathBuf,
    /// <type>:<value>, split at the first colon (repeatable).
    #[arg(long = "seed")]
    pub seeds: Vec<String>,
    /// Only these modules (repeatable; default: every eligible auto module).
    #[arg(long = "module")]
    pub modules: Vec<String>,
    #[arg(long)]
    pub depth: Option<u32>,
    /// Give up waiting after this long, e.g. 10m (default 30m).
    #[arg(long, default_value = "30m")]
    pub timeout: String,
    /// Print the summary and events as JSON lines.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    pub case: PathBuf,
    /// Only this entity type.
    #[arg(long = "type")]
    pub ty: Option<String>,
    /// Fuzzy match against values.
    #[arg(long = "match")]
    pub matching: Option<String>,
    /// List the neighbours of <type>:<value>.
    #[arg(long)]
    pub neighbors: Option<String>,
    /// With --neighbors: only this predicate.
    #[arg(long)]
    pub predicate: Option<String>,
    /// Shortest path between two entities.
    #[arg(long, num_args = 2, value_names = ["FROM", "TO"])]
    pub path: Option<Vec<String>>,
    /// List findings.
    #[arg(long)]
    pub findings: bool,
    /// One JSON object per line.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExportFormat {
    Json,
    Md,
    Tracelabs,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    pub case: PathBuf,
    #[arg(long, value_enum)]
    pub format: ExportFormat,
    /// Only these findings (repeatable; default all).
    #[arg(long = "finding")]
    pub findings: Vec<i64>,
    /// json only: include evidence blobs as base64.
    #[arg(long = "with-blobs")]
    pub with_blobs: bool,
    /// Write here instead of stdout.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    pub case: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(long)]
    pub json: bool,
}
