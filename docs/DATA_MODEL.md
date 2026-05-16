# Data Model — gpp (git++)

## Hashing & Encoding

- Hash algorithm: BLAKE3 (256-bit)
- Display format: Base32 lowercase, 52 characters (e.g., `3fa9b2kx7m...`)
- Short display: First 8 characters when unambiguous (e.g., `3fa9b2kx`)
- Compression: zstd level 3 for all stored objects
- Encryption: age (filippo.io/age) for encrypted objects

## Core Object Types

### Blob

Raw file content. Identical to Git's blob concept.

```rust
struct Blob {
    content: Vec<u8>,
}
// Stored as: zstd(content)
// Hash: blake3(content)
```

### Tree

Directory listing. Each entry points to a Blob or another Tree.

```rust
struct Tree {
    entries: Vec<TreeEntry>,
}

struct TreeEntry {
    name: String,           // File or directory name
    kind: EntryKind,        // File | Directory | Symlink
    hash: Hash,             // Blob hash (file) or Tree hash (directory)
    mode: u32,              // Unix permissions
    size: u64,              // File size in bytes (0 for directories)
}

enum EntryKind {
    File,
    Directory,
    Symlink,
}
```

### Changeset

The primary unit of curated history. Replaces Git's commit.

```rust
struct Changeset {
    id: Hash,
    parents: Vec<Hash>,              // Multiple parents for merges
    tree: Hash,                      // Root Tree at this point
    timestamp: i64,                  // Unix microseconds UTC
    author: Author,
    committer: Option<Author>,       // If different from author
    intent: Option<Hash>,            // Link to Intent object
    timeline_range: Option<(i64, i64)>, // Timeline entries this covers
    semantic_changes: Vec<SemanticChange>,
    thread: Option<Hash>,            // ConversationThread hash
    cost: Option<CostRecord>,        // Token cost attribution
    replay_snapshot: Option<Hash>,   // ReplaySnapshot hash
    policy_results: Vec<PolicyResult>,
    signature: Option<Signature>,
    metadata: BTreeMap<String, String>,
}

struct Author {
    author_type: AuthorType,
    name: String,                    // Display name
    identity: String,                // Email (human) or agent ID string
    agent_meta: Option<Hash>,        // Link to AgentMeta (agents only)
}

enum AuthorType {
    Human,
    Agent,
}

struct Signature {
    algorithm: String,               // "ed25519" | "age"
    public_key: Vec<u8>,
    signature_bytes: Vec<u8>,
}
```

### Intent

Captures WHY a change was made. Linked from Changeset.

```rust
struct Intent {
    id: Hash,
    intent_type: IntentType,
    description: String,             // Human-readable summary
    prompt: Option<String>,          // The prompt that triggered AI work
    task_reference: Option<String>,  // Issue ID, ticket number, etc.
    goal: Option<String>,            // High-level objective
    constraints: Vec<String>,        // Stated constraints or requirements
    timestamp: i64,
}

enum IntentType {
    HumanDirected,                   // Human initiated the work
    AgentProposed,                   // Agent suggested the change
    PolicyTriggered,                 // Automated policy enforcement
    ReviewResponse,                  // Response to code review feedback
    BugFix,
    Feature,
    Refactor,
    Documentation,
    Dependency,
}
```

### AgentMeta

Identity and configuration snapshot for an AI agent session.

```rust
struct AgentMeta {
    id: Hash,
    agent_name: String,              // "claude-code" | "copilot" | "cursor" | etc.
    model_id: String,                // "claude-sonnet-4-20250514"
    model_version: Option<String>,   // Additional version info
    system_prompt_hash: Option<Hash>,// Hash of system prompt (not stored in full)
    rules_file_hash: Option<Hash>,   // Hash of .cursorrules / CLAUDE.md
    temperature: Option<f32>,
    max_tokens: Option<u64>,
    tools_available: Vec<String>,    // MCP tools the agent had access to
    context_window_size: Option<u64>,
    session_id: String,              // Unique per agent invocation
    started_at: i64,
    ended_at: Option<i64>,
}
```

