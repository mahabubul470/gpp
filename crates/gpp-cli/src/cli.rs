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
    /// View and manage the continuous timeline
    Timeline(TimelineArgs),
    /// Promote timeline entries to a curated changeset
    Promote(PromoteArgs),
    /// View changeset history
    Log(LogArgs),
    /// Show changes (line-based; semantic diff arrives in Phase 2)
    Diff(DiffArgs),
    /// Manage branches and agent exploration branches
    Branch(BranchArgs),
}

#[derive(Args)]
pub struct TimelineArgs {
    #[command(subcommand)]
    pub action: Option<TimelineAction>,

    /// Show entries since this time (e.g. "1h", "2d", "today", "2026-05-16")
    #[arg(long, global = true)]
    pub since: Option<String>,
    /// Show entries until this time
    #[arg(long, global = true)]
    pub until: Option<String>,
    /// Filter by author id
    #[arg(long, global = true)]
    pub author: Option<String>,
    /// Filter by file path glob
    #[arg(long, global = true, value_name = "PATTERN")]
    pub file: Option<String>,
    /// Show last N entries
    #[arg(short = 'n', long, global = true, default_value_t = 20)]
    pub limit: u32,
    /// Show per-file change details
    #[arg(long, global = true)]
    pub stat: bool,
}

#[derive(Subcommand)]
pub enum TimelineAction {
    /// Live-stream timeline entries as they happen
    Watch,
    /// Search timeline by file/author
    Search,
    /// Remove old timeline entries per retention policy
    Prune {
        /// Override retention (e.g. "60d", "24h")
        #[arg(long)]
        older_than: Option<String>,
    },
    /// Export the (filtered) timeline as JSON
    Export {
        /// Output file (default: stdout)
        path: Option<PathBuf>,
    },
}

#[derive(Args)]
pub struct PromoteArgs {
    /// Start of timeline range (entry id or time)
    #[arg(long)]
    pub from: Option<String>,
    /// End of timeline range (entry id or time)
    #[arg(long)]
    pub to: Option<String>,
    /// Changeset description
    #[arg(short, long)]
    pub message: Option<String>,
    /// Intent type: feature|bugfix|refactor|docs|dependency
    #[arg(long)]
    pub intent: Option<String>,
    /// Link to a task/issue
    #[arg(long)]
    pub task: Option<String>,
    /// Interactively select entries (Phase 1: not implemented)
    #[arg(short, long)]
    pub interactive: bool,
    /// Use AI to summarize (later phase: not implemented)
    #[arg(long)]
    pub auto_summarize: bool,
    /// Cryptographically sign (later phase: not implemented)
    #[arg(long)]
    pub sign: bool,
}

#[derive(Args)]
pub struct LogArgs {
    /// One line per changeset
    #[arg(long)]
    pub oneline: bool,
    /// Show a simple ASCII graph column
    #[arg(long)]
    pub graph: bool,
    /// Show semantic change summaries (Phase 2: not available)
    #[arg(long)]
    pub semantic: bool,
    /// Filter by author id
    #[arg(long)]
    pub author: Option<String>,
    /// Only agent-authored changesets
    #[arg(long)]
    pub agent: bool,
    /// Only human-authored changesets
    #[arg(long)]
    pub human: bool,
    /// Filter by intent type
    #[arg(long)]
    pub intent: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    /// Show last N changesets
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args)]
pub struct DiffArgs {
    /// Target: empty (working vs HEAD), <changeset>, or <cs1>..<cs2>
    pub target: Option<String>,
    /// Force line-based diff (the only mode in Phase 1)
    #[arg(long)]
    pub line: bool,
    /// Show semantic operations (Phase 2: falls back to line)
    #[arg(long)]
    pub semantic: bool,
    /// Show only statistics
    #[arg(long)]
    pub stat: bool,
    /// Show only file names
    #[arg(long)]
    pub files: bool,
}

#[derive(Args)]
pub struct BranchArgs {
    #[command(subcommand)]
    pub action: Option<BranchAction>,
    /// Show all branches including explorations
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Subcommand)]
pub enum BranchAction {
    /// Create a new branch at the current tip
    Create { name: String },
    /// Delete a branch
    Delete { name: String },
    /// Switch to a branch
    Switch { name: String },
    /// Create an exploration branch (explorations/<name>)
    Explore { name: String },
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
