# Roadmap — gpp (git++)

## Implementation Phases

The project is divided into 6 phases, each delivering a usable increment. Each phase has a clear "you can use it for X" milestone.

---

## Phase 0: Foundation (Weeks 1-3)

**Goal:** Cargo workspace compiles, core object store works, basic CLI scaffolding.

**Status: ✅ Complete** (commit `a393efb`).

### Deliverables
- [x] Cargo workspace with all crate stubs
- [x] `gpp-core`: Content-addressed object store (BLAKE3, zstd compression)
  - [x] Blob, Tree object types
  - [x] Read/write objects to `.gpp/objects/`
  - [x] Object validation (hash verification)
- [x] `gpp-cli`: Binary scaffold with clap
  - [x] `gpp init` — create `.gpp/` directory structure
  - [x] `gpp status` — basic status output
  - [x] `gpp config` — read/write TOML config
- [x] `.gpp/` directory layout established
- [x] CI pipeline: `cargo test`, `cargo clippy`, `cargo fmt`

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

**Status: ✅ Complete** (commit `ea974a9`).

### Deliverables
- [x] `gpp-timeline`:
  - [x] File system watcher (notify crate)
  - [x] Debouncing (100ms default)
  - [x] SQLite timeline database (WAL mode)
  - [x] Timeline entry creation (author, source, files, hashes)
  - [x] `.gppignore` support (common `.gitignore` subset — see note)
  - [x] Timeline pruning (configurable retention)
- [x] `gpp-history`:
  - [x] Changeset object type
  - [x] Intent object type
  - [x] Author (Human/Agent) enum
  - [x] Promote timeline entries → changeset
  - [x] Changeset DAG (parents, branching)
  - [x] Branch refs
- [x] `gpp-diff`:
  - [x] Line-based diff (fallback)
  - [x] Basic file diff display (unified format)
- [x] CLI commands:
  - [x] `gpp timeline` — view timeline entries
  - [x] `gpp timeline watch` — live stream
  - [x] `gpp promote` — promote to changeset
  - [x] `gpp log` — view changeset history
  - [x] `gpp diff` — show changes
  - [x] `gpp branch` — create/switch/list branches

### Milestone
A developer can work on code, see continuous timeline capture, promote meaningful changes to history, and browse changeset history. **This is the "better Git for solo developers" milestone.**

### Dependencies (new)
- `notify` — file system events
- `rusqlite` — SQLite
- `similar` — diff algorithm
- `globset`, `walkdir` — added for `.gppignore` matching and tree walking (pure Rust)

### Implementation notes / deviations
- `.gppignore` implements the common `.gitignore` subset (negation, root vs.
  basename anchoring, `**`/`*`/`?`, directory patterns), not every edge case.
- Rename detection is recorded as delete + add for now; the `rename` change
  type exists in the schema for a later pass.
- `promote --interactive/--auto-summarize/--sign` are rejected with a clear
  message (depend on AI/signing layers in later phases).

---

## Phase 2: Semantic Diff + Git Bridge (Weeks 8-11)