### SemanticChange

A high-level description of what changed, computed by the semantic diff engine.

```rust
struct SemanticChange {
    change_type: SemanticChangeType,
    description: String,
    files: Vec<String>,              // Affected file paths
    old_hashes: Vec<Hash>,           // Before state
    new_hashes: Vec<Hash>,           // After state
}

enum SemanticChangeType {
    Add { path: String },
    Delete { path: String },
    Modify { path: String },
    Rename { old_path: String, new_path: String },
    Move { declaration: String, from_file: String, to_file: String },
    RenameSymbol { old_name: String, new_name: String, scope: String },
    Refactor { description: String },
    DependencyAdd { name: String, version: String },
    DependencyRemove { name: String },
    DependencyUpdate { name: String, old_version: String, new_version: String },
}
```

## Graphex Objects

### GraphNode

A single entity in the knowledge graph.

```rust
struct GraphNode {
    id: Hash,
    node_type: NodeType,
    name: String,                    // Human-readable identifier
    description: String,             // Rich description
    access_tier: AccessTier,         // Encryption/access level
    properties: BTreeMap<String, String>, // Extensible typed properties
    created_by: Author,
    created_at: i64,
    updated_at: i64,
    confidence: f32,                 // 0.0 - 1.0, how confident is this info
    validated_at: Option<i64>,       // Last human validation timestamp
    source: NodeSource,              // How this node was created
}

enum NodeType {
    Service,                         // Microservice, API, backend service
    Module,                          // Code module, package, crate
    Concept,                         // Domain concept (e.g., "order batch")
    Convention,                      // Coding convention or pattern
    ExternalSystem,                  // Third-party API, database, etc.
    Person,                          // Team member or stakeholder
    Policy,                          // Regulatory or business policy
    Schema,                          // Database schema, API schema
    Glossary,                        // Domain-specific term definition
    Decision,                        // Architecture decision record
}

enum AccessTier {
    Public,                          // Readable by anyone, including agents
    AgentReadable,                   // Agents can read projected context
    AgentRestricted,                 // Only available in trusted local environments
    HumanOnly,                       // Never projected to agents
}

enum NodeSource {
    HumanCreated,
    AgentProposed { agent_id: String, approved_by: Option<String> },
    AutoInferred { from_changeset: Hash },
    Federated { source_project: String },
}
```

### GraphEdge

A relationship between two nodes.

```rust
struct GraphEdge {
    id: Hash,
    from_node: Hash,
    to_node: Hash,
    relation: EdgeRelation,
    properties: BTreeMap<String, String>,
    created_by: Author,
    created_at: i64,
    confidence: f32,
    bidirectional: bool,
}

enum EdgeRelation {
    DependsOn,
    CommunicatesWith,
    OwnedBy,
    ImplementsPolicy,
    Contains,
    Uses,
    Contradicts,
    Supersedes,
    RelatedTo,
    FederatedFrom,
    Custom(String),
}
```

## Timeline Schema (SQLite)

```sql
-- Core timeline entries
CREATE TABLE timeline_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,          -- Unix microseconds UTC
    author_type TEXT NOT NULL CHECK(author_type IN ('human', 'agent')),
    author_id   TEXT NOT NULL,
    source      TEXT NOT NULL CHECK(source IN ('editor', 'cli', 'agent-sdk', 'fs-watch', 'import')),
    summary     TEXT,
    parent_id   INTEGER REFERENCES timeline_entries(id),
    promoted_to TEXT                        -- Changeset hash if promoted
);

CREATE INDEX idx_timeline_timestamp ON timeline_entries(timestamp);
CREATE INDEX idx_timeline_author ON timeline_entries(author_id);

-- Files changed in each entry
CREATE TABLE timeline_files (
    entry_id    INTEGER NOT NULL REFERENCES timeline_entries(id) ON DELETE CASCADE,
    file_path   TEXT NOT NULL,
    blob_hash   TEXT NOT NULL,
    change_type TEXT NOT NULL CHECK(change_type IN ('add', 'modify', 'delete', 'rename')),
    old_hash    TEXT,
    old_path    TEXT,                       -- For renames
    PRIMARY KEY (entry_id, file_path)
);

CREATE INDEX idx_timeline_files_path ON timeline_files(file_path);

-- Pruning metadata
CREATE TABLE timeline_retention (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    pruned_before INTEGER NOT NULL,        -- Timestamp up to which we pruned
    pruned_at   INTEGER NOT NULL,
    entries_removed INTEGER NOT NULL
);
```

