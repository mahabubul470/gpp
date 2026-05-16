# Roadmap — gpp (git++)

## Implementation Phases

The project is divided into 6 phases, each delivering a usable increment. Each phase has a clear "you can use it for X" milestone.

---

## Phase 0: Foundation (Weeks 1-3)

**Goal:** Cargo workspace compiles, core object store works, basic CLI scaffolding.

### Deliverables
- [ ] Cargo workspace with all crate stubs
- [ ] `gpp-core`: Content-addressed object store (BLAKE3, zstd compression)
  - [ ] Blob, Tree object types
  - [ ] Read/write objects to `.gpp/objects/`
  - [ ] Object validation (hash verification)
- [ ] `gpp-cli`: Binary scaffold with clap
  - [ ] `gpp init` — create `.gpp/` directory structure
  - [ ] `gpp status` — basic status output
  - [ ] `gpp config` — read/write TOML config
- [ ] `.gpp/` directory layout established
- [ ] CI pipeline: `cargo test`, `cargo clippy`, `cargo fmt`

### Milestone
`gpp init` creates a valid repository. Objects can be stored and retrieved.

### Dependencies
- `blake3` — hashing
- `zstd` — compression
- `clap` — CLI argument parsing
- `serde`, `toml` — config serialization
- `anyhow`, `thiserror` — error handling
- `tracing` — logging

---

## Phase 1: Timeline + Basic History (Weeks 4-7)

**Goal:** Continuous file change capture works. Developers can promote timeline entries to changesets.

### Deliverables
- [ ] `gpp-timeline`:
  - [ ] File system watcher (notify crate)
  - [ ] Debouncing (100ms default)
  - [ ] SQLite timeline database (WAL mode)
  - [ ] Timeline entry creation (author, source, files, hashes)
  - [ ] `.gppignore` support (same syntax as `.gitignore`)
  - [ ] Timeline pruning (configurable retention)
- [ ] `gpp-history`:
  - [ ] Changeset object type
  - [ ] Intent object type
  - [ ] Author (Human/Agent) enum
  - [ ] Promote timeline entries → changeset
  - [ ] Changeset DAG (parents, branching)
  - [ ] Branch refs
- [ ] `gpp-diff`:
  - [ ] Line-based diff (fallback)
  - [ ] Basic file diff display (unified format)
- [ ] CLI commands:
  - [ ] `gpp timeline` — view timeline entries
  - [ ] `gpp timeline watch` — live stream
  - [ ] `gpp promote` — promote to changeset
  - [ ] `gpp log` — view changeset history
  - [ ] `gpp diff` — show changes
  - [ ] `gpp branch` — create/switch/list branches

### Milestone
A developer can work on code, see continuous timeline capture, promote meaningful changes to history, and browse changeset history. **This is the "better Git for solo developers" milestone.**

### Dependencies (new)
- `notify` — file system events
- `rusqlite` — SQLite
- `similar` — diff algorithm

---

## Phase 2: Semantic Diff + Git Bridge (Weeks 8-11)

**Goal:** Semantic diffing works for Phase 1 languages. Git repos can be imported.

### Deliverables
- [ ] `gpp-diff` (enhanced):
  - [ ] Tree-sitter integration
  - [ ] AST parsing for Rust, TypeScript, Python, Go
  - [ ] Declaration fingerprinting
  - [ ] Cross-file move detection
  - [ ] Symbol rename detection
  - [ ] Semantic diff display format
  - [ ] Plugin interface (`LanguageParser` trait)
- [ ] `gpp-git-bridge`:
  - [ ] `gpp git-import` — import Git history to gpp
  - [ ] `gpp git-export` — export gpp history as Git
  - [ ] Hash mapping database (SQLite)
  - [ ] Bidirectional sync mode (`gpp git-bridge --watch`)
- [ ] CLI updates:
  - [ ] `gpp diff --semantic` (default for supported languages)
  - [ ] `gpp git-import`, `gpp git-export`, `gpp git-bridge`