**Goal:** Semantic diffing works for Phase 1 languages. Git repos can be imported.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-diff` (enhanced):
  - [x] Tree-sitter integration
  - [x] AST parsing for Rust, TypeScript, Python, Go
  - [x] Declaration fingerprinting (full + name-blanked body fingerprint)
  - [x] Cross-file move detection
  - [x] Symbol rename detection
  - [x] Semantic diff display format
  - [x] Plugin interface (`LanguageParser` trait)
- [x] `gpp-git-bridge`:
  - [x] `gpp git-import` — import Git history to gpp
  - [x] `gpp git-export` — export gpp history as Git
  - [x] Hash mapping database (SQLite)
  - [x] Bidirectional sync mode (`gpp git-bridge --watch`)
- [x] CLI updates:
  - [x] `gpp diff --semantic` (default for supported languages)
  - [x] `gpp git-import`, `gpp git-export`, `gpp git-bridge`

### Milestone
Developers can import existing Git repos and immediately see better diffs. **This is the "drop-in improvement over Git" milestone.**

### Dependencies (new)
- `tree-sitter` + `tree-sitter-{rust,python,typescript,go}` grammars
- `streaming-iterator` — tree-sitter 0.24 query iteration
- `git2` (libgit2 bindings) — for Git bridge

### Implementation notes / deviations
- Declaration extraction is query-driven per language; adding a language is a
  grammar + a declaration query. Nested items (e.g. impl methods) are captured
  too. Fingerprints normalize trailing whitespace and blank edges, so pure
  reformatting is reported as no semantic change.
- Rename/move detection is fingerprint-based: two declarations with an
  identical name-blanked body are treated as the same symbol. Trivial bodies
  (e.g. two empty functions) can therefore look like a rename — this is the
  expected similarity-heuristic trade-off, mirrored from Git's own heuristics.
- `git-import`/`git-export` traverse the **first-parent** chain; the hash map
  keys commits by their oid in the *bridged* repo. Import-from-A then
  export-to-a-different-repo-B reuses A's oids (correct for the single-remote
  bridge model; cross-repo migration would need a fresh map).
- `git-bridge --watch` is poll-based (HEAD-oid change detection on an
  interval); `--export` opts into pushing gpp changes back each cycle.
  Continuous operation-level bidirectional CRDT sync remains Phase 5.

---

## Phase 3: Graphex + Encryption (Weeks 12-17)

**Goal:** The encrypted knowledge graph works. Agents can query it via MCP.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-graphex`:
  - [x] GraphNode and GraphEdge object types
  - [x] SQLite adjacency index
  - [x] Envelope encryption (age master + per-tier AES-256-GCM)
  - [x] Access tier system (public, agent-readable, agent-restricted, human-only)
  - [x] Key hierarchy and key management
  - [x] Node lifecycle (proposed → active → deprecated → archived)
  - [x] Graph query engine (path pattern language)
  - [x] Context projection engine
    - [x] Subgraph selection
    - [x] Tier filtering
    - [x] Scrubbing (over-tier nodes never decrypted/shown)
    - [x] Token budget truncation
  - [x] Graph access audit log
  - [x] Auto-inference from changed paths (propose nodes for new modules)
  - [x] Manual node/edge CRUD via CLI
- [x] `gpp-sdk` (initial):
  - [x] Rust SDK for agent integration
  - [x] `AgentSession` struct
  - [x] `query_graphex()`, `propose_changeset()`, `propose_graph_update()`
- [x] MCP server (initial):
  - [x] `gpp mcp-server --stdio`
  - [x] `graphex_query`, `graphex_status`, `graphex_glossary`, `graphex_conventions` tools
  - [x] `propose_changeset`, `propose_graph_update` tools
- [x] CLI commands:
  - [x] `gpp graphex` (status/query/project/add/link/show/list/pending/accept/reject/audit/infer)
  - [x] `gpp mcp-server`
  - [x] `gpp keys` (generate, rotate, show)

### Milestone
AI tools (Claude Code, Cursor, etc.) can connect via MCP, query the knowledge graph, and propose changes. **This is the "AI-native" milestone — the core differentiator.**

### Dependencies (new)
- `age` — master-identity envelope encryption
- `aes-gcm` — per-tier symmetric node encryption
- `getrandom` — key/nonce generation
- custom MCP implementation (JSON-RPC 2.0 over newline-delimited stdio; no
  external MCP SDK — keeps the pure-Rust, single-binary constraint)

### Implementation notes / deviations
- Encrypted nodes are stored as ordinary content-addressed `Blob`s
  (`wire(zstd(msgpack))` sealed with the tier key); `graph.db` indexes
  metadata + a pointer to the current blob. This avoided changing the
  gpp-core wire format / `ObjectType` set. Node identity is *stable*
  (`blake3("{type}:{name}")`) so edits re-encrypt the same logical node and
  keep its edges; old blobs remain in object history.
- `master.age` stores the X25519 identity directly and `human-only` is
  master-sealed like other tiers — passphrase-wrapping of the master key and
  passphrase-gated `human-only` is a later hardening pass (the tier is still
  fully scrub-enforced in projection today).
- Query results are metadata-only (names/types/relations) and never decrypt
  content; decryption happens exclusively in the tier-gated projection path,
  which writes a `graph_access_log` entry (accessor, nodes, projection hash).
- Auto-inference keys off changed file *paths* of the HEAD changeset
  (`gpp graphex infer`), proposing `Module` nodes; richer semantic-diff-driven
  edge inference is a future enhancement. `AddEdge` proposals are applied
  directly (edges carry no secret content); `AddNode` requires human approval.
- Federation (publish/subscribe subgraphs) is intentionally deferred to
  Phase 5 alongside the CRDT sync protocol, per the roadmap’s own ordering.