## Trust Schema (SQLite)

```sql
CREATE TABLE agent_scores (
    agent_id        TEXT PRIMARY KEY,
    agent_name      TEXT NOT NULL,
    model_id        TEXT,
    trust_score     REAL NOT NULL DEFAULT 50.0,
    total_changesets INTEGER NOT NULL DEFAULT 0,
    survived_review INTEGER NOT NULL DEFAULT 0,
    regressions     INTEGER NOT NULL DEFAULT 0,
    first_seen      INTEGER NOT NULL,
    last_active     INTEGER NOT NULL,
    status          TEXT NOT NULL DEFAULT 'sandboxed'
                    CHECK(status IN ('auto-merge', 'review-required', 'sandboxed', 'blocked'))
);

CREATE TABLE agent_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_id    TEXT NOT NULL REFERENCES agent_scores(agent_id),
    event_type  TEXT NOT NULL,              -- 'changeset_merged', 'changeset_rejected', 'regression', 'graph_update_approved', etc.
    changeset   TEXT,
    details     TEXT,                       -- JSON blob
    timestamp   INTEGER NOT NULL
);

CREATE INDEX idx_agent_events_agent ON agent_events(agent_id, timestamp);
```

## Graphex Schema (SQLite)

```sql
-- Node index (actual content stored as encrypted blobs in object store)
CREATE TABLE graph_nodes (
    hash        TEXT PRIMARY KEY,
    node_type   TEXT NOT NULL,
    name        TEXT NOT NULL,
    access_tier TEXT NOT NULL DEFAULT 'public',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    confidence  REAL NOT NULL DEFAULT 1.0
);

CREATE INDEX idx_graph_nodes_type ON graph_nodes(node_type);
CREATE INDEX idx_graph_nodes_name ON graph_nodes(name);

-- Edge adjacency list
CREATE TABLE graph_edges (
    hash        TEXT PRIMARY KEY,
    from_node   TEXT NOT NULL REFERENCES graph_nodes(hash),
    to_node     TEXT NOT NULL REFERENCES graph_nodes(hash),
    relation    TEXT NOT NULL,
    bidirectional INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    confidence  REAL NOT NULL DEFAULT 1.0
);

CREATE INDEX idx_graph_edges_from ON graph_edges(from_node);
CREATE INDEX idx_graph_edges_to ON graph_edges(to_node);
CREATE INDEX idx_graph_edges_relation ON graph_edges(relation);

-- Access log for audit trail
CREATE TABLE graph_access_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,
    accessor_type TEXT NOT NULL,            -- 'human' | 'agent'
    accessor_id TEXT NOT NULL,
    action      TEXT NOT NULL,              -- 'read' | 'query' | 'project' | 'propose_update'
    nodes_accessed TEXT NOT NULL,           -- JSON array of node hashes
    projection_hash TEXT,                   -- Hash of the projected context sent to agent
    details     TEXT
);

CREATE INDEX idx_graph_access_time ON graph_access_log(timestamp);
CREATE INDEX idx_graph_access_accessor ON graph_access_log(accessor_id);
```

## Cost Schema (SQLite)

