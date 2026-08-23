# TODO — gpp (git++) Next Work

Status snapshot: all 9 phases (0–8) implemented; 184 workspace tests pass;
clippy/fmt clean; line coverage 65.70% baseline (counts verified
2026-07-12). P0.1–P0.3
are now done — the remaining P0 work is the test-depth/integration pass
(P0.4–P0.5). This document is the single prioritized backlog for what
comes next. It consolidates the `[~]` partial items and "Implementation
notes / deviations" deferrals scattered through [`ROADMAP.md`](ROADMAP.md),
plus newly identified hardening work, into one actionable list.

Legend: **P0** = correctness/hardening before any real adoption ·
**P1** = deferred features promised in the roadmap ·
**P2** = post-launch stretch · `[ ]` open · `[~]` partially done.

---

## P0 — Hardening before real-world use

These are not new features; they are gaps between "milestone met" and
"safe to depend on". Ordered: a silent runtime gap that *misleads* a
user about protection outranks an internal quality gap; small high-risk
fixes outrank large infra efforts; measurement precedes the work it gates.

**▶ Next:** P0.4 continuation (`gpp-notify` / `gpp-rbac` / `gpp-replay`
depth) and a `gpp clone` command (P0.5 finding).

- [x] **P0.1 — Policy enforcement points beyond promote** (Phase 4
  deviation). Done 2026-06-23 (`gpp-cli`). Wired the existing `PolicySet`
  API into the two points that were previously no-ops: timeline-capture
  now surfaces violations as non-fatal warnings (`warn`; block-severity
  hits are flagged "will block promote/sync"), and `gpp sync` /
  `gpp sync serve` / `gpp sync peer` now run a `block`-severity gate over
  every branch-tip snapshot — the exact content `build_push` transmits —
  before a byte leaves the repo, aborting on any block violation.
  Catches the real scenario the promote-time check can't: content that
  reached a tip without passing the *current* policy (installed after the
  fact, or pulled in from a peer). The timeline `watch` callback now
  receives `&Timeline` so the CLI can scan the just-captured snapshot.
  Tests: `crates/gpp-cli/tests/policy_enforcement.rs` (5 e2e: timeline
  warn + clean, sync block via `sync`/`serve`, clean-sync-allowed).
- [x] **P0.2 — Relay pre-handshake key rejection** (Phase 7 deviation).
  Done 2026-06-23 (`gpp-sync` + `gpp-relay`). Added
  `gpp_sync::serve_with_auth(.., allow: Option<&[String]>)` (and a new
  `Error::Unauthorized`); `serve` delegates with `None` (TOFU-only,
  unchanged). The relay now passes its `--auth-keys` allowlist through, so
  a peer whose Noise static key is absent is rejected immediately after
  the handshake — before the repo-id exchange, TOFU pin, or any object
  data. (Noise XX only reveals the initiator's static key once the
  handshake completes, so that is the earliest point the key is known;
  the previous code checked it post-hoc, *after* fully serving the sync.)
  Tests: `gpp-sync` unauthorized-rejected-before-data + authorized-accepted.
- [x] **P0.3 — Add coverage measurement to CI** (`cargo llvm-cov`).
  Done 2026-06-23. New `coverage` job in `.github/workflows/ci.yml`
  (install via `taiki-e/install-action`, `--summary-only` to the log +
  `--lcov` uploaded as the `coverage-lcov` artifact). Verified locally.
  **Baseline (2026-06-23): 65.70% line / 62.85% region / 60.80% fn.**
  Worst offenders for P0.4 to target: `gpp-relay/main.rs` 0% (binary,
  no harness), `gpp-tui` 55%, `gpp-graphex/project.rs` 48%,
  `gpp-graphex/object.rs` 58% / `store.rs` 63%, `gpp-remote` 67%.
