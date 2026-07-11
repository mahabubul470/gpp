# Social drafts — belief bisect launch

Drafts for Show HN, r/rust, and X. One wedge everywhere: agent memory
goes stale silently; a VCS that hosts the memory on its own history can
witness the staleness. Everything claimed is runnable in the current
build. No benchmark or "smarter agents" claims — operational claims only.

---

## (a) Show HN

**Title:**

> Show HN: Gpp – version control that knows when your agent's memory went stale

**Text:**

> Every agent memory system (CLAUDE.md files, memory banks, git-like
> memory stores) shares a failure mode: the memory doesn't know when the
> code moved, so stale facts get served verbatim until something re-reads
> and re-checks them. Gpp hosts the knowledge graph on the repo's own
> changeset stream, so staleness is a deterministic history query — diff
> intersection plus evidence-span blob hashes, no LLM, no network — and
> `gpp belief bisect` names the exact commit that staled a fact, with the
> offending hunk. Validated on real history: five beliefs seeded at axum
> 0.6.0, bisected across 288 commits to 0.7.0; all four invalidated
> beliefs land on commits in axum's own changelog (#1751, #1868), and the
> control belief (`State<T>`) correctly survives.
>
> Repo: https://github.com/mahabubul470/gpp
> Write-up: https://github.com/mahabubul470/gpp/blob/main/docs/outreach/blog-belief-bisect.md

*(HN etiquette note for the poster: put the repo as the submission URL and
the text in a first comment if the text field feels long.)*

---

## (b) r/rust

**Title:**

> gpp: an AI-native VCS in Rust — belief staleness as a deterministic
> history query (validated against axum 0.6→0.7)

**Body:**

> I've been building gpp, a version control system in Rust aimed at
> repos where AI agents contribute continuously. The feature I'd most
> like eyes on: the repo's knowledge graph can hold *beliefs* — claims
> anchored at a changeset with evidence spans — and staleness checking is
> a pure history computation: first-parent walk to the anchor, tree
> flatten + diff intersection per commit, evidence spans drift-adjusted
> line-by-line and compared by blob hash. Scope touch marks a belief
> *stale-candidate* (re-verify); only evidence-span content change or
> file deletion marks it *invalidated*. No LLM calls anywhere in the
> engine. `gpp belief bisect` returns the first commit that staled a
> claim, with the hunk.
>
> Validation on real history: seeded five beliefs true at axum-v0.6.0,
> advanced 288 first-parent commits to v0.7.0 through the git bridge, and
> bisected. All four invalidations land on commits documented as breaking
> changes in axum's 0.7.0 changelog (#1751 ×3, #1868); the `State<T>`
> control belief survives. Evidence seeded at `routing/mod.rs:64` was
> drift-tracked to line 59 through unrelated upstream edits without a
> false invalidation. Import of 1,251 commits ≈ 8 s; a full bisect
> re-scan over the 288-commit range ≈ 0.5 s.
>
> Implementation, since this is r/rust:
>
> - 21-crate Cargo workspace, single `gpp` binary (clap derive)
> - Content-addressed store: BLAKE3 ids, zstd frames
> - Semantic diff via tree-sitter (Rust/Python/TypeScript/Go grammars)
> - Local indexes: rusqlite (bundled SQLite, WAL)
> - Knowledge graph encrypted at rest: age master key wrapping per-node
>   AES-256-GCM; MessagePack + zstd node blobs
> - P2P sync over Noise_XX (`snow`)
> - `#![forbid(unsafe_code)]` across the workspace; `thiserror` in
>   libraries, `anyhow` in the CLI; 177 workspace tests (the belief
>   engine has an e2e suite scripting a synthetic 7-commit repo in CI)
>
> It bridges to plain Git (import/export), so it sits alongside GitHub
> rather than replacing it. Honest limits: beliefs are hand-seeded (no
> automatic extraction), symbol scopes are top-level declarations, and
> semantic judgment ("does the new code contradict the claim?") is a
> deliberate trait stub for a v2. Test depth is uneven across the
> integration crates — noted in the README.
>
> Code review very welcome, especially on the staleness engine
> (`crates/gpp-graphex/src/stale.rs`) and the store
> (`crates/gpp-core`): https://github.com/mahabubul470/gpp

---

## (c) X/Twitter thread outline

**1/** Your coding agent's memory goes stale silently. That CLAUDE.md
line — "token expiry is 24h" — has been false since a commit in June,
and nothing flagged it. We made staleness a version-control query.
Deterministic, offline, zero LLM calls.

**2/** The structural problem: memory systems (memory banks, git-like
memory stores) live *beside* the repo, so drift must be *detected* —
re-read, re-embed, ask a model. A VCS hosting the memory on its own
event stream *witnesses* drift: every commit arrives with diff + author +
time attached.

**3/** So: anchor a claim at a changeset with evidence spans →
`gpp belief stale` intersects every later commit's diff with the claim's
scope, compares evidence spans by blob hash → `gpp belief bisect` names
the first commit that staled it, with the offending hunk.

**4/** Validated on real history: 5 beliefs seeded at axum 0.6.0,
bisected across 288 commits to 0.7.0. All 4 invalidated beliefs land on
commits in axum's own 0.7 changelog (#1751, #1868). The control belief —
`State<T>`, unchanged in 0.7 — survives. Evidence lines drift-tracked
(64→59) with no false positives.

**5/** Honest semantics, because that's the design: a scope touch =
*stale-candidate* (re-verify), never "false". Only evidence-span change
or deletion = *invalidated* — grounds gone, not disproven. Semantic
judgment is deliberately a v2 stub.

**6/** It's part of gpp, an AI-native VCS in Rust (bridges to plain Git;
MCP server built in — your agent sees stale beliefs flagged in its
context). Demo + validation table:
https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect
