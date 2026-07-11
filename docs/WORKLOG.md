# WORKLOG — gpp (git++)

Running engineering log. Newest entry first. Each entry: date, scope,
what was learned/decided/done, and any deviation from plans or docs.
(Companion to [`TODO.md`](TODO.md), which tracks the backlog; this file
tracks *why* things went the way they did.)

---

## 2026-07-12 — Outreach pass: one wedge, fixed basics, professional site

Executed the outreach handoff end to end (§1–§7); everything requiring
Mahabubul's account is collected in
[`outreach/MANUAL-CHECKLIST.md`](outreach/MANUAL-CHECKLIST.md).

**Basics (§1):** every `gpp-vcs` reference replaced with `mahabubul470`
(15 files — gpp-vcs was a placeholder future org; if it's ever created,
transfer the repo and the URLs keep working via redirect). Added the
missing MIT `LICENSE` the site/Cargo.toml already claimed. Test counts
were drifting (site said 123, README 133, reality 177) — synced to 177
with the verified date; counts now live in exactly two places (README
status + site status section).

**Demo (§4):** `scripts/demo.sh` — deterministic, temp-dir, <30 s arc:
capture → promote → tree-sitter rename collapse → belief goes stale →
bisect names the commit. Recorded headless with asciinema 3.2 →
`site/assets/demo.{cast,gif}` (agg, dracula, 100×32). Because belief
bisect already shipped (previous handoff), the "placeholder until the
feature lands" instruction was obsolete — bisect is the finale now.

**README (§2):** restructured — problem hook + GIF, 30-second try,
three differentiators, MCP quickstart with the real `.mcp.json`, badges;
honesty block preserved but corrected (integration suites now exist;
passphrase-wrapped key shipped). Layer table demoted below status.

**Site (§3):** full redesign, single-file, no JS beyond copy-to-clip.
Restrained dark palette, Inter + JetBrains Mono, real demo GIF in the
hero, memory-file-vs-witnessed contrast, the actual axum validation
table as the proof section, comparison reframed from 9 adversarial ✕'s
to 4 "different assumptions" rows with the git bridge surfaced, 17
crates collapsed under "the platform underneath", OG/Twitter meta +
generated `assets/og.png` (SVG source alongside).

**Releases (§5):** release.yml already existed with 2 targets; extended
to 4 (added macos-x86_64 cross-compile + windows-msvc with zip
packaging), generated release notes, fail-fast off. Homebrew formula
already pointed at the right tap path; sha256 fill-in is a per-release
manual step (checklist). Windows leg is untested until the first tag.

**Drafts (§6–7):** `docs/MCP.md` (tool list verified against `mcp.rs` —
7 tools; noted GRAPHEX_PROTOCOL.md's larger table is spec-not-build),
`docs/outreach/`: blog post (real axum numbers throughout), Show HN,
r/rust, thread outline, MCP-directory listing text. Zero hype words;
operational claims only.

**Known tensions recorded:** rusqlite bundles SQLite's C source — in
mild conflict with CLAUDE.md's "no C dependencies" line; decided
framing: "no hand-written C; bundled, vetted SQLite is the exception".
asciinema+agg were cargo-installed locally for the recordings.

## 2026-07-11 — Belief Bisect: implementation + demos landed

Landed the full feature the same day as discovery (entry below). What
shipped, where, and what deviated:

**Library** (`gpp-graphex`): `belief.rs` (BeliefData/Scope/Evidence/
BeliefStatus/Cause/StatusChange + store veneer; `SemanticInvalidator`
trait stub for LLM v2 — no impl), `stale.rs` (the engine: first-parent
chain to anchor, per-commit tree-flatten diff, globset scope match,
drift-adjusted evidence spans, tree-sitter symbol re-resolution per
commit, `ancestors()` for time-travel). `GraphNode` gained
`belief: Option<BeliefData>` with `#[serde(default)]` (old blobs decode
unchanged) and `NodeType::Belief`; projections render a "### Beliefs"
section with ⚠/✗ staleness flags so agents are warned in-context.
Supporting: `gpp_core::flatten_tree` promoted from two private CLI
copies; `gpp_diff::{line_ops, excerpt}` expose structured diff ops.

**CLI** (`gpp-cli/src/belief.rs`): `gpp belief add/log/at/stale/bisect/
reaffirm`, all honoring global `--json`. Docs in `CLI_SPEC.md`.

**Tests**: engine unit tests (drift→invalidation, symbol refinement,
deletion, idempotent append-only record, `status_at` time-travel) +
`gpp-cli/tests/belief_bisect.rs` (6 e2e tests scripting the §5 synthetic
JWT→sessions refactor; runs in the normal CI test job). Workspace: 177
tests green, clippy/fmt clean.

**Tier-2 axum demo** (`demos/belief-bisect/run-axum-demo.sh` + README):
pinned `axum-v0.6.0` (1b6780cf) → `axum-v0.7.0` (b7d14d36), imported via
the git bridge (both-stage: import at 0.6, seed 5 beliefs at that HEAD,
advance branch, incremental re-import — no new capture built). Results:
4/4 invalidated beliefs bisect to commits in axum's 0.7.0 changelog
(#1751 ×3, #1868), the `State<T>` control belief survives all 288
first-parent commits. Import of 1,251 commits ≈ 8 s; full bisect rescan
≈ 0.5 s; synthetic tier in ms. Notable: evidence seeded at
`routing/mod.rs:64` was correctly drift-tracked to line 59 by the
culprit; pinning the Router evidence to the signature line only kept
field-refactor PR #1806 from firing a premature invalidation.

**Deviations / notes**:
- Bisect returns the first *Invalidated* commit when one exists, else
  the first StaleCandidate (matches the handoff's expected demo answers;
  earlier stale signals are counted in the output).
- `StatusChange.at` is the *changeset's* timestamp, never scan
  wall-time, so scans are idempotent and `belief at` deterministic.
- `belief at` folds recorded history (per §4 "reconstruct from
  append-only history") — run `belief stale` first to materialize.
- Belief identity = claim text (node id = blake3("belief:"+claim)), so
  re-adding the same claim updates rather than duplicates.
- asciinema installed 2026-07-12 (`cargo install asciinema`, v3.2.0);
  `demos/belief-bisect/belief-bisect.cast` recorded headless (18 s,
  100×30) from the paced `record-demo.sh` walkthrough.

## 2026-07-11 — Belief Bisect: discovery pass (§1 of handoff)

Feature: VCS-native knowledge staleness — Graphex `Belief` nodes whose
staleness/invalidation is *witnessed* by the repo's own history rather
than detected after the fact. Core question it must answer with zero
LLM/network calls: "what did we believe about X, when did it go stale,
and which commit did it?"

### What the codebase actually provides

**Graphex node schema** (`crates/gpp-graphex/src/object.rs`)
- `NodeType` is a flat enum (`object.rs:15`); adding a variant means new
  arms in `as_str()` and `parse()`. Node id = `blake3("{type}:{name}")`
  (`object.rs:154`) — stable across content edits, so a belief keeps its
  id and edges as its status history grows.
- `GraphNode` content is MessagePack (`to_vec_named`) → zstd → per-tier
  AES-256-GCM envelope → content-addressed blob in `.gpp/objects/`;
  SQLite `graph.db` holds a last-writer-wins metadata row pointing at
  the current blob (`store.rs:132` put path, `store.rs:173` get path).
  Editing a node writes a *new immutable blob* and re-points the row —
  prior versions persist in the object store. Named-field MessagePack
  tolerates added struct fields via `#[serde(default)]`.
- Existing status patterns: `NodeState` lifecycle column
  (Proposed/Active/Deprecated/Archived), append-only `graph_access_log`
  table, `confidence` + `validated_at` fields on every node.

**Commit capture / event stream**
- There is no "flight recorder" crate; the continuous-capture engine is
  `gpp-timeline` (`Timeline::capture`, `lib.rs:76`; per-file
  `FileChange{path, blob_hash, change, old_hash}` records in SQLite,
  indexed by path). Commit-granularity events are `gpp_history::Changeset`
  objects (`object.rs:82`) promoted from timeline ranges.
- **There is no `CommitId` type.** Changesets are identified by
  `gpp_core::Hash` (BLAKE3, base32 display). History walking is
  first-parent via `gpp_history::walk` (`log.rs:28`).
- No pre-packaged "commits touching pathspec"; the house pattern is
  flatten-both-trees-and-diff — `changeset_delta`
  (`gpp-cli/src/phase4.rs:72`) and `flatten_tree`
  (`gpp-cli/src/phase1.rs:154`).
- git2 (libgit2) is used **only** inside `gpp-git-bridge`; every other
  crate is git-free. `import` converts git commits → changesets
  topologically and stamps `metadata["git_commit"]` with the original
  SHA (`import.rs:96`), with an idempotent oid↔hash map DB.

**Tree-sitter / symbols**
- The `parsers/` directory in CLAUDE.md does not exist; grammars are
  crate deps of `gpp-diff` (rust/python/typescript/go, `Cargo.toml:13`).
- Symbol extraction already exists: `gpp_diff::parse_declarations`
  returns `Declaration{kind, name, start_line, end_line, full_fp,
  body_fp}` per file (`parser.rs:73`), with `parser_for_path` for
  language detection (extension-based). Top-level declarations only —
  good enough for span refinement.

**CLI conventions**
- clap derive; a command group = `XxxArgs{#[command(subcommand)] action}`
  + `XxxAction` enum in `cli.rs`, dispatched from the single `match` in
  `main.rs` to a handler module. Best templates: `gpp remote`
  (`cli.rs:146`) and `gpp graphex` (`cli.rs:577`, store-backed +
  json-aware handler `phase3.rs`).
- Global `--json` flag on root `Cli` (`cli.rs:36`), threaded by hand to
  handlers; convention is branch-early, `serde_json::json!`,
  `to_string_pretty`. Errors: `anyhow` end to end, `bail!` for user
  errors. Plain `println!` output, no color crate.

### Deviations from the handoff (codebase wins)

1. **"anchor_commit: CommitId" → anchor is a changeset `Hash`.** The
   staleness engine walks native gpp history, not git. Git repos enter
   through the existing `gpp git import` bridge; nothing new is built
   for capture (per handoff §1.2, reusing the commit-capture path).
2. **Axum tier-2 demo runs through the bridge**: clone axum → `git
   import` → beliefs anchored at the imported changeset corresponding
   to the pinned 0.6 commit → engine walks imported changesets → bisect
   output reports the git SHA from `metadata["git_commit"]` so results
   cross-check directly against axum's changelog.
3. **Belief is not a parallel object type**: it's `NodeType::Belief` +
   a structured `belief: Option<BeliefData>` payload field on
   `GraphNode` (`#[serde(default)]`, so existing blobs decode
   unchanged). Status + append-only history live *inside* the encrypted
   node content; time-travel (`belief at`) reconstructs from that
   history, which is safe because node blobs are immutable and each
   update appends. Belief listing decrypts belief-type nodes and
   filters in Rust — fine at hand-seeded scale, avoids index schema
   churn and keeps CRDT sync semantics untouched.
4. **No `parsers/` dir** — symbol refinement uses `gpp-diff`'s existing
   `parse_declarations`; no new grammar plumbing.
5. **WORKLOG.md did not exist** (handoff assumed it); created as
   `docs/WORKLOG.md` alongside `docs/TODO.md` (TODO is not at repo root
   either).
6. Handoff suggested "libgit2/gitoxide primitives already in gpp" for
   commit enumeration — the house pattern is git-free native walks
   (`gpp_history::walk` + tree flatten); git2 stays quarantined in the
   bridge.

### Plan of record (per handoff §10)

1. ~~Discovery~~ (this entry).
2. `BeliefData` + `NodeType::Belief` in gpp-graphex; `gpp belief
   add`/`log` CLI (`--json` on everything).
3. Staleness engine (path-level) in gpp-graphex (new dep: gpp-diff,
   globset) over `gpp_history::walk`; `gpp belief stale`/`bisect`;
   synthetic-repo integration test in gpp-cli.
4. Evidence span drift-tracking + blob-hash classification; tree-sitter
   symbol refinement.
5. `gpp belief at <changeset>` time-travel.
6. Axum 0.6→0.7 tier-2 demo via git-bridge import + changelog
   validation table.
7. Contrast artifact, asciinema, docs.

Naming discipline (per handoff §0): scope-only intersection ⇒
`StaleCandidate`; only evidence-span content change ⇒ `Invalidated`.
No accuracy claims anywhere; operational/audit value only.