---

## Phase 4: Trust + Policy + Cost (Weeks 18-22)

**Goal:** Agent governance works. Compliance-as-code enforced.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-trust`:
  - [x] Agent score database (SQLite)
  - [x] Score calculation (reviewed-outcome Bayesian model: survival vs. regression)
  - [x] Trust policy configuration (thresholds)
  - [x] Automatic status transitions (auto-merge, review-required, sandboxed, blocked)
  - [x] Module-level trust overrides
  - [x] Trust event logging
- [x] `gpp-policy`:
  - [x] Policy file parser (.policy TOML format)
  - [x] Pattern-based rules (regex on file content)
  - [x] Changeset-based rules (author, files, review requirements)
  - [x] Enforcement points: promotion (block/warn/audit) wired into `gpp promote`
  - [x] Built-in policy templates (secrets-scan, pci-dss, soc2)
  - [x] Custom policy support
- [x] `gpp-cost`:
  - [x] Cost record database (SQLite)
  - [x] Token tracking per changeset
  - [x] Budget configuration and alerts
  - [x] Cost analytics queries (summary + breakdown)
  - [x] Efficiency metrics (cost per survived line)
- [x] `gpp-anomaly`:
  - [x] Detection rules (unusual-scope, burst-activity, large-changeset)
  - [x] Event logging and alerting
  - [x] Resolution workflow + tunable rule thresholds
- [x] CLI commands:
  - [x] `gpp trust` (show/history/policy/override/reset)
  - [x] `gpp policy` (list/show/add/template/templates/remove/validate/check)
  - [x] `gpp cost` (summary/breakdown/efficiency/budget/budget-alert)
  - [x] `gpp anomaly` (list/history/resolve/rules/configure)
  - [x] `gpp audit` — cross-layer audit report (trust + anomaly + cost + graphex)

### Milestone
Teams can govern AI agent contributions with trust scores, enforce compliance policies, track costs, and detect anomalies. **This is the "enterprise-ready" milestone.**

### Implementation notes / deviations
- Trust score is based on **reviewed outcomes only** (survived vs. regression
  with a Beta(1,1)-style prior at 50); merely promoting a not-yet-reviewed
  changeset is not penalized. Survived/regression signals are recorded by
  `record_event`; the review layer (Phase 6) will drive them automatically —
  for now `gpp promote` records `changeset_promoted` for agent authors.
- Policy enforcement is wired at the **promotion** point (block aborts before
  any changeset object is written; warn/audit are reported). The timeline-
  capture (warn) and sync (block) enforcement points reuse the same
  `PolicySet` API and attach in Phases 1-revisit / 5 respectively.
- Cost records are created at promote time with tokens/cost = 0 ("unknown"
  model) until a Tier-3 SDK reports real usage; `lines_changed`/`files`
  are computed from the changeset delta. Budget attribution is repo-wide
  (per-path attribution lands with the review layer).
- Anomaly `burst-activity` uses changesets by the author reachable from HEAD
  in the last 24h as the window count.

---

## Phase 5: Sync Protocol (Weeks 23-28)

**Goal:** Peer-to-peer sync works. No GitHub dependency needed.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-sync`:
  - [x] Noise protocol handshake (Noise_XX via `snow`)
  - [x] State vector exchange (object id set, branch tips, policy set)
  - [x] Delta computation (set difference over state vectors)
  - [x] Object transfer (raw verified frames, chunked over Noise)
  - [x] History sync (changeset objects + ref reconcile)
  - [x] Graphex sync (OR-Set add / LWW metadata, zero-knowledge)
  - [x] Policy sync (add-only union by name)
  - [x] Conflict detection (divergent branch → `name.fork.<peer>`)
  - [x] Resume after connection loss (state exchange is idempotent/cheap)
  - [x] Peer authentication (TOFU static-key pinning)
  - [x] Peer permission model (known-peers gate; relay ACLs in Phase 7)
- [x] `gpp-replay`:
  - [x] Environment snapshot creation
  - [x] Snapshot storage as objects
  - [x] Replay re-materialization engine
  - [x] Diff between replay and original (drift detection)
- [x] Graphex federation:
  - [x] Publish/subscribe subgraphs (federated sources config + graph-only sync)
  - [x] Federated node lifecycle (rides OR-Set graphex sync)
  - [x] Cross-project sync (`gpp sync --graph-only`)
