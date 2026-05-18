# gpp (git++)

An AI-native version control system built in Rust. gpp replaces Git's
commit-centric model with a continuous-capture architecture designed for a
world where AI agents are first-class code contributors.

See [`CLAUDE.md`](CLAUDE.md) for project context, [`docs/`](docs/) for the
full specification (architecture, data model, CLI, protocols, roadmap), and
[`docs/book/`](docs/book/) for the user guide + tutorials.

## Status

**All 9 phases (0–8) complete.** See [`docs/ROADMAP.md`](docs/ROADMAP.md)
for the per-phase deliverables and documented deviations. 123 workspace
tests pass; `cargo clippy` / `cargo fmt` clean. No stub crates remain.

| Layer | Crate | What's implemented |
|---|---|---|
| Storage | `gpp-core` | Content-addressed store (BLAKE3 + zstd), `Blob`/`Tree`, raw verified frame transfer |
| Timeline | `gpp-timeline` | SQLite (WAL) capture, `.gppignore`, debounced watcher, pruning |
| History | `gpp-history` | `Changeset`/`Intent`/`Author`, branch refs, promote, DAG walk |
| Diff | `gpp-diff` | Line + **tree-sitter semantic** diff (Rust/Python/TS/Go), rename/move detection |
| Git bridge | `gpp-git-bridge` | `git-import`/`git-export`/`git-bridge`, SQLite hash map |
| Graphex | `gpp-graphex` | Encrypted (age + AES-GCM) knowledge graph, tier-gated projection, query, lifecycle, audit |
| SDK / MCP | `gpp-sdk` | `AgentSession`; `gpp mcp-server --stdio` (JSON-RPC MCP) |
| Trust | `gpp-trust` | Reputation scoring, status transitions, overrides, events |
| Policy | `gpp-policy` | `.policy` TOML rules, promotion-time enforcement, built-in templates |
| Cost | `gpp-cost` | Per-changeset token/$ records, budgets, efficiency |
| Anomaly | `gpp-anomaly` | Scope/burst/size detection, resolution workflow |
| Sync | `gpp-sync` | Noise_XX P2P; objects/refs/policies/graphex; fork-preserve |
| Replay | `gpp-replay` | Reproducible environment snapshots + drift diff |
| Review/RBAC/Notify | `gpp-review` `gpp-rbac` `gpp-notify` | Review lifecycle, roles + branch protection, events/inbox/HMAC webhooks |
| Remote | `gpp-remote` | GitHub/GitLab/Bitbucket PR creation, enriched bodies, plain-Git push |
| Relay | `gpp-relay` | Always-on sync hub binary + health endpoint + Dockerfile |
| Clients | `gpp-cli` `gpp-tui` `gpp-deps` | Full CLI, ratatui TUI (`gpp ui`), dependency intel (`gpp deps`) |

Also: `extensions/{gh-gpp,vscode-gpp,neovim-gpp}`, GitHub Actions +
GitLab CI templates, `deploy/` Docker images, `packaging/` Homebrew.

Documented follow-ups (recorded in the ROADMAP, not silently skipped):
live registry/CVE/license APIs, native PyO3/napi bindings, bidirectional
platform-review polling, apt/dpkg packages, passphrase-wrapped master key.

## Install

```bash
cargo install --git https://github.com/gpp-vcs/gpp gpp-cli     # the `gpp` binary
cargo install --git https://github.com/gpp-vcs/gpp gpp-relay   # relay node
```

## Build & test

```bash
cargo build --release
cargo test --workspace
cargo bench -p gpp-core -p gpp-diff      # criterion perf suite
```

## Try it

```bash
# Solo-dev flow
gpp init --graphex .
echo "fn main() {}" > main.rs
gpp timeline                              # continuous capture
gpp promote -m "first cut" --intent feature
gpp log --oneline
gpp diff HEAD                             # semantic diff

# AI-native: connect an agent over MCP
gpp mcp-server --stdio

# Governance
gpp policy template secrets-scan
gpp trust show
gpp audit --include-cost --include-graphex

# Decentralized: sync two repos over Noise
gpp sync serve 127.0.0.1:9473            # on peer A
gpp sync add a 127.0.0.1:9473 && gpp sync # on peer B

# GitHub-compatible
gpp remote setup --platform github --repository acme/webapp
gpp remote pr-create --base main
```
