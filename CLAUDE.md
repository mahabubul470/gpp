# CLAUDE.md — gpp (git++) Project Context

## What is gpp?

gpp (git++) is an AI-native version control system built in Rust. It replaces Git's commit-centric model with a continuous-capture architecture designed for a world where AI agents are first-class code contributors.

The core insight: Git was designed for humans making deliberate, sequential commits. AI agents produce changes continuously, across multiple files simultaneously, faster than humans can review. gpp is built for this reality.

## Project Structure

```
gpp/
├── CLAUDE.md                    # You are here
├── Cargo.toml                   # Workspace root
├── crates/
│   ├── gpp-core/                # Storage, objects, content-addressing
│   ├── gpp-timeline/            # Continuous change capture engine
│   ├── gpp-history/             # Curated changeset management + review workflow
│   ├── gpp-graphex/             # Encrypted knowledge graph
│   ├── gpp-trust/               # Agent reputation & behavioral RBAC
│   ├── gpp-policy/              # Compliance-as-code engine
│   ├── gpp-diff/                # Semantic diff engine (tree-sitter based)
│   ├── gpp-sync/                # CRDT-based P2P replication
│   ├── gpp-relay/               # Always-on relay node (sync hub, no special authority)
│   ├── gpp-cost/                # Token/compute cost attribution
│   ├── gpp-deps/                # Dependency intelligence
│   ├── gpp-anomaly/             # Pattern detection & alerting
│   ├── gpp-replay/              # Reproducible agent environments
│   ├── gpp-notify/              # Event system, webhooks, chat/PM integrations
│   ├── gpp-remote/              # Platform integration (GitHub, GitLab, Bitbucket APIs)
│   ├── gpp-review/              # Code review workflow engine
│   ├── gpp-rbac/                # Human permission model (owner/maintainer/contributor/reader)
│   ├── gpp-cli/                 # CLI binary
│   ├── gpp-tui/                 # Terminal UI (ratatui-based, lazygit-style)
│   ├── gpp-sdk/                 # Agent SDK (Rust + FFI for Python/JS)
│   └── gpp-git-bridge/          # Git import/export compatibility
├── extensions/
│   ├── gh-gpp/                  # GitHub CLI (`gh`) extension
│   ├── vscode-gpp/              # VS Code extension (TypeScript)
│   └── neovim-gpp/              # Neovim plugin (Lua)
├── docs/
│   ├── ARCHITECTURE.md
│   ├── DATA_MODEL.md
│   ├── CLI_SPEC.md
│   ├── GRAPHEX_PROTOCOL.md
│   ├── SYNC_PROTOCOL.md
│   ├── SECURITY_MODEL.md
│   ├── ROADMAP.md
│   └── CONTRIBUTING.md
├── parsers/                     # Tree-sitter grammars for semantic diff
│   ├── rust/
│   ├── python/
│   ├── typescript/
│   └── go/
├── policies/                    # Built-in compliance policy templates
│   ├── secrets-scan.policy
│   ├── pci-dss.policy
│   └── soc2.policy
└── tests/
    ├── integration/
    ├── fixtures/
    └── benchmarks/
```

## Architecture Overview

gpp is organized as 12 core layers + 5 integration layers, each a separate Rust crate in a Cargo workspace:

### Core Layers
1. **Storage** (gpp-core) — Content-addressed encrypted blob store
2. **Timeline** (gpp-timeline) — Continuous append-only file change capture
3. **History** (gpp-history) — Curated changesets promoted from timeline
4. **Graphex** (gpp-graphex) — Encrypted versioned knowledge graph
5. **Trust** (gpp-trust) — Agent reputation scoring and behavioral RBAC
6. **Policy** (gpp-policy) — Compliance-as-code enforcement at storage layer
7. **Semantic Diff** (gpp-diff) — Tree-sitter based structural code diffing
8. **Sync** (gpp-sync) — CRDT-based offline-first P2P replication
9. **Cost** (gpp-cost) — Token and compute cost attribution per changeset
10. **Dependencies** (gpp-deps) — Live risk-aware dependency intelligence
11. **Anomaly** (gpp-anomaly) — Agent behavior pattern detection
12. **Replay** (gpp-replay) — Reproducible agent environment snapshots

