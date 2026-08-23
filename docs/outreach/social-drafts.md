# Social drafts — belief bisect launch

Drafts for Show HN, r/rust, and X. One wedge everywhere: agent memory
goes stale silently; a VCS that hosts the memory on its own history can
witness the staleness. Everything claimed is runnable in the current
build. No benchmark or "smarter agents" claims *about gpp* — operational
claims only. The STALE figure (55.2%, arXiv 2605.06527) is cited as
third-party evidence of the problem; it measures frontier models on
everyday-fact memory, and gpp claims no score on it.

---

## (a) Show HN

**Title:**

> Show HN: Gpp – version control that knows when your agent's memory went stale

**Text:**

> Every agent memory system (CLAUDE.md files, memory banks, git-like
> memory stores) shares a failure mode: the memory doesn't know when the
> code moved, so stale facts get served verbatim until something re-reads
> and re-checks them. This gap was recently measured — on the STALE
> benchmark (arXiv 2605.06527), the best frontier model detects that a
> stored belief has been invalidated only 55.2% of the time. Gpp makes
> the code half of that problem a history query instead of a model
> judgment: it hosts the knowledge graph on the repo's own changeset
> stream, so staleness is deterministic — diff intersection plus
> evidence-span blob hashes, no LLM, no network — and `gpp belief bisect`
> names the exact commit that staled a fact, with the offending hunk.
> Validated on real history across five repos in four languages — axum
> 0.6→0.7, flask 1.1→2.0, clap 3→4, zod 3→4, go-redis 8→9 (221–1237
> commits each): 21 beliefs seeded true at the old tags, and every
> invalidation bisects to a pinned, documented culprit (axum #1751/#1868,
> flask #3554/#3562/#3828, zod's nil-UUID fix #483 and the "Zod 4" merge,
> go-redis #2171/#2244), while the control beliefs survive. It's honest
> about limits too: two clap beliefs die at an *undocumented* internal
> file reorg — the true moment their file-anchored evidence vanished.
>
> As of 0.2.0 the agent writes the notes itself: over MCP,
> `propose_belief` records a claim plus the exact lines it rests on,
> verified against the current changeset. A human approves it; from
> then on history polices it — every later `graphex_query` shows the
> belief with a freshness envelope (anchor changeset, commits since, and
> the commit that staled it), and the approver gets a
> `belief.invalidated` event the first time a commit breaks it. Agent
> memory that's human-gated going in and history-gated afterwards.
>
> How it differs from the neighbors: session-capture tools (Entire,
> re_gent) record how code was written but not whether what you believe
> about it is still true; codebase knowledge graphs (Cognee, Potpie)
> prune stale nodes on re-ingestion but can't name the commit that
> staled one; git-like memory stores version the notes, not the code
> the notes are about; the newer "bi-temporal" memory products
> (Sentra, Mneme) invalidate facts at ingestion time — when their
> indexer noticed — rather than at the changeset that caused it. gpp
> anchors the claims on the code's own history, so the invalidation
> event and the belief live in the same store.
>
> Repo: https://github.com/mahabubul470/gpp
> Write-up: https://mahabubul470.github.io/gpp/blog/belief-bisect/

*(HN etiquette note for the poster: put the repo as the submission URL and
the text in a first comment if the text field feels long.)*

---

## (b) r/rust

**Title:**

> gpp: an AI-native VCS in Rust — belief staleness as a deterministic
> history query (validated against axum 0.6→0.7)

**Body:**

> I've been building gpp, a version control system in Rust aimed at
> repos where AI agents contribute continuously. Context for why: the
> STALE benchmark (arXiv 2605.06527, May 2026) showed the best frontier
> model detects that a stored memory has been invalidated only 55.2% of
> the time — and for *code*, the invalidating event is already sitting
> in version control, so asking a model is the wrong tool. The feature
> I'd most like eyes on: the repo's knowledge graph can hold *beliefs* — claims
> anchored at a changeset with evidence spans — and staleness checking is
> a pure history computation: first-parent walk to the anchor, tree
> flatten + diff intersection per commit, evidence spans drift-adjusted
> line-by-line and compared by blob hash. Scope touch marks a belief
> *stale-candidate* (re-verify); only evidence-span content change or
> file deletion marks it *invalidated*. No LLM calls anywhere in the
> engine. `gpp belief bisect` returns the first commit that staled a
> claim, with the hunk.
>
> Validation on real history — five repos, four grammars, each imported
> through the git bridge and bisected across a major version: axum
> 0.6→0.7 (288 first-parent commits), flask 1.1→2.0 (221), clap 3→4
> (616), zod 3→4 (1237), go-redis 8→9 (388). 21 beliefs seeded true at
> the old tags; every invalidation lands on a pinned, documented culprit
> (axum #1751/#1868; flask #3554/#3562/#3828; zod #483 + the "Zod 4"
> merge; go-redis #2171/#2244 + the v9 merge) and the controls survive.
> Evidence seeded at axum `routing/mod.rs:64` was drift-tracked to line
> 59 through unrelated upstream edits without a false invalidation. One
> honest edge the matrix surfaced: clap's undocumented internal reorg
> (#3438) kills file-anchored beliefs before the 4.0 API removals do —
> "grounds gone" semantics working as specified. Import of 1,251 commits
> ≈ 8 s; a full bisect re-scan over 288 commits ≈ 0.5 s.
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
>   libraries, `anyhow` in the CLI; 179 workspace tests (the belief
>   engine has an e2e suite scripting a synthetic 7-commit repo in CI)
>
> New in 0.2.0: agents write beliefs themselves via an MCP
> `propose_belief` tool (evidence spans verified against HEAD with the
> same code path the CLI uses — the helpers live in `gpp-graphex`), which
> land `Proposed` for human approval and are scanned from the moment
> they exist; projections carry a freshness envelope per belief
> (anchor, first-parent commits since, culprit); and `belief stale`
> emits `belief.invalidated` events on transitions only. Found a real
> bug doing it: the belief save path hard-coded the Active state, so a
> routine scan would have silently approved an agent's proposal — now
> pinned by a test.
>
> It bridges to plain Git (import/export), so it sits alongside GitHub
> rather than replacing it. Honest limits: beliefs are hand-seeded or
> agent-proposed (no automatic extraction from code or conversation),
> symbol scopes are top-level declarations, and
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
and nothing flagged it. On the STALE benchmark, the best frontier model
catches an invalidated memory 55.2% of the time. We made staleness a
version-control query instead. Deterministic, offline, zero LLM calls.

**2/** The structural problem: memory systems (memory banks, git-like
memory stores) live *beside* the repo, so drift must be *detected* —
re-read, re-embed, ask a model. A VCS hosting the memory on its own
event stream *witnesses* drift: every commit arrives with diff + author +
time attached.

**3/** So: anchor a claim at a changeset with evidence spans →
`gpp belief stale` intersects every later commit's diff with the claim's
scope, compares evidence spans by blob hash → `gpp belief bisect` names
the first commit that staled it, with the offending hunk.

**4/** Validated on real history: 21 beliefs across five repos in four
languages — axum 0.6→0.7, flask 1.1→2.0, clap 3→4, zod 3→4, go-redis
8→9. Every invalidation bisects to a pinned, documented commit (down to
zod's quiet nil-UUID regex widening, #483); every control survives.
Evidence lines drift-tracked (64→59) with no false positives.

**5/** Honest semantics, because that's the design: a scope touch =
*stale-candidate* (re-verify), never "false". Only evidence-span change
or deletion = *invalidated* — grounds gone, not disproven. Semantic
judgment is deliberately a v2 stub.

**5b/** 0.2.0: the agent writes the belief itself over MCP
(`propose_belief` — claim + the exact lines it rests on). A human
approves it; history polices it. Every later context query shows the
anchor, commits since, and the commit that broke it. You get pinged the
first time that happens.

**6/** It's part of gpp, an AI-native VCS in Rust (bridges to plain Git;
MCP server built in — your agent sees stale beliefs flagged in its
context). Demo + validation table:
https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect
