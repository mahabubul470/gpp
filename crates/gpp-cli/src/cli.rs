//! Command-line argument definitions (clap).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "gpp",
    version,
    about = "gpp (git++) — AI-native version control system",
    long_about = None
)]
pub struct Cli {
    /// Override repository path (default: search upward for .gpp/)
    #[arg(long, global = true, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    /// Override config file (reserved; not used in Phase 0)
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// When to colorize output
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Emit machine-readable JSON
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new gpp repository
    Init(InitArgs),
    /// Show working directory status and pending changes
    Status(StatusArgs),
    /// View and edit configuration
    Config(ConfigArgs),
}

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize (default: current directory)
    pub path: Option<PathBuf>,

    /// Enable the Graphex knowledge graph layer
    #[arg(long)]
    pub graphex: bool,

    /// Disable continuous timeline capture
    #[arg(long)]
    pub no_timeline: bool,

    /// Enable full-repo encryption (not just Graphex)
    #[arg(long)]
    pub encryption: bool,

    /// Initialize with a Git bridge to an existing remote
    #[arg(long, value_name = "URL")]
    pub git_bridge: Option<String>,

    /// Import from an existing Git repository (Phase 2)
    #[arg(long, value_name = "PATH")]
    pub from_git: Option<PathBuf>,

    /// Use a project template (later phase)
    #[arg(long, value_name = "NAME")]
    pub template: Option<String>,
}

#[derive(Args)]
pub struct StatusArgs {
    /// One-line summary
    #[arg(short, long)]
    pub short: bool,

    /// Show recent timeline entries
    #[arg(long)]
    pub timeline: bool,

    /// Show active agent sessions
    #[arg(long)]
    pub agents: bool,

    /// Show cost summary for current session
    #[arg(long)]
    pub cost: bool,
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,

    /// Operate on the repository config (.gpp/config.toml) [default]
    #[arg(long, global = true)]
    pub local: bool,

    /// Operate on the global config (~/.config/gpp/config.toml)
    #[arg(long, global = true)]
    pub global: bool,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a config value
    Get { key: String },
    /// Set a config value
    Set { key: String, value: String },
    /// List all config
    List,
    /// Open config in $EDITOR
    Edit,
}