### Integration Layers
13. **Relay** (gpp-relay) — Always-on sync hub (not a server with authority — just a persistent peer)
14. **Review** (gpp-review) — Code review workflow (pending/approved/rejected/changes-requested)
15. **RBAC** (gpp-rbac) — Human permission model (owner/maintainer/contributor/reader)
16. **Notify** (gpp-notify) — Event system with webhooks, Slack/Discord/email integration
17. **Remote** (gpp-remote) — Platform integration layer (GitHub, GitLab, Bitbucket APIs)

### Client Interfaces
- **CLI** (gpp-cli) — Primary command-line interface
- **TUI** (gpp-tui) — Interactive terminal UI (ratatui-based, lazygit-style)
- **SDK** (gpp-sdk) — Agent SDK with Rust + Python + JS bindings
- **MCP Server** — Model Context Protocol server for AI tool integration
- **gh-gpp** — GitHub CLI extension
- **vscode-gpp** — VS Code extension
- **neovim-gpp** — Neovim plugin

### Agent Interaction Tiers
gpp supports three tiers of AI agent integration:
- **Tier 1 (Passive):** Agent edits files normally, timeline captures everything. Zero config. Works with any AI tool.
- **Tier 2 (Context-aware):** Agent connects via MCP, queries Graphex for project context. Better changes because of context.
- **Tier 3 (Native):** Agent uses gpp SDK directly — creates exploration branches, proposes changesets with intent, reports costs.

### Deployment Modes
- **Mode 1 (Serverless P2P):** Direct peer-to-peer sync. No hosted dependency.
- **Mode 2 (Relay):** `gpp-relay` binary as always-online sync hub. Any team member can run one. Stores encrypted objects, never decrypts.
- **Mode 3 (Hosted Platform):** Optional `gpp.dev` web UI for browsing, reviewing, dashboards. Everything works without it.

### GitHub Integration Strategy
gpp treats GitHub as a first-class sync target, not a competitor. The Git bridge pushes/pulls normal Git commits. Graphex, timeline, trust, cost, and policies live locally — GitHub only sees clean Git history. Optional GitHub API integration via `gpp remote` enables auto-opening PRs with rich descriptions, syncing issue references, and CI/CD hooks.

## Key Design Decisions

- **Rust only.** Performance, memory safety, single binary distribution. No runtime dependencies.
- **Content-addressed storage** using BLAKE3 hashing (faster than SHA-256, cryptographically secure).
- **Encryption via age (filippo.io/age)** for the Graphex layer and sensitive metadata.
- **Tree-sitter** for language-aware semantic diffing — pluggable parsers per language.
- **CRDT (Conflict-free Replicated Data Types)** for sync — specifically Automerge-style operation-based CRDTs.
- **SQLite** for local indexes (timeline index, graph adjacency, trust scores) — embedded, no server.
- **Git bridge** for import/export compatibility — never break existing workflows.
- **MCP (Model Context Protocol)** for agent SDK integration — agents query Graphex via MCP server.

## Conventions

- All monetary/token cost values stored as integers (micro-dollars, 1 = $0.000001)
- All timestamps are UTC, stored as i64 Unix microseconds
- All IDs are BLAKE3 hashes, displayed as base32 (human-readable, case-insensitive)
- Error handling uses `thiserror` for library crates, `anyhow` for CLI
- Logging uses `tracing` crate throughout
- Config files use TOML format
- Test coverage target: 80%+ for core crates, 60%+ for CLI

## Build & Run

```bash
cargo build --release
cargo test --workspace
cargo run --bin gpp -- init --graphex
```

## What NOT to Do

- Do NOT add any C dependencies — pure Rust or Rust bindings only
- Do NOT use async in the storage layer — keep it synchronous for simplicity
- Do NOT store plaintext secrets anywhere, even in test fixtures
- Do NOT break Git bridge compatibility without explicit approval
- Do NOT use unwrap() in library code — always propagate errors