### Milestone
Developers can import existing Git repos and immediately see better diffs. **This is the "drop-in improvement over Git" milestone.**

### Dependencies (new)
- `tree-sitter` + language grammars
- `git2` (libgit2 bindings) — for Git bridge

---

## Phase 3: Graphex + Encryption (Weeks 12-17)

**Goal:** The encrypted knowledge graph works. Agents can query it via MCP.

### Deliverables
- [ ] `gpp-graphex`:
  - [ ] GraphNode and GraphEdge object types
  - [ ] SQLite adjacency index
  - [ ] Envelope encryption (age)
  - [ ] Access tier system (public, agent-readable, agent-restricted, human-only)
  - [ ] Key hierarchy and key management
  - [ ] Node lifecycle (proposed → active → deprecated → archived)
  - [ ] Graph query engine (path pattern language)
  - [ ] Context projection engine
    - [ ] Subgraph selection
    - [ ] Tier filtering
    - [ ] Scrubbing
    - [ ] Token budget truncation
  - [ ] Graph access audit log
  - [ ] Auto-inference from semantic diffs (propose nodes for new modules/services)
  - [ ] Manual node/edge CRUD via CLI
- [ ] `gpp-sdk` (initial):
  - [ ] Rust SDK for agent integration
  - [ ] `AgentSession` struct
  - [ ] `query_graphex()`, `propose_changeset()`, `propose_graph_update()`
- [ ] MCP server (initial):
  - [ ] `gpp mcp-server --stdio`
  - [ ] `graphex_query`, `graphex_status`, `graphex_glossary`, `graphex_conventions` tools
  - [ ] `propose_changeset`, `propose_graph_update` tools
- [ ] CLI commands:
  - [ ] `gpp graphex` (all subcommands)
  - [ ] `gpp mcp-server`
  - [ ] `gpp keys` (generate, rotate, show)

### Milestone
AI tools (Claude Code, Cursor, etc.) can connect via MCP, query the knowledge graph, and propose changes. **This is the "AI-native" milestone — the core differentiator.**

### Dependencies (new)
- `age` — encryption
- `aes-gcm` — symmetric encryption
- `mcp-sdk` or custom MCP implementation

---

## Phase 4: Trust + Policy + Cost (Weeks 18-22)

**Goal:** Agent governance works. Compliance-as-code enforced.

### Deliverables
- [ ] `gpp-trust`:
  - [ ] Agent score database (SQLite)
  - [ ] Score calculation (survival rate, regression rate, review approval, etc.)
  - [ ] Trust policy configuration
  - [ ] Automatic status transitions (auto-merge, review-required, sandboxed, blocked)
  - [ ] Module-level trust overrides
  - [ ] Trust event logging
- [ ] `gpp-policy`:
  - [ ] Policy file parser (.policy TOML format)
  - [ ] Pattern-based rules (regex on file content)
  - [ ] Changeset-based rules (author, files, review requirements)
  - [ ] Enforcement points: timeline capture (warn), promotion (block), sync (block)
  - [ ] Built-in policy templates (secrets-scan, pci-dss)
  - [ ] Custom policy support
- [ ] `gpp-cost`:
  - [ ] Cost record database (SQLite)
  - [ ] Token tracking per changeset
  - [ ] Budget configuration and alerts
  - [ ] Cost analytics queries
  - [ ] Efficiency metrics (cost per survived line)
- [ ] `gpp-anomaly`:
  - [ ] Detection rules (unusual-scope, burst-activity, large-changeset, etc.)
  - [ ] Event logging and alerting
  - [ ] Resolution workflow
- [ ] CLI commands:
  - [ ] `gpp trust` (all subcommands)
  - [ ] `gpp policy` (all subcommands)
  - [ ] `gpp cost` (all subcommands)
  - [ ] `gpp anomaly` (all subcommands)
  - [ ] `gpp audit` — comprehensive audit report generation