```sql
CREATE TABLE cost_records (
    changeset_id    TEXT PRIMARY KEY,
    agent_id        TEXT NOT NULL,
    model_id        TEXT NOT NULL,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    cached_tokens   INTEGER NOT NULL DEFAULT 0,
    cost_microdollars INTEGER NOT NULL DEFAULT 0,
    duration_ms     INTEGER NOT NULL DEFAULT 0,
    files_touched   INTEGER NOT NULL DEFAULT 0,
    lines_changed   INTEGER NOT NULL DEFAULT 0,
    lines_survived  INTEGER,                -- NULL until review complete
    timestamp       INTEGER NOT NULL
);

CREATE INDEX idx_cost_agent ON cost_records(agent_id);
CREATE INDEX idx_cost_timestamp ON cost_records(timestamp);

CREATE TABLE cost_budgets (
    module_pattern  TEXT PRIMARY KEY,        -- Glob pattern for file paths
    weekly_limit    INTEGER NOT NULL,        -- Microdollars
    alert_threshold REAL NOT NULL DEFAULT 0.8 -- Alert at 80% of limit
);
```

## Anomaly Schema (SQLite)

```sql
CREATE TABLE anomaly_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,
    rule_id     TEXT NOT NULL,
    severity    TEXT NOT NULL CHECK(severity IN ('info', 'warning', 'review', 'block')),
    agent_id    TEXT,
    changeset   TEXT,
    description TEXT NOT NULL,
    details     TEXT,                       -- JSON blob
    resolved    INTEGER NOT NULL DEFAULT 0,
    resolved_by TEXT,
    resolved_at INTEGER
);

CREATE INDEX idx_anomaly_time ON anomaly_events(timestamp);
CREATE INDEX idx_anomaly_unresolved ON anomaly_events(resolved) WHERE resolved = 0;
```

## Review Schema (SQLite)

```sql
CREATE TABLE reviews (
    id              TEXT PRIMARY KEY,       -- BLAKE3 hash
    changeset_id    TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK(status IN ('pending', 'approved', 'changes_requested', 'rejected', 'merged')),
    requested_by    TEXT NOT NULL,
    requested_at    INTEGER NOT NULL,
    merged_at       INTEGER,
    merged_by       TEXT,
    thread_hash     TEXT,                   -- ConversationThread object hash
    remote_pr_id    TEXT,                   -- GitHub PR number / GitLab MR ID
    remote_pr_url   TEXT
);

CREATE INDEX idx_reviews_changeset ON reviews(changeset_id);
CREATE INDEX idx_reviews_status ON reviews(status);

CREATE TABLE review_decisions (
    review_id       TEXT NOT NULL REFERENCES reviews(id),
    reviewer_id     TEXT NOT NULL,
    reviewer_type   TEXT NOT NULL CHECK(reviewer_type IN ('human', 'agent')),
    decision        TEXT NOT NULL CHECK(decision IN ('approve', 'request_changes', 'reject')),
    reason          TEXT,
    decided_at      INTEGER NOT NULL,
    PRIMARY KEY (review_id, reviewer_id)
);

CREATE TABLE review_comments (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    review_id       TEXT NOT NULL REFERENCES reviews(id),
    author_id       TEXT NOT NULL,
    author_type     TEXT NOT NULL CHECK(author_type IN ('human', 'agent')),
    file_path       TEXT,                   -- NULL for general comments
    line_number     INTEGER,                -- NULL for file-level or general
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    remote_comment_id TEXT                  -- Synced platform comment ID
);

CREATE INDEX idx_review_comments_review ON review_comments(review_id);
```

## Review Object

```rust
struct Review {
    id: Hash,
    changeset: Hash,
    status: ReviewStatus,
    requested_by: Author,
    requested_at: i64,
    reviewers: Vec<ReviewerRecord>,
    thread: Option<Hash>,
    policy_requirements: ReviewPolicy,
    remote_pr: Option<RemotePrLink>,
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
    comments: Vec<Hash>,
}

enum ReviewDecision {
    Approve,
    RequestChanges { reason: String },
    Reject { reason: String },
}

struct ReviewPolicy {
    min_reviewers: u32,
    require_human: bool,                 // At least one human reviewer
    require_owner: bool,                 // Require semantic code owner
    auto_assign_owners: bool,            // Auto-assign from Graphex ownership
}

struct RemotePrLink {
    platform: String,                    // "github" | "gitlab" | "bitbucket"
    pr_id: String,
    pr_url: String,
    synced_at: i64,
}
```

