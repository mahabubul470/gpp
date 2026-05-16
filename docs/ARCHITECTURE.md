# Architecture — gpp (git++)

## System Overview

gpp is a layered system where each layer has a single responsibility and communicates through well-defined interfaces. The layers are implemented as separate Rust crates in a Cargo workspace. Each crate exposes a public API and can be used independently or composed.

```
┌─────────────────────────────────────────────────────────────────────┐
│            CLI / TUI / gh-gpp / VS Code / Neovim                     │
├──────────────────────────────┬──────────────────────────────────────┤
│          Agent SDK (MCP)     │         Remote (GitHub/GitLab/BB)    │
├──────────────┬───────────────┼──────────────┬──────────────────────┤
│   Review     │    RBAC       │    Notify    │   Relay              │
├──────────────┼───────────────┼──────────────┼──────────────────────┤
│   Anomaly    │    Replay     │  Dependencies│   Cost               │
├──────────────┴──────┬────────┴──────────────┴──────────────────────┤
│     Trust Engine    │        Policy Engine                          │
├─────────────────────┴──────────────────────────────────────────────┤
│                    Graphex (Knowledge Graph)                         │
├─────────────────────┬──────────────────────────────────────────────┤
│   Semantic Diff     │           Sync Protocol                       │
├─────────────────────┴──────────────────────────────────────────────┤
│                    History (Curated Changesets)                      │
├────────────────────────────────────────────────────────────────────┤
│                    Timeline (Continuous Capture)                     │
├────────────────────────────────────────────────────────────────────┤
│                    Storage (Content-Addressed Blobs)                 │
└────────────────────────────────────────────────────────────────────┘
```

## Layer 1: Storage (gpp-core)

The foundation. A content-addressed object store similar to Git's but with richer object types.

### Object Types

| Type | Description | Git Equivalent |
|------|-------------|----------------|
| `Blob` | Raw file content | `blob` |
| `Tree` | Directory listing with metadata | `tree` |
| `Changeset` | Collection of changes with intent | `commit` |
| `Intent` | Why a change was made (prompt, task ID, goal) | — |
| `AgentMeta` | Agent identity, model version, config snapshot | — |
| `GraphNode` | Knowledge graph node (encrypted) | — |
| `GraphEdge` | Knowledge graph relationship | — |
| `PolicyRule` | Compliance rule definition | — |
| `ConversationThread` | Discussion attached to a changeset | — |
| `Review` | Code review state and decisions | — |
| `Permission` | Human RBAC role assignment | — |
| `Notification` | Event notification record | — |
| `RemoteMeta` | Platform-specific metadata (PR number, CI status) | — |

### Content Addressing

Every object is hashed with BLAKE3 and stored at `.gpp/objects/<hash[0:2]>/<hash[2:]>`. Objects are compressed with zstd before storage. Encryption is optional per object — Graphex nodes are always encrypted, code blobs are encrypted only if the repo-level encryption flag is set.

### Storage Layout

```
.gpp/
├── config.toml              # Repository configuration
├── objects/                  # Content-addressed object store
│   ├── 3f/a9b2...           # Compressed, optionally encrypted blobs
│   └── ...
├── timeline/                 # Timeline database (SQLite)
│   └── timeline.db
├── graphex/                  # Knowledge graph
│   ├── graph.db              # Adjacency index (SQLite)
│   └── keys/                 # Encrypted key store
├── trust/                    # Agent trust data
│   └── trust.db
├── policies/                 # Active policy rules
│   └── *.policy
├── reviews/                  # Code review state
│   └── reviews.db
├── rbac/                     # Human permissions
│   └── permissions.db
├── notify/                   # Notification queue and config
│   ├── events.db
│   └── integrations.toml    # Slack/Discord/webhook config
├── remote/                   # Platform integration state
│   ├── github.toml           # GitHub API config (token, repo, etc.)
│   └── cache/                # Cached remote metadata (PR states, CI results)
├── refs/                     # Branch-like references
│   ├── main
│   ├── explorations/         # Agent exploration branches
│   └── agents/               # Per-agent working branches
├── HEAD                      # Current ref pointer
└── git-bridge/               # Git compatibility data
    └── mapping.db            # gpp hash ↔ Git SHA mapping
```

## Layer 2: Timeline (gpp-timeline)

An always-on file system watcher that records every meaningful change.

### How It Works

1. Uses `notify` crate (inotify on Linux, FSEvents on macOS) to watch the working directory
2. On file change, debounces for 100ms, then snapshots the changed file(s)
3. Creates a `TimelineEntry` with: timestamp, file paths, content hashes, author (human or agent ID), and change source (editor, CLI, agent SDK)
4. Appends to `timeline.db` (SQLite WAL mode for concurrent reads)
5. Stores the actual content as blobs in the object store

### Timeline Entry Schema

```sql
CREATE TABLE timeline_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,     -- Unix microseconds UTC
    author_type TEXT NOT NULL,         -- 'human' | 'agent'
    author_id   TEXT NOT NULL,         -- username or agent identifier
    source      TEXT NOT NULL,         -- 'editor' | 'cli' | 'agent-sdk' | 'fs-watch'
    summary     TEXT,                  -- Auto-generated or AI-summarized
    parent_id   INTEGER REFERENCES timeline_entries(id)
);

CREATE TABLE timeline_files (
    entry_id    INTEGER REFERENCES timeline_entries(id),
    file_path   TEXT NOT NULL,
    blob_hash   TEXT NOT NULL,         -- BLAKE3 hash of file content
    change_type TEXT NOT NULL,         -- 'add' | 'modify' | 'delete' | 'rename'
    old_hash    TEXT,                  -- Previous blob hash (for modify/delete)
    PRIMARY KEY (entry_id, file_path)
);
```

### Performance Target

- Timeline should add < 5ms latency to any file save operation
- SQLite WAL mode allows concurrent reads during writes
- Old timeline entries can be pruned/compacted (configurable retention: default 30 days)

## Layer 3: History (gpp-history)

Curated changesets promoted from the timeline. This is the "clean history" layer — what you share, review, and deploy.

### Promotion Flow

```
Timeline entries → Select range → Create Changeset → Attach intent/threads → History
```

A changeset can be created:
- Manually: `gpp promote --from <timestamp> --to <timestamp>`
- By AI: Agent SDK calls `promote()` with a summary
- Automatically: Policy rule triggers promotion (e.g., "promote every passing CI run")

### Changeset Structure

```rust
struct Changeset {
    id: Hash,                       // BLAKE3 of contents
    parent: Option<Hash>,           // Previous changeset (DAG, not linear)
    tree: Hash,                     // Root tree object at this point
    timestamp: i64,                 // Unix microseconds UTC
    author: Author,                 // Human or agent
    intent: Option<Hash>,           // Link to Intent object
    timeline_range: (i64, i64),     // Timeline entry range this covers
    files_changed: Vec<FileChange>, // Semantic file changes
    thread: Option<Hash>,           // Conversation thread
    signature: Option<Vec<u8>>,     // Optional cryptographic signature
    metadata: BTreeMap<String, String>, // Extensible key-value metadata
}
```

## Layer 4: Graphex (gpp-graphex)

The encrypted project knowledge graph. See GRAPHEX_PROTOCOL.md for the full protocol specification.

### Core Concepts

- **Nodes** represent entities: services, modules, concepts, conventions, people, external systems
- **Edges** represent relationships: depends-on, communicates-with, owned-by, implements-policy
- **Each node is individually encrypted** with its own key (envelope encryption via age)
- **Access tiers** control what agents can see: `public`, `agent-readable`, `agent-restricted`, `human-only`
- **The graph is versioned** alongside code — every graph mutation creates a timeline entry
- **Federation** allows sharing subgraphs across projects within an organization

### Query Interface

```
gpp graphex query "auth-service -> depends-on -> *"
gpp graphex query "* -> implements-policy -> pci-dss"
gpp graphex query "orders-service -> * -> *" --depth 2
```

Query language is a simple path pattern:
- `*` matches any node
- `->` follows an edge in the specified direction
- `--depth N` controls traversal depth
- Filters: `--type service`, `--tier agent-readable`, `--since 2026-01-01`

### Context Projection

When an agent queries Graphex, it doesn't get raw nodes. It gets a **projected context** — a flattened, scrubbed summary safe for the agent's trust tier.

```
Agent request: "I need context for working on the orders module"
→ Graphex checks agent trust tier
→ Selects relevant subgraph (orders + 1-hop neighbors)
→ Filters by access tier (strips agent-restricted nodes)
→ Flattens to structured text summary
→ Returns projected context (never raw graph data)
```

## Layer 5: Trust Engine (gpp-trust)

Tracks agent behavior and adjusts permissions dynamically.

### Trust Score Calculation

```
trust_score = weighted_average(
    survival_rate     * 0.35,  // % of changes surviving review unchanged
    regression_rate   * 0.25,  // inverse of regressions introduced
    review_approval   * 0.20,  // % of changes approved on first review
    graph_accuracy    * 0.10,  // % of graph updates approved
    convention_follow * 0.10,  // adherence to project conventions from Graphex
)
```

Score ranges:
- 90-100: Auto-merge eligible for low-risk changes
- 70-89: Review required, but can work on any module
- 50-69: Sandboxed — restricted to specific modules
- Below 50: Blocked — changes require explicit human override

### Trust Policies

```toml
# .gpp/trust-policy.toml
[thresholds]
auto_merge_min = 90
review_required_min = 70
sandbox_min = 50

[module_overrides]
"orders-service" = { auto_merge_min = 95 }  # Higher bar for critical modules
"docs/**" = { auto_merge_min = 75 }             # Lower bar for documentation
```

## Layer 6: Policy Engine (gpp-policy)

Compliance rules enforced at the storage layer — not as hooks that can be skipped.

### Policy File Format

```toml
# .gpp/policies/secrets-scan.policy
[policy]
name = "secrets-scan"
version = "1.0"
severity = "block"  # 'block' | 'warn' | 'audit'

[[rules]]
type = "pattern"
pattern = '(?i)(api[_-]?key|secret|password|token)\s*[=:]\s*["\'][^"\']{8,}'
message = "Potential hardcoded secret detected"
exclude = ["*.test.*", "*.example.*", "docs/**"]

[[rules]]
type = "pattern"
pattern = '-----BEGIN (RSA |EC |DSA )?PRIVATE KEY-----'
message = "Private key detected — never commit private keys"

[[rules]]
type = "changeset"
condition = "files_match('**/compliance/**') && author.type == 'agent'"
require = "reviewers >= 2 && reviewers.all(r => r.type == 'human')"
message = "Agent changes to compliance code require 2 human reviewers"
```

### Enforcement Points

Policies are checked at:
1. **Timeline capture** — warns on capture, but doesn't block (safety net shouldn't have friction)
2. **History promotion** — blocks if severity is "block"
3. **Sync push** — blocks pushing policy-violating changesets to remotes

## Layer 7: Semantic Diff (gpp-diff)

Structure-aware diffing powered by tree-sitter.

### Diff Types

| Diff Type | Description | Example |
|-----------|-------------|---------|
| `TextDiff` | Fallback line-based diff | Unknown file types |
| `StructuralDiff` | AST-aware diff | Function modified, parameter added |
| `SemanticMove` | Code moved between files | Function extracted to new module |
| `SemanticRename` | Identifier renamed across codebase | Variable/function rename |
| `SemanticRefactor` | Structural reorganization | Interface extraction, pattern change |

### How It Works

1. Parse both old and new files with tree-sitter
2. Build AST fingerprints for top-level declarations (functions, classes, types)
3. Match declarations across old/new using fingerprint similarity
4. For matched pairs, compute structural diff (added/removed/modified nodes)
5. For unmatched items, check across files for moves
6. Produce a `SemanticDiffResult` with high-level operations

### Supported Languages (Phase 1)

- Rust
- TypeScript / JavaScript
- Python
- Go

### Plugin Interface

```rust
pub trait LanguageParser: Send + Sync {
    fn language_id(&self) -> &str;
    fn file_extensions(&self) -> &[&str];
    fn parse(&self, source: &[u8]) -> Result<ParseTree>;
    fn extract_declarations(&self, tree: &ParseTree) -> Vec<Declaration>;
    fn fingerprint(&self, decl: &Declaration) -> Vec<u8>;
}
```

## Layer 8: Sync Protocol (gpp-sync)

See SYNC_PROTOCOL.md for the full specification. Summary:

- CRDT-based (operation-based, Automerge-inspired)
- Peer-to-peer — any node can sync with any other node
- Offline-first — works without network, syncs when reconnected
- Zero-knowledge graph sync — graph structure syncs, but encrypted content doesn't leak
- Transport: TCP with noise protocol encryption (like WireGuard)

## Layer 9: Cost Attribution (gpp-cost)

### Tracked Metrics Per Changeset

```rust
struct CostRecord {
    changeset_id: Hash,
    agent_id: String,
    model_id: String,            // e.g., "claude-sonnet-4-20250514"
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
    estimated_cost_microdollars: i64,  // 1 = $0.000001
    duration_ms: u64,            // Wall clock time for agent session
    files_touched: u32,
    lines_changed: u32,
    lines_survived_review: u32,  // After human review
}
```

### Analytics Queries

```bash
gpp cost --this-week                          # Total spend this week
gpp cost --module orders --last-month     # Module-level spend
gpp cost --agent claude-code --efficiency     # Cost per survived line
gpp cost --budget-alert 100.00               # Alert at $100/week
```

## Layer 10: Dependencies (gpp-deps)

### Dependency Record

```rust
struct DependencyRecord {
    name: String,
    version: String,
    registry: String,           // "crates.io" | "npm" | "pypi" | etc.
    used_in: Vec<String>,       // Which modules import this
    criticality: Criticality,   // how deep in critical path
    maintainer_count: u32,
    last_release: i64,          // timestamp
    known_cves: Vec<CveRecord>,
    license: String,
    risk_score: f64,            // 0.0 (safe) to 1.0 (high risk)
}
```

### Auto-Assessment

When an agent adds a dependency:
1. Fetch metadata from registry API
2. Check CVE databases
3. Analyze license compatibility
4. Compute risk score
5. Check against policy rules
6. Surface assessment to developer before accepting

## Layer 11: Anomaly Detection (gpp-anomaly)

### Detection Rules

| Rule | Trigger | Severity |
|------|---------|----------|
| `unusual-scope` | Agent modifies files outside its normal module | Warning |
| `burst-activity` | > 20 changesets in 5 minutes from one agent | Warning |
| `large-changeset` | Changeset > 3x average project change size | Info |
| `convention-violation` | Change contradicts Graphex convention node | Warning |
| `new-module-access` | Agent touches a module for the first time | Info |
| `deletion-spike` | > 50% of changes are deletions | Warning |
| `dependency-add` | Agent adds new external dependency | Review |

### Response Actions

Each anomaly can trigger: `log`, `warn` (notify developer), `pause` (hold changeset for review), or `block`.

## Layer 12: Replay Engine (gpp-replay)

### Environment Snapshot

```rust
struct ReplaySnapshot {
    changeset_id: Hash,
    agent_config: AgentConfig,
    model_id: String,
    model_version: String,
    system_prompt_hash: Hash,
    temperature: f32,
    context_projected: Hash,      // The exact Graphex projection given to agent
    working_tree_hash: Hash,      // The exact code state before agent ran
    timeline_position: i64,       // Where in timeline this session started
    environment: BTreeMap<String, String>, // Relevant env vars (scrubbed)
}
```

### Replay Command

```bash
gpp replay cs:a3f9b2              # Replay the session that produced this changeset
gpp replay cs:a3f9b2 --model claude-opus-4-6  # Replay with different model
gpp replay cs:a3f9b2 --diff       # Compare replay result with original
```

## Git Bridge (gpp-git-bridge)

### Import

```bash
gpp git-import /path/to/git/repo   # Import full Git history
gpp git-import --branch main        # Import single branch
```

Converts: Git commits → gpp changesets, Git trees → gpp trees, Git blobs → gpp blobs. Maintains a bidirectional hash mapping in `git-bridge/mapping.db`.

### Export

```bash
gpp git-export --to /path/to/git/repo   # Export as Git commits
gpp git-export --branch main --push      # Export and push
```

### Continuous Sync

```bash
gpp git-bridge --watch   # Bidirectional sync with a linked Git remote
```

## Agent SDK

### Rust API

```rust
use gpp_sdk::AgentSession;

let session = AgentSession::new("claude-code", "claude-sonnet-4-20250514")?;
let context = session.query_graphex("orders -> *")?;
session.begin_exploration("fix-orders-rounding")?;
// ... make changes ...
session.propose_changeset("Fixed rounding error in order batch")?;
session.propose_graph_update(GraphUpdate::AddEdge {
    from: "orders-service",
    to: "currency-utils",
    relation: "depends-on",
})?;
session.end()?;
```