### Milestone
Teams can govern AI agent contributions with trust scores, enforce compliance policies, track costs, and detect anomalies. **This is the "enterprise-ready" milestone.**

---

## Phase 5: Sync Protocol (Weeks 23-28)

**Goal:** Peer-to-peer sync works. No GitHub dependency needed.

### Deliverables
- [ ] `gpp-sync`:
  - [ ] Noise protocol handshake (via snow crate)
  - [ ] State vector exchange (bloom filters, branch tips, vector clocks)
  - [ ] Delta computation
  - [ ] Object transfer (batched, compressed)
  - [ ] History sync (changeset DAG, ref updates via LWW)
  - [ ] Graphex sync (OR-Set CRDT, zero-knowledge)
  - [ ] Policy sync
  - [ ] Conflict detection and resolution
  - [ ] Resume after connection loss
  - [ ] Peer authentication (TOFU key exchange)
  - [ ] Peer permission model
- [ ] `gpp-replay`:
  - [ ] Environment snapshot creation
  - [ ] Snapshot storage as objects
  - [ ] Replay execution engine
  - [ ] Diff between replay and original
- [ ] Graphex federation:
  - [ ] Publish/subscribe subgraphs
  - [ ] Federated node lifecycle
  - [ ] Cross-project sync
- [ ] CLI commands:
  - [ ] `gpp sync` (all subcommands)
  - [ ] `gpp replay`
  - [ ] `gpp graphex federation`

### Milestone
Teams can sync without GitHub. Multiple projects can federate knowledge. **This is the "decentralized" milestone.**

### Dependencies (new)
- `snow` — Noise protocol
- `bloom` or custom — bloom filters
- `automerge` or custom — CRDT operations

---

## Phase 6: Review + RBAC + Notifications (Weeks 29-34)

**Goal:** Collaboration workflow works. Teams can review, assign permissions, and get notified.

### Deliverables
- [ ] `gpp-review`:
  - [ ] Review object type and SQLite schema
  - [ ] Review lifecycle (pending → approved/rejected → merged)
  - [ ] Auto-assign reviewers from Graphex semantic ownership
  - [ ] Review comments with file/line targeting
  - [ ] Review policy enforcement (min reviewers, require human, require owner)
  - [ ] ConversationThread integration (threads attached to changesets)
- [ ] `gpp-rbac`:
  - [ ] Role system (owner/maintainer/contributor/reader)
  - [ ] Role assignment and revocation
  - [ ] Branch protection rules
  - [ ] Enforcement at CLI and sync protocol levels
  - [ ] Role change auditing (role changes are changesets)
- [ ] `gpp-notify`:
  - [ ] Event system with typed events
  - [ ] Notification database and inbox
  - [ ] Integration backends: Slack, Discord, email, webhooks
  - [ ] HMAC-signed outgoing webhooks
  - [ ] Jira/Linear integration (status transitions on promote/merge)
  - [ ] Configurable event subscriptions per backend
- [ ] CLI commands:
  - [ ] `gpp review` (all subcommands)
  - [ ] `gpp rbac` (all subcommands)
  - [ ] `gpp inbox`
  - [ ] `gpp notify` (all subcommands)

### Milestone
Teams can do code review inside gpp, manage permissions, and get notified via Slack/Discord/webhooks. **This is the "team collaboration" milestone.**

### Dependencies (new)
- `lettre` — email sending
- `reqwest` — HTTP client for webhooks and platform APIs
- `hmac`, `sha2` — webhook signatures

---

## Phase 7: Remote Platform Integration (Weeks 35-40)

**Goal:** gpp works seamlessly with GitHub, GitLab, and Bitbucket. The gh extension exists.