## RBAC Schema (SQLite)

```sql
CREATE TABLE roles (
    identity        TEXT PRIMARY KEY,       -- Email or key fingerprint
    role            TEXT NOT NULL DEFAULT 'reader'
                    CHECK(role IN ('owner', 'maintainer', 'contributor', 'reader')),
    assigned_by     TEXT NOT NULL,
    assigned_at     INTEGER NOT NULL,
    expires_at      INTEGER                 -- NULL = permanent
);

CREATE TABLE role_history (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    identity        TEXT NOT NULL,
    old_role        TEXT,
    new_role        TEXT NOT NULL,
    changed_by      TEXT NOT NULL,
    changed_at      INTEGER NOT NULL,
    reason          TEXT,
    changeset_id    TEXT                    -- Role changes are versioned as changesets
);

CREATE INDEX idx_role_history_identity ON role_history(identity);

-- Branch protection rules
CREATE TABLE branch_protections (
    branch_pattern  TEXT PRIMARY KEY,       -- Glob: "main", "release/*"
    min_reviewers   INTEGER NOT NULL DEFAULT 1,
    require_human   INTEGER NOT NULL DEFAULT 1,
    require_role    TEXT NOT NULL DEFAULT 'maintainer',  -- Min role to merge
    require_policy  INTEGER NOT NULL DEFAULT 1,          -- All policies must pass
    allow_agent_merge INTEGER NOT NULL DEFAULT 0         -- Can agents merge here?
);
```

## RBAC Object

```rust
struct Permission {
    identity: String,                    // Email or key fingerprint
    role: Role,
    assigned_by: String,
    assigned_at: i64,
    expires_at: Option<i64>,
}

enum Role {
    Owner,                               // Full control + key management
    Maintainer,                          // Review, merge, policy management
    Contributor,                         // Promote, propose, explore
    Reader,                              // Sync and read only
}

struct BranchProtection {
    branch_pattern: String,
    min_reviewers: u32,
    require_human: bool,
    require_role: Role,
    require_policy_pass: bool,
    allow_agent_merge: bool,
}
```

## Notification Schema (SQLite)

```sql
CREATE TABLE events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type      TEXT NOT NULL,          -- 'changeset.promoted', 'review.requested', etc.
    timestamp       INTEGER NOT NULL,
    actor_id        TEXT NOT NULL,
    actor_type      TEXT NOT NULL CHECK(actor_type IN ('human', 'agent', 'system')),
    target_type     TEXT NOT NULL,          -- 'changeset', 'review', 'trust', 'policy', etc.
    target_id       TEXT NOT NULL,
    summary         TEXT NOT NULL,
    details         TEXT,                   -- JSON blob
    dispatched      INTEGER NOT NULL DEFAULT 0  -- Sent to integrations?
);

CREATE INDEX idx_events_type ON events(event_type);
CREATE INDEX idx_events_time ON events(timestamp);
CREATE INDEX idx_events_undispatched ON events(dispatched) WHERE dispatched = 0;

CREATE TABLE notifications (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id        INTEGER NOT NULL REFERENCES events(id),
    recipient_id    TEXT NOT NULL,
    read            INTEGER NOT NULL DEFAULT 0,
    read_at         INTEGER,
    acknowledged    INTEGER NOT NULL DEFAULT 0,
    ack_at          INTEGER
);

CREATE INDEX idx_notifications_recipient ON notifications(recipient_id);
CREATE INDEX idx_notifications_unread ON notifications(read) WHERE read = 0;

CREATE TABLE integration_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id        INTEGER NOT NULL REFERENCES events(id),
    backend         TEXT NOT NULL,          -- 'slack', 'discord', 'webhook', 'email', 'jira'
    status          TEXT NOT NULL CHECK(status IN ('sent', 'failed', 'skipped')),
    response        TEXT,                   -- Response body or error message
    sent_at         INTEGER NOT NULL
);
```

## Notification Object

