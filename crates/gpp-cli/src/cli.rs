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
    /// Show changes (semantic for supported languages, else line-based)
    Diff(DiffArgs),
    /// Manage branches and agent exploration branches
    Branch(BranchArgs),
    /// Import a Git repository's history into gpp
    GitImport(GitImportArgs),
    /// Export gpp history into a Git repository
    GitExport(GitExportArgs),
    /// Keep a Git repository and gpp in sync (import; optionally watch)
    GitBridge(GitBridgeArgs),
    /// Manage Graphex encryption keys
    Keys(KeysArgs),
    /// Query and manage the Graphex knowledge graph
    Graphex(GraphexArgs),
    /// Run the MCP server for AI tool integration
    McpServer(McpServerArgs),
    /// Agent trust scores and behavioral RBAC
    Trust(TrustArgs),
    /// Compliance-as-code policies
    Policy(PolicyArgs),
    /// Token / compute cost analytics
    Cost(CostArgs),
    /// Agent behavior anomaly alerts
    Anomaly(AnomalyArgs),
    /// Cross-layer audit report
    Audit(AuditArgs),
    /// Peer-to-peer synchronization
    Sync(SyncArgs),
    /// Reproduce a changeset's environment
    Replay(ReplayArgs),
    /// Merge a divergent fork branch into the current branch
    Merge(MergeArgs),
}

#[derive(Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub action: Option<SyncSub>,
    /// Sync only the Graphex layer (no code objects)
    #[arg(long, global = true)]
    pub graph_only: bool,
    /// Also sync the Graphex index
    #[arg(long, global = true)]
    pub include_graphex: bool,
}

#[derive(Subcommand)]
pub enum SyncSub {
    /// Register a peer
    Add { name: String, address: String },
    /// Remove a peer
    Remove { name: String },
    /// Show configured peers
    Status,
    /// Accept inbound syncs on an address (Ctrl-C to stop)
    Serve { address: String },
    /// Sync with one peer (default: all configured peers)
    Peer { name: String },
}

#[derive(Args)]
pub struct ReplayArgs {
    /// Changeset to reproduce (HEAD, a branch, or a hash)
    pub changeset: String,
    /// Compare a reproduced dir to the snapshot instead of writing
    #[arg(long)]
    pub diff: bool,
    /// Show what would be reproduced without writing
    #[arg(long)]
    pub dry_run: bool,
    /// Directory to materialize into
    #[arg(long, default_value = "replay-out")]
    pub output: std::path::PathBuf,
    /// Capture/override an env var (key=value, repeatable)
    #[arg(long = "env", value_name = "K=V")]
    pub env: Vec<String>,
}

#[derive(Args)]
pub struct MergeArgs {
    /// The fork ref to merge (e.g. "main.fork.office")
    pub fork_ref: String,
}

#[derive(Args)]
pub struct TrustArgs {
    #[command(subcommand)]
    pub action: TrustAction,
}

#[derive(Subcommand)]
pub enum TrustAction {
    /// Show all agent trust scores
    Show {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Show an agent's trust event history
    History {
        agent: String,
        #[arg(long)]
        since: Option<String>,
    },
    /// View trust thresholds
    Policy,
    /// Manually override an agent's status
    Override {
        agent: String,
        /// auto-merge|review-required|sandboxed|blocked
        #[arg(long)]
        status: String,
        #[arg(long)]
        reason: String,
        /// e.g. "7d" or "permanent"
        #[arg(long)]
        duration: Option<String>,
    },
    /// Reset an agent's score to default
    Reset { agent: String },
}

#[derive(Args)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub action: PolicyAction,
}

#[derive(Subcommand)]
pub enum PolicyAction {
    /// List active policies
    List,
    /// Show one policy's rules
    Show { name: String },
    /// Add a policy from a file
    Add { file: std::path::PathBuf },
    /// Install a built-in template
    Template { name: String },
    /// List built-in templates
    Templates,
    /// Remove a policy by name
    Remove { name: String },
    /// Validate a policy file's syntax
    Validate { file: std::path::PathBuf },
    /// Run all policies against the working tree (or a changeset)
    Check {
        #[arg(long)]
        changeset: Option<String>,
    },
}

#[derive(Args)]
pub struct CostArgs {
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    #[arg(long)]
    pub agent: Option<String>,
    /// Show cost per surviving line
    #[arg(long)]
    pub efficiency: bool,
    /// Per-agent/model breakdown
    #[arg(long)]
    pub breakdown: bool,
    /// Show budget status
    #[arg(long)]
    pub budget: bool,
    /// Set a weekly budget alert (in dollars) for the given pattern
    #[arg(long, value_name = "DOLLARS")]
    pub budget_alert: Option<f64>,
    #[arg(long, default_value = "**")]
    pub module: String,
}

#[derive(Args)]
pub struct AnomalyArgs {
    #[command(subcommand)]
    pub action: Option<AnomalyAction>,
    #[arg(long)]
    pub agent: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Subcommand)]