### Deliverables
- [ ] `gpp-remote`:
  - [ ] `RemotePlatform` trait and platform abstraction
  - [ ] `GitHubRemote` implementation (REST + GraphQL API via `octocrab`)
  - [ ] `GitLabRemote` implementation
  - [ ] `BitbucketRemote` implementation
  - [ ] `GenericGitRemote` (bare Git, no platform API)
  - [ ] PR creation with gpp metadata enrichment (intent, semantic diff, agent meta, policy results, cost)
  - [ ] Bidirectional review sync (gpp reviews ↔ platform PR reviews)
  - [ ] Bidirectional comment sync
  - [ ] CI status import
  - [ ] Issue linking (changeset metadata → platform issue)
  - [ ] Optional: Graphex distribution via `.gpp/` directory in Git repo
- [ ] `gh-gpp` extension:
  - [ ] `gh gpp promote` — promote + push + create enriched PR
  - [ ] `gh gpp review` — review with Graphex context
  - [ ] `gh gpp trust` — trust scores as PR comment
  - [ ] `gh gpp cost` — cost attribution as PR comment
  - [ ] `gh gpp audit` — audit report linked to issues
  - [ ] `gh gpp sync` — pull GitHub changes into gpp
- [ ] `gpp-relay`:
  - [ ] Relay node binary (`gpp-relay`)
  - [ ] Object storage and forwarding
  - [ ] Peer authentication
  - [ ] Docker image
  - [ ] Relay status API (simple health check endpoint)
- [ ] CI/CD integration:
  - [ ] GitHub Actions: `gpp-policy-check` action
  - [ ] GitHub Actions: `gpp-trust-gate` action
  - [ ] GitHub Actions: `gpp-audit-report` action
  - [ ] GitLab CI template
- [ ] CLI commands:
  - [ ] `gpp remote` (all subcommands)
  - [ ] `gpp relay` (all subcommands)

### Milestone
Teams using GitHub/GitLab continue using their existing platform while getting gpp intelligence in PRs and CI. `gh gpp promote` is the easiest entry point. **This is the "GitHub-compatible" milestone — the adoption unlocker.**

### Dependencies (new)
- `octocrab` — GitHub API
- `go` toolchain — for gh extension (gh extension convention)

---

## Phase 8: TUI + Editor Extensions + Polish (Weeks 41-48)

**Goal:** Production-ready. Rich client interfaces. Documentation. Community launch.

### Deliverables
- [ ] `gpp-tui`:
  - [ ] Terminal UI with `ratatui`
  - [ ] Panels: timeline, history, graphex, agents, reviews, anomalies, cost, inbox
  - [ ] Layout presets (default, minimal, review, monitoring)
  - [ ] Live timeline updates
  - [ ] Interactive Graphex explorer
  - [ ] Keyboard-driven workflow (promote, review, approve from TUI)
- [ ] `vscode-gpp` extension:
  - [ ] Timeline sidebar panel (live updates)
  - [ ] Graphex tree view explorer
  - [ ] Inline annotations (agent authorship, trust scores, semantic ownership)
  - [ ] Semantic diff rendering in VS Code diff viewer
  - [ ] MCP context injection for VS Code AI features
  - [ ] Review workflow integration
- [ ] `neovim-gpp` plugin:
  - [ ] Lua plugin with telescope pickers
  - [ ] Timeline, Graphex query, changeset review pickers
  - [ ] Inline virtual text annotations
- [ ] Performance optimization:
  - [ ] Benchmark suite (criterion)
  - [ ] Timeline capture < 5ms latency target
  - [ ] Object store read < 1ms for hot cache
  - [ ] Sync initial clone < 30s for 100k objects
- [ ] `gpp-deps`:
  - [ ] Dependency graph from lockfiles (Cargo.lock, package-lock.json, etc.)
  - [ ] Registry API integration (crates.io, npm, PyPI)
  - [ ] CVE database integration
  - [ ] License compatibility check
  - [ ] Risk score computation
  - [ ] Auto-assessment on agent dependency additions