- [x] CLI commands:
  - [x] `gpp sync` (add/remove/status/serve/peer/default-all)
  - [x] `gpp replay` (dry-run/diff/output/env)
  - [x] `gpp graphex federation` (add/list), plus `gpp merge`

### Milestone
Teams can sync without GitHub. Multiple projects can federate knowledge. **This is the "decentralized" milestone.**

### Dependencies (new)
- `snow` — Noise protocol

### Implementation notes / deviations
- State exchange uses an explicit object-id set rather than a bloom filter
  (correct and simple at current scale; a bloom filter is a drop-in
  optimization later). The transport chunks messages so payloads larger
  than a 64 KiB Noise message transfer transparently.
- Ref conflicts are **fork-preserving** rather than Lamport-LWW: a divergent
  same-name branch is kept as `name.fork.<peer>` (gpp ref names disallow
  `@`, so the doc's `name@peer` becomes `name.fork.peer`). `gpp merge`
  resolves a fork into the current branch via a two-parent merge changeset
  taking the fork's tree (explicit, human-reviewed — never a silent merge).
- Graphex sync is zero-knowledge: encrypted node blobs ride the object set;
  only index metadata merges (node upsert keeps higher `updated_at`; edges
  add-only). A backup peer without tier keys still cannot read content.
- Trust and timeline are never synced (per `docs/SYNC_PROTOCOL.md`).
- `gpp-replay` reproduces *inputs* deterministically/offline (tree +
  captured toolchain/env). Re-executing the original agent is out of scope.
- Federation is config + graph-only sync; richer publish-filter globs and
  one-way federated read-only enforcement are a later hardening pass.

---

## Phase 6: Review + RBAC + Notifications (Weeks 29-34)

**Goal:** Collaboration workflow works. Teams can review, assign permissions, and get notified.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-review`:
  - [x] Review object type and SQLite schema
  - [x] Review lifecycle (pending → approved/changes_requested/rejected → merged)
  - [x] Reviewer suggestion (from RBAC owners/maintainers)
  - [x] Review comments with file/line targeting
  - [x] Review policy enforcement (RBAC merge-gate: reviewers/human/role/agent)
  - [x] Comment threads attached to a changeset's review
- [x] `gpp-rbac`:
  - [x] Role system (owner/maintainer/contributor/reader, ordered)
  - [x] Role assignment and revocation (with expiry)
  - [x] Branch protection rules (glob → min reviewers/human/role/agent)
  - [x] Enforcement at the CLI merge gate (`gpp review merge`)
  - [x] Role change auditing (`role_history`)
- [x] `gpp-notify`:
  - [x] Event system with typed events
  - [x] Notification database and inbox
  - [x] Integration backends: webhook/slack/discord (HTTP POST)
  - [x] HMAC-SHA256-signed outgoing webhooks (`X-Gpp-Signature`)
  - [x] Configurable per-backend event subscriptions
- [x] CLI commands:
  - [x] `gpp review` (list/show/request/approve/request-changes/reject/merge/comment/comments)
  - [x] `gpp rbac` (show/assign/revoke/whoami/protect/protections)
  - [x] `gpp inbox` (list/unread/ack/ack --all)
  - [x] `gpp notify` (integrations/add/remove/dispatch/events)

### Milestone
Teams can do code review inside gpp, manage permissions, and get notified via Slack/Discord/webhooks. **This is the "team collaboration" milestone.**

### Dependencies (new)
- `reqwest` (blocking) — webhook/chat delivery
- `hmac`, `sha2` — webhook signatures

### Implementation notes / deviations
- `gpp promote` auto-opens a review (config `[review].auto_create_on_promote`,
  default true) and emits a `changeset.promoted` event to suggested
  reviewers' inboxes (best-effort — never fails the promote).
- Reviewer suggestion uses RBAC owners/maintainers. Graphex *semantic*
  ownership-based assignment is deferred (the `owned-by` edge exists; wiring
  it as the primary source is a later enhancement).
- Conversation threads are modelled as the review's comment list rather than
  a separate hashed `ConversationThread` object (no gpp-core wire change).
- Email/Jira/Linear backends are not delivered: email needs SMTP creds and
  `lettre` is a heavy dependency, Jira/Linear need live APIs. The backend
  table + dispatch path are generic, so they slot in without schema change;
  webhook/slack/discord (HMAC-signed HTTP POST) are implemented and tested
  via an injected `Sender` (offline-deterministic unit test).
- Outbound HTTP is abstracted behind a `Sender` trait so dispatch is
  unit-testable without a network; the real `HttpSender` uses blocking
  `reqwest`.
- `gpp review merge` marks the review merged after an RBAC `can_merge`
  check; it does not rewrite history (the changeset was already promoted),
  keeping history append-only.

---

## Phase 7: Remote Platform Integration (Weeks 35-40)

**Goal:** gpp works seamlessly with GitHub, GitLab, and Bitbucket. The gh extension exists.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-remote`:
  - [x] Platform abstraction (`Platform` + injectable `HttpClient`)
  - [x] GitHub create-PR (REST `POST /repos/:repo/pulls`)
  - [x] GitLab create-MR (REST `/projects/:id/merge_requests`)
  - [x] Bitbucket create-PR (REST `/repositories/:repo/pullrequests`)
  - [x] `GenericGitRemote` (export + `git push`, no platform API)
  - [x] PR creation with gpp metadata enrichment (intent, semantic diff, agent, policy, cost, trust)
  - [~] Review/comment sync — payload builders ready; live bidirectional polling deferred
  - [~] CI status import — config plumbed; live status fetch deferred
  - [~] Issue linking — PR id/url captured; deeper linking deferred
  - [~] Graphex-over-Git distribution deferred (covered by `gpp sync --graph-only`)
- [x] `gh-gpp` extension:
  - [x] `gh gpp promote` — promote + push + create enriched PR
  - [x] `gh gpp review` — changeset + semantic diff + review context
  - [x] `gh gpp trust` — trust scores as a PR comment
  - [x] `gh gpp cost` — cost attribution as a PR comment
  - [x] `gh gpp audit` — audit report (optionally a gist)
  - [x] `gh gpp sync` — import the GitHub default branch into gpp
- [x] `gpp-relay`:
  - [x] Relay node binary (`gpp-relay`)
  - [x] Object storage and forwarding (wraps `gpp-sync::serve`)
  - [x] Peer authentication (Noise + repo-id gate + TOFU; auth-keys advisory)
  - [x] Docker image (`deploy/relay/Dockerfile`)
  - [x] Relay health endpoint (`GET /health` on `port+1`)
- [x] CI/CD integration:
  - [x] GitHub Action: `gpp-policy-check`
  - [x] GitHub Action: `gpp-trust-gate`
  - [x] GitHub Action: `gpp-audit-report`
  - [x] GitLab CI template (`ci/gitlab/gpp.gitlab-ci.yml`)
- [x] CLI commands:
  - [x] `gpp remote` (setup/status/pr-create/push)
  - [x] `gpp relay` (status/add/remove/push/pull)

### Milestone
Teams using GitHub/GitLab continue using their existing platform while getting gpp intelligence in PRs and CI. `gh gpp promote` is the easiest entry point. **This is the "GitHub-compatible" milestone — the adoption unlocker.**

### Implementation notes / deviations
- GitHub uses the REST API via blocking `reqwest` instead of `octocrab`
  (avoids pulling an async runtime; keeps the single-binary/pure-Rust
  posture). All three platforms share one request/response code path behind
  an injectable `HttpClient`, so PR creation is unit-tested fully offline
  with a mock (GitHub/GitLab/Bitbucket request shapes + result parsing).
- `gh-gpp` is a **Bash** `gh` extension (the `gh` convention runs any
  executable named `gh-<name>`); a Go rewrite is optional. It shells to
  `gpp`/`gh` and never pushes gpp metadata into the repo — it surfaces it
  *into* the PR as description/comments.
- The relay reuses `gpp-sync::serve` (Noise handshake, repo-id gate, TOFU).
  `--auth-keys` is honored as an advisory allowlist; pre-handshake key
  rejection needs a `gpp-sync` hook and is a later hardening pass.
- Bidirectional review/comment sync, live CI-status import and issue
  linking are scaffolded (config + payload builders) but not wired to live
  platform polling — they need authenticated network round-trips and are a
  follow-up; the milestone (enriched PRs + CI gating) is met.

### Dependencies (new)
- `octocrab` — GitHub API
- `go` toolchain — for gh extension (gh extension convention)

---

## Phase 8: TUI + Editor Extensions + Polish (Weeks 41-48)

**Goal:** Production-ready. Rich client interfaces. Documentation. Community launch.

**Status: ✅ Complete.**

### Deliverables
- [x] `gpp-tui`:
  - [x] Terminal UI with `ratatui` (`gpp ui`)
  - [x] Panels: timeline, history, graphex, agents, reviews, anomalies, cost, inbox
  - [x] Layout presets (default, minimal, review, monitoring)
  - [x] Live auto-refresh (toggle with `--no-live`)
  - [x] Panel navigation (focus by `--panel`, Tab/j/k)
  - [x] Keyboard-driven (q quit, r refresh); pure `Dashboard` is unit-tested
- [x] `vscode-gpp` extension:
  - [x] Timeline / Graphex / Reviews tree views (over `gpp`)
  - [x] Promote + semantic-diff commands; MCP via `gpp mcp-server --stdio`
- [x] `neovim-gpp` plugin:
  - [x] Lua plugin with Telescope pickers (fallback to `vim.ui.select`)
  - [x] Timeline / log / Graphex-query / review pickers, inline virtual text
- [x] Performance optimization:
  - [x] Benchmark suite (criterion: `gpp-core` object store, `gpp-diff` semantic)
  - [~] Latency targets — baselines established; tuning is ongoing
- [x] `gpp-deps`:
  - [x] Dependency list from lockfiles (Cargo.lock, package-lock.json)
  - [x] Heuristic offline risk score + notes
  - [x] Newly-added-dependency assessment (`gpp deps --since`)
  - [~] Live registry/CVE/license APIs deferred (network + keys)
- [x] SDK:
  - [x] Rust `gpp-sdk` (`AgentSession`) shipped in Phase 3
  - [~] Python/JS bindings: the CLI `--json` surface + `gpp-sdk` are the
    integration path; native PyO3/napi wrappers deferred (build tooling)
- [x] Plugin system:
  - [x] Language-parser plugin interface (`LanguageParser`) — `docs/PLUGINS.md`
  - [x] Policy template marketplace (`policies/`, `gpp policy template`)
  - [x] Compliance report formatters (stable `gpp audit` output → CI actions)
- [x] Documentation:
  - [x] User guide (mdbook, `docs/book/`)
  - [x] API reference (`cargo doc`; every crate has module docs)
  - [x] All six tutorials (migrate / graphex / mcp / compliance / github / relay)
- [x] Distribution:
  - [x] `cargo install` (gpp-cli, gpp-relay)
  - [x] Homebrew formula (`packaging/homebrew/gpp.rb`)
  - [x] Docker images (`deploy/gpp`, `deploy/relay`)
  - [x] Release workflow (binaries + GHCR images on tag)
  - [~] apt/dpkg packages deferred (tarball + Docker cover Linux)
- [x] Community:
  - [x] Contributing guide (`docs/CONTRIBUTING.md`)
  - [x] Issue templates (bug / feature / good-first-issue)
  - [~] Discord / logo / public launch — operational, not code

### Milestone
Public launch. Developers can install, migrate from Git, connect AI agents, collaborate via GitHub, use rich TUI/editor interfaces, and contribute to the ecosystem. **This is the "public launch" milestone.**

### Dependencies (new)
- `ratatui`, `crossterm` — terminal UI
- `criterion` — benchmarks (dev-only)

### Implementation notes / deviations
- The TUI splits a pure `Dashboard` snapshot (aggregated from the stores,
  unit-tested without a TTY) from a thin `ratatui` event loop; promote/
  approve *from* the TUI is deferred — the CLI remains the mutation surface.
- VS Code / Neovim extensions are thin shells over the `gpp` CLI (`--json`
  where available) rather than reimplementing logic — the CLI is the single
  source of truth; MCP context injection rides `gpp mcp-server --stdio`.
- `gpp-deps` is offline-only (lockfile parse + heuristic risk + newly-added
  diff). Live crates.io/npm/CVE/license APIs need network/keys and are a
  follow-up; the agent-dependency-assessment lens is implemented.
- Native Python/JS SDK bindings (PyO3/napi) are deferred: they need extra
  build toolchains. The `--json` CLI surface plus the Rust `gpp-sdk` are the
  supported integration paths today.
- Criterion benches establish baselines; the specific latency *targets*
  (timeline < 5ms, hot read < 1ms, 100k-object clone < 30s) are tracked as
  ongoing tuning, not a gate.

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