### MCP Server

gpp runs an MCP server that any AI tool can connect to:

```json
{
  "mcpServers": {
    "gpp": {
      "command": "gpp",
      "args": ["mcp-server"],
      "env": { "GPP_REPO": "/path/to/repo" }
    }
  }
}
```

Exposed MCP tools:
- `graphex_query` — Query the knowledge graph
- `timeline_status` — Get recent timeline entries
- `propose_changeset` — Promote timeline entries to a changeset
- `propose_graph_update` — Suggest a knowledge graph mutation
- `trust_status` — Check current agent trust score
- `policy_check` — Validate a change against policies
- `cost_estimate` — Estimate token cost for a context projection

## Agent Interaction Tiers

gpp supports three tiers of AI agent integration, designed so that each tier adds value without requiring the one above it.

### Tier 1: Passive (Zero Config)

The agent doesn't know gpp exists. It edits files normally via any tool (Cursor, Copilot, Claude Code, vim, etc.). The timeline layer captures every change in the background. The developer promotes changesets manually. This works with *any* AI tool from day one — no plugin, no MCP, no SDK. The agent benefits from continuous capture without awareness of it.

### Tier 2: Context-Aware (MCP Connection)

The agent connects via MCP and queries Graphex before starting work. It receives projected context about the project architecture, conventions, glossary terms, and recent decisions. It still writes code the normal way — file edits captured by the timeline — but makes *better* changes because it has context. Setup is 5 lines in the agent's MCP config:

```json
{
  "mcpServers": {
    "gpp": {
      "command": "gpp",
      "args": ["mcp-server"],
      "env": { "GPP_REPO": "." }
    }
  }
}
```

### Tier 3: Native (SDK Integration)

The agent uses the gpp SDK directly. It creates exploration branches, proposes changesets with intent metadata, proposes graph updates, reports its own token costs, and participates in the review workflow. This requires the AI tool vendor to integrate the SDK — longer-term play, but the richest experience.

### Agent-to-Agent Collaboration

When multiple agents work on the same repo, exploration branches isolate them naturally. gpp supports an orchestration pattern:

```
Developer assigns task "fix orders rounding"
  ├── Agent A (Claude Code) → exploration/orders-fix-claude
  ├── Agent B (Copilot) → exploration/orders-fix-copilot
  └── Agent C (Codex) → exploration/orders-fix-codex
Developer reviews all three, accepts best approach, merges.
```

Agents at Tier 2+ can optionally read each other's exploration branches (controlled by trust policies). A "lead agent" pattern is supported where one agent reviews other agents' explorations and recommends the best approach to the developer.

## Relay Node (gpp-relay)

The relay is NOT a server with authority. It's a persistent peer — always online, always reachable, acting as a sync hub. It stores encrypted objects and forwards sync operations. It never needs to decrypt Graphex content.

### Architecture

```rust
// gpp-relay is a minimal binary
struct RelayNode {
    storage: ObjectStore,        // Stores forwarded objects
    sync: SyncEngine,            // Handles peer connections
    peers: PeerRegistry,         // Tracks connected peers
    auth: PeerAuthenticator,     // Noise protocol keys
    config: RelayConfig,
}
```

### Deployment

```bash
# Single binary, single command
gpp-relay --port 9473 --storage /data/gpp --max-repos 100

# Or via Docker
docker run -p 9473:9473 -v /data:/data ghcr.io/gpp-vcs/gpp-relay
```

### Relay vs GitHub

| | GitHub | gpp-relay |
|---|---|---|
| Authority | Central authority, owns the "truth" | No authority, just another peer |
| Content access | GitHub can read all code | Relay stores encrypted blobs, can't read content |
| Features | PRs, issues, Actions, Copilot | Sync only — no UI, no features |
| Cost | $4-21/user/month | Self-hosted, $5/month VPS |
| Availability | Subject to GitHub outages | You control uptime |
| Dependency | Vendor lock-in | Swap anytime, data is local |

## Review Workflow (gpp-review)

Code review built into the VCS, not dependent on an external platform.

### Review Lifecycle