- [~] **P0.4 — Test depth pass.** Bring smoke-only crates up to the
  core-crate bar. Add a `tests/` integration dir per crate exercising
  the real store/db path (not just inline unit tests). Priority order by
  current thinness: `gpp-sdk` (1), `gpp-notify` / `gpp-rbac` /
  `gpp-replay` / `gpp-tui` (2 each), `gpp-trust` / `gpp-sync` /
  `gpp-cost` (3 each). Target: each core crate ≥ 80%, integration crates
  ≥ 60% (the `CLAUDE.md` target). Large, ongoing; gated by P0.3.
  - [x] `gpp-tui` (2026-06-24): **55% → 86.6% line** (clears the 80%
    core bar). Extracted the keymap (`Action::from_key`), wraparound
    (`step_selection`), and frame rendering (`draw`) out of the TTY
    `run` loop so they're testable headless; added `tests/dashboard.rs`
    exercising `Dashboard::collect` against real timeline/history/trust/
    anomaly/cost/review stores + `TestBackend` render assertions.
    2 → 17 tests. Remaining gap is the `run` event loop (needs a PTY).
  - [ ] Next: `gpp-sdk` (1 test, thinnest), then `gpp-notify` /
    `gpp-rbac` / `gpp-replay`.
- [x] **P0.5 — End-to-end integration tests.** Done 2026-08-23 as
  `crates/gpp-cli/tests/integration.rs` (drives the real binary across
  layers; a workspace-level `tests/` dir is not a crate): two-peer sync
  over a live `sync serve` socket both directions; git-import → promote →
  git-export with byte-level content + provenance checks;
  promote → review → RBAC merge gate (pending refused, contributor
  approval satisfies review but not RBAC, maintainer merges); MCP
  query → propose → accept → query with audit trail. **Found and fixed
  three real bugs on the first run** (see WORKLOG): sync mis-classified a
  fast-forward as a fork and built an invalid fork ref name from the
  peer's socket address (aborting the sync); `git-import` from a sibling
  checkout left the working dir empty so the next promote would export a
  deletion of every imported file; `git-export` to a repo other than the
  import source referenced commits the target did not hold. *Open:* no
  `gpp clone`/join — a second peer is bootstrapped by copying
  `.gpp/sync/repo_id` (the test does this by hand); worth a command.
- [x] **Passphrase-wrapped master key** (Phase 3 deviation). Done
  2026-05-18 (`gpp-graphex`): `$GPP_GRAPHEX_PASSPHRASE` (or
  `KeyStore::{generate,open}_with`) scrypt-wraps `master.age` at rest and
  seals `human-only` directly to the passphrase, so the master identity
  alone can no longer decrypt human-only. Legacy unwrapped stores keep
  working (auto-detected). `gpp keys show/generate` report the mode.
  Tests: crypto passphrase round-trip + wrong-pass, keys
  passphrase-store round-trip, human-only-not-master-readable.

## P1 — Deferred roadmap features

