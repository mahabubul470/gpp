# TODO — gpp (git++) Next Work

Status snapshot: all 9 phases (0–8) implemented; 123 workspace tests pass;
clippy/fmt clean (verified 2026-05-18). This document is the single
prioritized backlog for what comes next. It consolidates the `[~]` partial
items and "Implementation notes / deviations" deferrals scattered through
[`ROADMAP.md`](ROADMAP.md), plus newly identified hardening work, into one
actionable list.

Legend: **P0** = correctness/hardening before any real adoption ·
**P1** = deferred features promised in the roadmap ·
**P2** = post-launch stretch · `[ ]` open · `[~]` partially done.

---

## P0 — Hardening before real-world use

These are not new features; they are gaps between "milestone met" and
"safe to depend on".

- [ ] **Test depth pass.** Bring smoke-only crates up to the core-crate
  bar. Add a `tests/` integration dir per crate exercising the real
  store/db path (not just inline unit tests). Priority order by current
  thinness: `gpp-sdk` (1), `gpp-notify` / `gpp-rbac` / `gpp-replay` /
  `gpp-tui` (2 each), `gpp-trust` / `gpp-sync` / `gpp-cost` (3 each).
  Target: each core crate ≥ 80%, integration crates ≥ 60% (the
  `CLAUDE.md` coverage target — currently unmeasured).
- [ ] **Add coverage measurement to CI** (`cargo llvm-cov`) so the
  target above is enforced, not aspirational.
- [ ] **End-to-end integration tests** under `tests/integration/`:
  two-peer sync round-trip, git-import→promote→git-export fidelity,
  promote→review→merge gate, MCP query→propose→accept. The crate-level
  suites are all isolated; nothing currently tests the layers together.
- [x] **Passphrase-wrapped master key** (Phase 3 deviation). Done
  2026-05-18 (`gpp-graphex`): `$GPP_GRAPHEX_PASSPHRASE` (or
  `KeyStore::{generate,open}_with`) scrypt-wraps `master.age` at rest and
  seals `human-only` directly to the passphrase, so the master identity
  alone can no longer decrypt human-only. Legacy unwrapped stores keep
  working (auto-detected). `gpp keys show/generate` report the mode.
  Tests: crypto passphrase round-trip + wrong-pass, keys
  passphrase-store round-trip, human-only-not-master-readable.
- [ ] **Relay pre-handshake key rejection** (Phase 7 deviation).
  `--auth-keys` is advisory only; add the `gpp-sync` hook to reject
  unknown static keys before the Noise handshake completes.
- [ ] **Policy enforcement points beyond promote** (Phase 4 deviation).
  Wire the existing `PolicySet` API into timeline-capture (warn) and
  sync (block) — the API is ready, the attach points are not.

## P1 — Deferred roadmap features

Promised in the roadmap, scaffolded, not yet wired to live I/O.

- [~] **Live dependency intelligence** (`gpp-deps`). Offline lockfile +
  heuristic risk works; add live crates.io / npm / OSV (CVE) / license
  APIs behind an opt-in network flag with response caching.
- [~] **Bidirectional platform sync** (`gpp-remote`, Phase 7). PR
  creation + enriched bodies work; payload builders for review/comment
  sync, CI-status import, and issue linking exist but aren't wired to
  authenticated polling. Implement the live round-trips.
- [~] **Native SDK bindings.** Rust `gpp-sdk` + `--json` CLI are the
  integration path today; add PyO3 (Python) and napi (JS) wrappers and
  the build tooling to ship them.
- [~] **Federation hardening** (Phase 5). Config + graph-only sync work;
  add publish-filter globs and one-way federated read-only enforcement.
- [ ] **Per-path cost attribution** (Phase 4 deviation). Cost is
  repo-wide today; attribute to changed paths now that the review layer
  exists.
- [ ] **Real token/cost capture.** Cost records are created at
  promote-time with tokens/cost = 0 until a Tier-3 SDK reports usage.
  Wire actual agent usage reporting through the SDK.
- [ ] **Graphex semantic reviewer assignment** (Phase 6 deviation). The
  `owned-by` edge exists; make it the primary reviewer-suggestion source
  instead of RBAC owners/maintainers only.
- [ ] **Semantic-diff-driven Graphex inference** (Phase 3 deviation).
  Auto-inference currently keys off changed file *paths*; drive richer
  edge inference from the semantic diff.
- [ ] **TUI mutation surface** (Phase 8 deviation). Promote/approve from
  the TUI (currently read-only; CLI is the only mutation path).
- [ ] **Latency target verification** (Phase 8). Criterion baselines
  exist; measure and tune against the stated targets (timeline < 5ms,
  hot read < 1ms, 100k-object clone < 30s) and gate them in CI.
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

- Pick the next item from the **highest** open priority band.
- When starting an item, move it to a `[~]` and note the crate(s).
- When an item lands, check it off here **and** update the matching
  ROADMAP deviation note so the two stay in sync.
- New deferrals discovered mid-work go in the band that matches their
  risk (a correctness gap is P0 even if it surfaces during a P1 task).