```
Changeset created
  → Review requested (auto or manual)
    → Pending review
      → Approved (by N reviewers per policy)
      → Changes requested (with thread comments)
      → Rejected (with reason)
    → Merged into target branch
```

### Review Assignment

Reviews are assigned based on semantic code ownership from the Graphex knowledge graph, not file-path CODEOWNERS:

```
Changeset touches orders logic → Graphex query "* -> owned-by -> *" for orders nodes
  → Orders team members are auto-assigned as reviewers
  → If changeset crosses ownership boundaries, all relevant owners notified
```

### Review Objects

```rust
struct Review {
    id: Hash,
    changeset: Hash,                     // Changeset being reviewed
    status: ReviewStatus,
    requested_by: Author,
    requested_at: i64,
    reviewers: Vec<ReviewerRecord>,
    thread: Option<Hash>,                // ConversationThread
    policy_requirements: ReviewPolicy,   // Min reviewers, human-only for certain modules
}

enum ReviewStatus {
    Pending,
    Approved,
    ChangesRequested,
    Rejected,
    Merged,
}

struct ReviewerRecord {
    reviewer: Author,
    decision: Option<ReviewDecision>,
    decided_at: Option<i64>,
    comments: Vec<Hash>,                 // ConversationThread entries
}

enum ReviewDecision {
    Approve,
    RequestChanges { reason: String },
    Reject { reason: String },
}
```

### Review via CLI

```bash
gpp review list                                 # Show pending reviews
gpp review show cs:a3f9b2                       # Show changeset with semantic diff + context
gpp review approve cs:a3f9b2                    # Approve
gpp review request-changes cs:a3f9b2 -m "Need tests for orders edge case"
gpp review reject cs:a3f9b2 -m "Wrong approach, see exploration/orders-fix-claude instead"
```

### Review via Remote Platforms

When `gpp remote` is configured for GitHub, reviews can be mirrored:
- `gpp review approve` → approves the corresponding GitHub PR
- GitHub PR approval → syncs back as a gpp review approval
- Review comments sync bidirectionally (gpp threads ↔ GitHub PR comments)

## Human RBAC (gpp-rbac)

Permission model for human collaborators, enforced by the sync protocol.

### Roles

| Role | Permissions |
|------|------------|
| **Owner** | Everything + rotate keys, manage federation, set policies, assign roles |
| **Maintainer** | Approve/reject reviews, override trust scores, manage policies, merge to protected branches |
| **Contributor** | Promote changesets, propose graph updates, create exploration branches, request reviews |
| **Reader** | Sync and read all content (decryptable at their tier), but cannot push changes |

### Role Assignment

```toml
# .gpp/rbac/roles.toml
[roles]
owner = ["owner@example.com"]
maintainer = ["maintainer1@example.com", "maintainer2@example.com"]
contributor = ["dev1@example.com", "dev2@example.com"]
reader = ["auditor@example.com"]
```

### Enforcement

Roles are enforced at two points:
1. **Local CLI** — `gpp promote` on a protected branch checks if the user is a maintainer
2. **Sync protocol** — the relay/peer rejects pushes from users without the required role

Role changes are themselves changesets — they're versioned, auditable, and require owner approval.

### Agent Permissions vs Human Permissions

Trust engine governs agents. RBAC governs humans. They're separate systems:
- A maintainer can override an agent's trust score
- A contributor cannot override trust scores but can work with auto-merge-eligible agents
- A reader cannot trigger any agent actions
- Agents never have RBAC roles — they have trust scores

## Notification System (gpp-notify)

Event-driven notifications for team awareness.

### Event Types

| Event | Default Action | Configurable |
|-------|---------------|-------------|
| `changeset.promoted` | Log | Notify module owners |
| `review.requested` | Notify reviewers | Channel message |
| `review.approved` | Notify author | — |
| `review.rejected` | Notify author | — |
| `policy.violation` | Notify author + maintainers | Block + alert channel |
| `trust.score_changed` | Log | Notify if below threshold |
| `trust.agent_blocked` | Notify maintainers | Alert channel |
| `anomaly.detected` | Notify author | Alert based on severity |
| `sync.conflict` | Notify affected users | — |
| `graphex.update_proposed` | Notify maintainers | — |
| `cost.budget_alert` | Notify owner | Alert channel |

### Integration Backends