Promised in the roadmap, scaffolded, not yet wired to live I/O. Loosely
tiered (not strictly ordered — none is picked until P0 clears):
**(a) make a placeholder layer real:** real token/cost capture →
per-path cost attribution (the latter is pointless until the former,
so they're listed in that order). **(b) adoption leverage:**
bidirectional platform sync, then live dependency intelligence.
**(c) polish:** everything after.

- [x] **Real token/cost capture.** Done 2026-06-24. Added
  `CostStore::add_usage` (accumulating upsert that fills the promote-time
  `"unknown"` placeholder) + `get`; `AgentSession::report_cost` (re-exports
  `gpp_cost::Usage`); a `gpp cost --report <cs> --model --input --output
  --cached --cost-micro --duration-ms` CLI path (resolves HEAD/short id to
  the canonical record); and a `report_cost` MCP tool. The MCP server now
  returns an `instructions` block teaching the propose→report loop, and
  `propose_changeset` returns the full changeset id so the agent can report
  against it. Tests: `gpp-cost` (accumulate + placeholder-fill + create),
  `gpp-sdk` (report round-trip), `gpp-cli/tests/cost_report.rs` (3 e2e).
  Docs: `tut-mcp.md` (full agent loop + `.mcp.json`), `CLI_SPEC.md`.
- [ ] **Per-path cost attribution** (Phase 4 deviation). Cost is
  repo-wide today; attribute to changed paths. Depends on real token
  capture above (attributing zeros per path is meaningless).
- [~] **Live dependency intelligence** (`gpp-deps`). OSV (CVE) enrichment
  done 2026-06-24: `gpp deps --network` queries api.osv.dev per dependency
  (Cargo→crates.io, npm→npm), folds advisories into the score (pins a
  vulnerable dep to risk ≥ 85 + a note listing advisory ids), and caches
  responses under `.gpp/cache/deps` with a 1-day TTL (`--cache-ttl` to
  override). All best-effort — per-dep network failures are reported, never
  fatal; an all-cache run does zero network I/O (client built lazily on
  first miss). Verified live: `smallvec 1.6.0` → RUSTSEC-2021-0003. Tests:
  `gpp-deps` (OSV response parse, risk/note application, fresh-vs-stale
  cache served offline). *Still open:* crates.io/npm registry metadata
  (yank/latest/downloads) and license APIs.
- [~] **Bidirectional platform sync** (`gpp-remote`, Phase 7). Inbound
  read round-trips done 2026-06-24 (GitHub): `HttpClient` gained a
  `get_json` method (default-erroring so POST-only clients still compile;
  `ReqwestClient` overrides it); `fetch_ci_status` imports the combined
  commit status and `fetch_pr_reviews` imports PR reviews into a
  `ReviewSummary` with an `is_approved()` gate mirroring the local one.
  CLI: `gpp remote ci [--git-ref REF]` and `gpp remote reviews --pr N`
  (GitHub-only, clear error otherwise). Pure parsers + GET dispatch are
  unit-tested offline via a GET mock (5 new tests). *Still open:* writing
  imported approvals back into the local `ReviewStore`, posting
  review/issue comments outbound, issue-ref linking, and GitLab/Bitbucket
  inbound. (PR creation + enriched bodies already worked.)
- [~] **Native SDK bindings.** Rust `gpp-sdk` + `--json` CLI are the
  integration path today; add PyO3 (Python) and napi (JS) wrappers and
  the build tooling to ship them.
- [~] **Federation hardening** (Phase 5). Config + graph-only sync work;
  add publish-filter globs and one-way federated read-only enforcement.
- [x] **Graphex semantic reviewer assignment** (Phase 6 deviation). Done
  2026-06-24. `owned-by` edges are now the primary reviewer-suggestion
  source: the promote hook maps the changeset's changed paths → module
  roots (`gpp_graphex::module_roots`, exposed from the existing inference
  logic) → graph nodes → their `owned-by` owners, and feeds them to the
  new `ReviewStore::suggest_reviewers_with_owners` (graph owners lead, RBAC
  maintainers fill in behind, de-duplicated; empty owners ⇒ old RBAC-only
  behaviour). Best-effort — a graph/role read failure never blocks the
  promote. Tests: `gpp-graphex` (`module_roots`), `gpp-review`
  (merge/empty policy), `gpp-cli/tests/reviewer_assignment.rs` (e2e: a
  graph owner with no RBAC role is notified on a change to her module).
- [x] **Belief Bisect — VCS-native knowledge staleness** (`gpp-graphex`,
  `gpp-diff`, `gpp-core`, `gpp-cli`). Done 2026-07-11 (see
  [`WORKLOG.md`](WORKLOG.md)). Graphex `Belief` nodes (claim + scope +
  evidence spans anchored at a changeset) plus a deterministic staleness
  engine: scope-intersecting commits mark `StaleCandidate`;
  evidence-span content change/deletion marks `Invalidated` at the
  culprit changeset, with drift-adjusted spans and tree-sitter symbol
  refinement. CLI: `gpp belief add/log/at/stale/bisect/reaffirm` (all
  `--json`); beliefs surface in agent projections with staleness flags.
  Zero LLM/network on the core path (`SemanticInvalidator` trait stub is
  the v2 seam). Tests: engine unit suite + `gpp-cli/tests/
  belief_bisect.rs` (6 e2e, deterministic synthetic refactor). Demo:
  `demos/belief-bisect/` — axum 0.6→0.7 through the git bridge; all 4
  invalidated beliefs bisect to changelog-documented breaking commits
  (#1751/#1868), control belief survives. *Open:* asciinema capture
  (script ready; tool not installed).
- [x] **Agent-written beliefs over MCP** (`propose_belief`), projection
  freshness envelopes, and `belief.*` notify events. Done 2026-08-23
  (0.2.0) — see the WORKLOG entry. Follow-ups landed 2026-08-23 (0.2.1):
  the scan runs inside `promote` (CLI + SDK) so events need no hook, and
  `reaffirm_belief` over MCP (refused for invalidated beliefs).
- [ ] **Semantic-diff-driven Graphex inference** (Phase 3 deviation).
  Auto-inference currently keys off changed file *paths*; drive richer
  edge inference from the semantic diff.
- [ ] **TUI mutation surface** (Phase 8 deviation). Promote/approve from
  the TUI (currently read-only; CLI is the only mutation path).
- [~] **Latency target verification** (Phase 8). Measured 2026-08-06
  (i7-1370P, 32 GB): `read_4k_blob_hot` **10.6 µs** vs the < 1 ms hot-read
  target (94× headroom); `write_4k_blob` 5.8 µs; `read_raw_4k` 2.7 µs;
  `semantic_200_fns_one_change` 14.6 ms; `line_unified_200_fns` 59 µs.
  *Still open:* the < 5 ms timeline target has no dedicated bench (the
  store write it sits on is µs-level, but the capture path — watcher →
  debounce → SQLite — is unmeasured), no 100k-object clone bench exists,
  and nothing gates these in CI.
- [~] **Outreach pass** (2026-07-12). Automatable parts done — see the
  WORKLOG entry: URL/count/LICENSE fixes, demo recording, README + site
  redesign around the staleness wedge, 4-target release matrix, MCP.md,
  blog/HN/r-rust drafts. *Remaining:* the account-bound steps in
  [`outreach/MANUAL-CHECKLIST.md`](outreach/MANUAL-CHECKLIST.md)
  (About/topics, v0.1.0 tag, brew tap, posting, MCP directory PRs), and
  verifying the Windows/macos-x86_64 release legs on the first tag.
- [ ] **apt/dpkg packaging.** Tarball + Docker + Homebrew ship today;
  add Debian packaging.
- [ ] **Email/Jira/Linear notify backends.** Dispatch path is generic
  (webhook/slack/discord done); add SMTP (`lettre`) + Jira/Linear.

## P2 — Post-launch stretch

Carried from ROADMAP "Stretch Goals" — not scheduled, captured so they
aren't lost.

- [ ] Web UI for Graphex visualization (`gpp.dev`, Mode 3)
- [ ] JetBrains plugin
- [ ] Agent orchestration layer (lead agent reviews exploration branches)
- [ ] Agent-to-agent collaboration
- [ ] Built-in AI changeset summarization (local models)
- [ ] Multi-repo / monorepo workspaces
- [ ] Graphex schema validation
- [ ] Time-travel debugging integration
- [ ] REST/gRPC API on relay node
- [ ] Hosted relay service
- [ ] Migration tools (Mercurial, SVN, Perforce)
- [ ] Mobile app for review/inbox

---

## How to use this doc

- Take the lowest-numbered open item in the highest open band (the
  **▶ Next** callout names it for P0); P1 is tiered, not strict.
- When starting an item, move it to a `[~]` and note the crate(s). Keep
  the **▶ Next** pointer current when a P0 item lands.
- When an item lands, check it off here **and** update the matching
  ROADMAP deviation note so the two stay in sync.
- New deferrals discovered mid-work go in the band that matches their
  risk (a correctness gap is P0 even if it surfaces during a P1 task).