- [ ] SDK expansion:
  - [ ] Python bindings (via PyO3)
  - [ ] JavaScript/TypeScript bindings (via napi-rs)
  - [ ] SDK documentation and examples
- [ ] Plugin system:
  - [ ] Language parser plugin interface
  - [ ] Policy template marketplace
  - [ ] Compliance report formatters
- [ ] Documentation:
  - [ ] User guide (mdbook)
  - [ ] API reference (rustdoc)
  - [ ] Tutorial: "Migrating from Git to gpp"
  - [ ] Tutorial: "Setting up Graphex for your project"
  - [ ] Tutorial: "Connecting AI agents via MCP"
  - [ ] Tutorial: "Compliance with gpp for regulated industries"
  - [ ] Tutorial: "Using gpp with GitHub"
  - [ ] Tutorial: "Setting up a relay node for your team"
- [ ] Distribution:
  - [ ] `cargo install gpp`
  - [ ] Homebrew formula
  - [ ] apt/dpkg packages
  - [ ] Docker images (gpp + gpp-relay)
  - [ ] GitHub Actions marketplace
- [ ] Community:
  - [ ] GitHub repository public launch
  - [ ] Discord server
  - [ ] Contributing guide
  - [ ] Issue templates
  - [ ] First-timer friendly issues
  - [ ] Logo and brand assets

### Milestone
Public launch. Developers can install, migrate from Git, connect AI agents, collaborate via GitHub, use rich TUI/editor interfaces, and contribute to the ecosystem. **This is the "public launch" milestone.**

### Dependencies (new)
- `ratatui` — terminal UI framework
- `crossterm` — terminal input/output

---

## Stretch Goals (Post-Launch)

- [ ] Web UI for Graphex visualization (`gpp.dev` hosted platform — Mode 3)
- [ ] JetBrains plugin (IntelliJ, WebStorm, etc.)
- [ ] Agent orchestration layer (lead agent reviewing exploration branches)
- [ ] Agent-to-agent collaboration (agents reading each other's explorations)
- [ ] AI-powered changeset summarization (built-in, using local models)
- [ ] Multi-repo workspaces (monorepo support)
- [ ] Graphex schema validation (enforce graph structure rules)
- [ ] Time-travel debugging integration (link to production observability)
- [ ] REST/gRPC API on relay node (for web UIs and remote tools)
- [ ] Hosted relay service (managed relay for teams not wanting to self-host)
- [ ] Migration tools from other VCS (Mercurial, SVN, Perforce)
- [ ] Mobile app for review and inbox notifications

---

## Estimated Timeline

| Phase | Duration | Cumulative | Milestone |
|-------|----------|-----------|-----------|
| 0: Foundation | 3 weeks | Week 3 | Repository initializes |
| 1: Timeline + History | 4 weeks | Week 7 | Better Git for solo devs |
| 2: Semantic Diff + Git Bridge | 4 weeks | Week 11 | Drop-in Git improvement |
| 3: Graphex + Encryption | 6 weeks | Week 17 | AI-native core |
| 4: Trust + Policy + Cost | 5 weeks | Week 22 | Enterprise-ready |
| 5: Sync Protocol | 6 weeks | Week 28 | Decentralized |
| 6: Review + RBAC + Notifications | 6 weeks | Week 34 | Team collaboration |
| 7: Remote Platforms + Relay | 6 weeks | Week 40 | GitHub-compatible |
| 8: TUI + Editors + Polish | 8 weeks | Week 48 | Public launch |

**Total: ~48 weeks (12 months) to public launch.**

This assumes a single focused developer. With a small team (2-3), phases can overlap and the timeline compresses to 7-8 months. Key acceleration opportunities:
- Phases 6 and 7 can largely run in parallel (review/RBAC is internal, remote/relay is external)
- The `gh-gpp` extension can start as early as Phase 2 (once Git bridge works)
- TUI can start development alongside Phase 5 (sync) since it's UI over existing layers