```toml
# .gpp/notify/integrations.toml
[slack]
webhook = "https://hooks.slack.com/services/T.../B.../xxx"
channel = "#webapp-dev"
events = ["policy.violation", "anomaly.detected", "trust.agent_blocked"]

[discord]
webhook = "https://discord.com/api/webhooks/..."
events = ["changeset.promoted", "review.requested"]

[email]
smtp = "smtp.example.com:587"
from = "gpp@example.com"
events = ["cost.budget_alert"]

[webhook]
url = "https://ci.example.com/hooks/gpp"
events = ["changeset.promoted"]  # Trigger CI builds
secret = "hmac-secret-here"      # HMAC signature for verification

[jira]
base_url = "https://example.atlassian.net"
project = "PROJ"
on_promote = "transition:in-review"
on_merge = "transition:done"
attach_semantic_diff = true

[linear]
api_key_env = "LINEAR_API_KEY"   # Read from environment, never stored
on_promote = "update_status"
```

### Inbox

```bash
gpp inbox                         # Show unread notifications
gpp inbox --unread                # Count only
gpp inbox ack 42                  # Acknowledge notification
gpp inbox ack --all               # Acknowledge all
```

### Webhook Protocol

Outgoing webhooks use HMAC-SHA256 signatures:

```
POST /hooks/gpp
Content-Type: application/json
X-GPP-Signature: sha256=<hmac>
X-GPP-Event: changeset.promoted

{
  "event": "changeset.promoted",
  "changeset": "3fa9b2kx...",
  "author": { "type": "agent", "name": "claude-code" },
  "intent": "Fix orders rounding error",
  "task": "PROJ-2847",
  "timestamp": 1747382400000000,
  "semantic_changes": [...]
}
```

## Remote Platform Integration (gpp-remote)

Platform-agnostic layer for interacting with GitHub, GitLab, Bitbucket, and other Git hosting platforms.

### Architecture

```rust
pub trait RemotePlatform: Send + Sync {
    fn name(&self) -> &str;
    fn create_pr(&self, changeset: &Changeset, opts: PrOptions) -> Result<PrRecord>;
    fn list_prs(&self, filters: PrFilters) -> Result<Vec<PrRecord>>;
    fn approve_pr(&self, pr_id: &str) -> Result<()>;
    fn merge_pr(&self, pr_id: &str, strategy: MergeStrategy) -> Result<()>;
    fn sync_reviews(&self, changeset: &Changeset) -> Result<Vec<ReviewSync>>;
    fn get_ci_status(&self, ref_name: &str) -> Result<CiStatus>;
    fn link_issue(&self, changeset: &Changeset, issue_id: &str) -> Result<()>;
    fn post_comment(&self, pr_id: &str, body: &str) -> Result<()>;
}
```

Implementations:
- `GitHubRemote` — uses GitHub REST/GraphQL API via `octocrab`
- `GitLabRemote` — uses GitLab API
- `BitbucketRemote` — uses Bitbucket API
- `GenericGitRemote` — bare Git push/pull, no platform features

### Configuration

```toml
# .gpp/config.toml
[remote]
platform = "github"                    # "github" | "gitlab" | "bitbucket" | "generic"
api_token_env = "GITHUB_TOKEN"         # Read from environment variable
repository = "acme/webapp"

[remote.pr]
auto_create = true                     # Auto-create PR on promote + push
include_intent = true                  # Include intent metadata in PR body
include_semantic_diff = true           # Include semantic diff summary
include_agent_meta = true              # Include agent info
include_policy_results = true          # Include policy check results
include_cost = true                    # Include cost attribution
draft = false                          # Create as draft PR

[remote.sync]
mirror_reviews = true                  # Bidirectional review sync
mirror_comments = true                 # Bidirectional comment sync
import_ci_status = true                # Pull CI status into gpp metadata
```

### PR Enrichment

When `gpp remote pr create` runs (or auto-triggers on promote), the PR body is enriched with gpp metadata:

```markdown
## Intent
Fix orders rounding error in batch processing

## Semantic Changes
- **Modified**: `orders::batch_processor::calculate_total()` — switched from f64 to i64 arithmetic
- **Added**: `orders::money_utils::round_cents()` — new rounding utility
- **Renamed**: `orders::types::Amount` → `orders::types::MoneyAmount` across 12 files

## Agent
- Tool: Claude Code
- Model: claude-sonnet-4-20250514
- Trust Score: 94.2 (auto-merge eligible)
- Session Duration: 8m 42s
- Token Cost: $1.24

## Policy Results
✅ secrets-scan: passed
✅ pci-dss: passed
✅ soc2: passed

## Task
Linked to PROJ-2847
```

### GitHub CLI Extension (gh-gpp)

A `gh` extension that bridges gpp commands into the GitHub workflow:

```bash
gh extension install gpp-vcs/gh-gpp

gh gpp promote                    # Promote + push + create PR with enriched body
gh gpp review                     # Review changeset with Graphex context
gh gpp trust                      # Show agent trust scores in PR comment
gh gpp cost                       # Add cost attribution as PR comment
gh gpp audit                      # Generate audit report linked to issue
gh gpp sync                       # Pull GitHub changes into local gpp history
```

The extension is a separate repo (`extensions/gh-gpp/`) written in Go (gh extension convention) that shells out to `gpp` CLI with `--json` output and calls the GitHub API.

### Graphex via GitHub (Optional)

Encrypted Graphex data can optionally be stored inside the GitHub repo in a `.gpp/` directory:

```
.gpp/graphex/        # Committed to Git, pushed to GitHub
├── graph.db          # Encrypted — GitHub can't read it
├── keys/             # Encrypted key envelopes
└── nodes/            # Encrypted node blobs
```

GitHub stores it as opaque binary. Teammates who clone and have gpp installed get the full knowledge graph decrypted locally. Teams wanting strict separation keep Graphex on their relay only:

```toml
# .gpp/config.toml
[graphex]
distribute_via = "relay"    # "relay" | "git" | "both"
```

## Terminal UI (gpp-tui)

Interactive terminal interface built with `ratatui`, inspired by `lazygit` and `gitui`.

```bash
gpp ui
```

### Panels

| Panel | Content | Keybindings |
|-------|---------|-------------|
| Timeline | Live-updating file changes | `j/k` navigate, `p` promote, `d` diff |
| History | Changeset DAG with semantic summaries | `Enter` expand, `r` review, `m` merge |
| Graphex | Interactive knowledge graph explorer | `q` query, `a` add node, `l` link |
| Agents | Active sessions, trust scores | `t` trust details, `b` block |
| Reviews | Pending reviews with diff preview | `a` approve, `c` request changes |
| Anomalies | Unresolved alerts | `Enter` details, `x` resolve |
| Cost | Token spend dashboard | `w` this week, `m` this month |
| Inbox | Notifications | `Enter` view, `Space` ack |

### Layout

```
┌─ Timeline ──────────────┬─ Diff Preview ─────────────┐
│ 14:03 agent:claude [3f] │ orders/batch.rs         │
│ 14:01 human:hasan  [2f] │ - let total = sum as f64;   │
│ 13:58 human:hasan  [1f] │ + let total = sum_paisa;    │
├─ History ───────────────┤                              │
│ cs:a3f9b2 Fix rounding  │                              │
│ cs:7e4d1c Add retry...  │                              │
├─ Agents ────────────────┼─ Inbox ─────────────────────┤
│ claude-code  94.2 ●     │ Review requested: cs:b4e2   │
│ copilot      81.7 ●     │ Anomaly: burst from codex   │
└─────────────────────────┴─────────────────────────────┘
```

## Adoption Path

gpp is designed for incremental adoption alongside existing Git/GitHub workflows:

### Week 1
Developer installs gpp, runs `gpp init --git-bridge` on existing GitHub repo. Team sees normal Git commits. Developer gets timeline capture and can explore Graphex privately.

### Month 1
Developer populates Graphex, connects Claude Code via MCP. Code quality improves. Teammates notice.

### Month 3
2-3 teammates adopt gpp. Share Graphex via encrypted directory in GitHub or relay. PRs get richer descriptions via `gh gpp promote`.

### Month 6
Team fully on gpp locally. GitHub remains remote for CI/CD, issues, external collaboration. Real workflow — capture, knowledge graph, governance — lives in gpp.

### Year 1
Optional migration to `gpp.dev` hosted platform or full P2P. GitHub becomes a mirror or is dropped. But this is always optional.