```rust
struct Event {
    id: u64,
    event_type: EventType,
    timestamp: i64,
    actor: Author,
    target_type: String,
    target_id: String,
    summary: String,
    details: Option<serde_json::Value>,
}

enum EventType {
    ChangesetPromoted,
    ReviewRequested,
    ReviewApproved,
    ReviewRejected,
    ReviewChangesRequested,
    PolicyViolation,
    TrustScoreChanged,
    TrustAgentBlocked,
    AnomalyDetected,
    SyncConflict,
    GraphexUpdateProposed,
    CostBudgetAlert,
}

struct Notification {
    id: u64,
    event_id: u64,
    recipient_id: String,
    read: bool,
    acknowledged: bool,
}
```

## Remote Platform Metadata

```rust
struct RemoteMeta {
    platform: PlatformType,
    repository: String,                  // "acme/webapp"
    pr_id: Option<String>,
    pr_url: Option<String>,
    ci_status: Option<CiStatus>,
    issue_refs: Vec<String>,             // ["PROJ-2847", "GH-123"]
    synced_at: i64,
}

enum PlatformType {
    GitHub,
    GitLab,
    Bitbucket,
    Generic,                             // Bare Git, no platform API
}

enum CiStatus {
    Pending,
    Running,
    Passed,
    Failed { details: String },
    Skipped,
}
```

## Configuration (TOML)

### Repository Config (.gpp/config.toml)

```toml
[core]
version = "0.1.0"
encryption = false            # Encrypt all blobs (not just Graphex)

[timeline]
enabled = true
debounce_ms = 100
retention_days = 30
ignore = [
    ".gpp/**",
    "node_modules/**",
    "target/**",
    ".git/**",
    "*.pyc",
    "__pycache__/**",
]

[graphex]
enabled = true
default_access_tier = "agent-readable"
federation = []               # Project IDs to federate with
distribute_via = "relay"      # "relay" | "git" | "both"

[trust]
auto_merge_min = 90
review_required_min = 70
sandbox_min = 50

[cost]
enabled = true
default_budget_weekly = 500_000_000  # $500 in microdollars

[review]
auto_assign_owners = true     # Auto-assign reviewers from Graphex ownership
min_reviewers = 1
require_human = true          # At least one human reviewer
auto_create_on_promote = true # Auto-create review when changeset is promoted

[sync]
peers = []
transport = "tcp+noise"
port = 9473

[relay]
enabled = false
address = ""                  # Relay node address
auto_push = true              # Auto-push on promote

[remote]
platform = "github"           # "github" | "gitlab" | "bitbucket" | "generic"
api_token_env = "GITHUB_TOKEN"
repository = ""               # e.g., "acme/webapp"

[remote.pr]
auto_create = true            # Auto-create PR on promote + push
include_intent = true
include_semantic_diff = true
include_agent_meta = true
include_policy_results = true
include_cost = true
draft = false

[remote.sync]
mirror_reviews = true         # Bidirectional review sync
mirror_comments = true
import_ci_status = true

[git-bridge]
enabled = true
remote = ""                   # Git remote URL to sync with
auto_sync = false
```

### Global Config (~/.config/gpp/config.toml)

```toml
[user]
name = "Jane Developer"
email = "owner@example.com"

[agent-defaults]
trust_initial_score = 50.0

[display]
hash_length = 8               # Short hash display length
color = true
time_format = "relative"      # "relative" | "iso" | "unix"
```

## Wire Format

Objects are serialized for storage and network transfer using a minimal binary format:

```
┌──────────────────────┐
│ Magic: "GPP\0" (4B)  │
│ Version: u8          │
│ Type: u8             │  Object type enum
│ Flags: u16           │  Compressed | Encrypted | Signed
│ Length: u32           │  Payload length
│ Payload: [u8]        │  zstd(msgpack(object)) or age(zstd(msgpack(object)))
│ Checksum: [u8; 4]    │  BLAKE3 truncated to 4 bytes
└──────────────────────┘
```

Serialization: MessagePack (compact, schema-less, fast Rust support via `rmp-serde`).