pub enum AnomalyAction {
    /// Show all anomalies (resolved and not)
    History,
    /// Mark an anomaly resolved
    Resolve {
        id: i64,
        #[arg(long)]
        reason: String,
    },
    /// List detection rules
    Rules,
    /// Configure a detection rule
    Configure {
        rule: String,
        #[arg(long)]
        threshold: Option<i64>,
        #[arg(long)]
        enabled: Option<bool>,
    },
}

#[derive(Args)]
pub struct AuditArgs {
    #[arg(long)]
    pub since: Option<String>,
    /// Include Graphex access log
    #[arg(long)]
    pub include_graphex: bool,
    /// Include cost summary
    #[arg(long)]
    pub include_cost: bool,
}

#[derive(Args)]
pub struct KeysArgs {
    #[command(subcommand)]
    pub action: KeysAction,
}

#[derive(Subcommand)]
pub enum KeysAction {
    /// Generate a fresh master + per-tier key hierarchy
    Generate,
    /// Rotate tier keys and re-encrypt all nodes
    Rotate,
    /// Show the master recipient and which tier keys exist
    Show,
}

#[derive(Args)]
pub struct GraphexArgs {
    #[command(subcommand)]
    pub action: GraphexAction,
}

#[derive(Subcommand)]
pub enum GraphexAction {
    /// Show graph statistics
    Status,
    /// Query the graph: "<subject> -> <relation> -> <object>"
    Query {
        pattern: String,
        #[arg(long, default_value_t = 1)]
        depth: usize,
        #[arg(long = "type")]
        node_type: Option<String>,
        #[arg(long)]
        tier: Option<String>,
        #[arg(long)]
        since: Option<String>,
        /// text|json
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Project context as an agent would receive it
    Project {
        /// Optional scope pattern
        pattern: Option<String>,
        /// Accessor max tier (default: agent-readable)
        #[arg(long, default_value = "agent-readable")]
        tier: String,
        #[arg(long, default_value_t = 8000)]
        budget: usize,
    },
    /// Add a node (human-created → Active)
    Add {
        #[arg(long = "type")]
        node_type: String,
        #[arg(long)]
        name: String,
        #[arg(long, short = 'd')]
        description: String,
        #[arg(long)]
        tier: Option<String>,
        /// key=value, repeatable
        #[arg(long, short = 'p')]
        properties: Vec<String>,
    },
    /// Create an edge between two nodes
    Link {
        from: String,
        #[arg(long)]
        relation: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        bidirectional: bool,
    },
    /// Show full node details
    Show { node: String },
    /// List nodes (optionally by state)
    List {
        /// proposed|active|deprecated|archived
        #[arg(long)]
        state: Option<String>,
    },
    /// List pending agent/auto proposals
    Pending,
    /// Approve a proposed node
    Accept { node: String },
    /// Reject (archive) a proposed node
    Reject { node: String },
    /// Show the access audit log
    Audit {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        accessor: Option<String>,
        #[arg(short = 'n', long, default_value_t = 50)]
        limit: usize,
    },
    /// Auto-infer proposed module nodes from the HEAD changeset
    Infer,
    /// Manage cross-project subgraph federation
    Federation {
        #[command(subcommand)]
        action: FederationAction,
    },
}

#[derive(Subcommand)]
pub enum FederationAction {
    /// Register a federated source (peer project subgraph)
    Add {
        #[arg(long)]
        project: String,
        #[arg(long)]
        address: String,
        #[arg(long, default_value = "default")]
        subgraph: String,
    },
    /// List federated sources
    List,
}

#[derive(Args)]
pub struct McpServerArgs {
    /// Use stdio transport (for Claude Code / Cursor integration)
    #[arg(long)]
    pub stdio: bool,
    /// TCP port (not implemented; use --stdio)
    #[arg(long)]
    pub port: Option<u16>,
    /// Maximum access tier exposed to connected agents
    #[arg(long, default_value = "agent-readable")]
    pub trust_tier: String,
}

#[derive(Args)]
pub struct GitImportArgs {
    /// Path to the Git repository to import from
    pub path: PathBuf,
}

#[derive(Args)]
pub struct GitExportArgs {
    /// Path to the Git repository to export into (created if absent)
    pub path: PathBuf,
}

#[derive(Args)]
pub struct GitBridgeArgs {
    /// Path to the Git repository to bridge with
    pub path: PathBuf,
    /// Also export gpp changes back to Git each cycle
    #[arg(long)]
    pub export: bool,
    /// Keep running, re-importing whenever Git HEAD moves
    #[arg(long)]
    pub watch: bool,
    /// Poll interval for --watch, in seconds
    #[arg(long, default_value_t = 2)]
    pub interval: u64,
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
    /// Force line-based diff (default for unsupported languages)
    #[arg(long)]
    pub line: bool,
    /// Force semantic diff even where it would not be the default
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
