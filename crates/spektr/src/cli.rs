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
    /// Open the TUI on a case.
    Open(OpenArgs),
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
    /// Playbooks: list, info, check, new.
    #[command(subcommand)]
    Playbook(PlaybookCmd),
    /// Capture a web page as evidence (HTML, headers, screenshot).
    Capture(CaptureArgs),
    /// Attach a local file as evidence.
    Attach(AttachArgs),
    /// Set up optional components.
    #[command(subcommand)]
    Setup(SetupCmd),
    /// The tutor: ten lessons on a bundled case, offline (about 15 minutes).
    Tutor {
        /// Start over.
        #[arg(long)]
        reset: bool,
    },
    /// Exec modules: new, test, check, install, list, info, enable, disable.
    #[command(subcommand)]
    Module(ModuleCmd),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ModuleLang {
    Python,
    Rust,
    Shell,
}

#[derive(Debug, Subcommand)]
pub enum ModuleCmd {
    /// Scaffold a working exec module in ./<name>/.
    New {
        name: String,
        #[arg(long, value_enum, default_value = "python")]
        lang: ModuleLang,
        /// Create it under this directory instead of ./
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Run a module once and show its validated output.
    Test {
        /// A module directory, or an installed module's name.
        name: String,
        /// <type>:<value>
        #[arg(long)]
        input: String,
        /// Save the module's messages as fixtures/<type>_<value>.jsonl.
        #[arg(long, conflicts_with = "fixture")]
        record: bool,
        /// Replay a fixture through validation instead of running live.
        #[arg(long)]
        fixture: Option<PathBuf>,
    },
    /// Lint the manifest, validate the vocabulary, replay the fixtures,
    /// check for secrets in fixtures and that the process exits.
    Check {
        name: String,
    },
    /// Install from a directory or a git URL (shows hosts and secrets,
    /// asks first, pins a tree hash).
    Install {
        /// A directory, a git URL, or with --update an installed module.
        source: String,
        /// Re-approve an installed module whose files changed.
        #[arg(long)]
        update: bool,
        /// Don't ask (scripts).
        #[arg(long)]
        yes: bool,
    },
    /// Built-in and installed modules.
    List,
    Info {
        name: String,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    #[command(hide = true)]
    ReplayFixture {
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlaybookCmd {
    /// Every playbook: built-in and from <config>/playbooks.
    List,
    /// A playbook's steps.
    Info { name: String },
    /// Check a playbook file (exit 1 on errors).
    Check { file: PathBuf },
    /// Scaffold <config>/playbooks/<name>.toml.
    New {
        name: String,
        /// The entity type it starts from.
        #[arg(long)]
        seed: String,
    },
}

#[derive(Debug, Args)]
pub struct CaptureArgs {
    pub case: PathBuf,
    pub url: String,
    /// The entity the capture is evidence for (<type>:<value>; default: the url).
    #[arg(long)]
    pub entity: Option<String>,
    #[arg(long = "no-screenshot")]
    pub no_screenshot: bool,
}

#[derive(Debug, Args)]
pub struct AttachArgs {
    pub case: PathBuf,
    pub path: PathBuf,
    /// The entity the file is evidence for (<type>:<value>).
    #[arg(long)]
    pub entity: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum SetupCmd {
    /// Download headless Chrome into the data dir, for capture screenshots.
    Capture,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    pub case: PathBuf,
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
    /// Start this playbook on the first seed, then print its steps and open
    /// checklist items.
    #[arg(long)]
    pub playbook: Option<String>,
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
