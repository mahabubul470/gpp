# Your coding agent's memory goes stale silently. Version control can catch it.

*Draft — for publication. Everything below is runnable against the current
build of [gpp](https://github.com/mahabubul470/gpp); the axum numbers come
from a recorded run of `demos/belief-bisect/run-axum-demo.sh`.*

Every serious agent setup now carries a memory: a `CLAUDE.md`, a memory
bank, a knowledge file, or one of the newer git-like memory stores
(Memoria, DiffMem, Git Context Controller). They all share a failure
mode: the memory doesn't know when the code moved.

```markdown
## Auth notes
- token expiry is 24h            <- silently false since commit fhcpef7c
```

That line gets served verbatim into your agent's context, every session,
months after someone changed the constant. Nothing flags it. The agent
confidently builds on a fact that stopped being true in June.

This failure mode now has measurements attached. The
[STALE benchmark](https://arxiv.org/abs/2605.06527) (May 2026) tests
exactly this question — can an agent tell that a stored belief has been
invalidated by later evidence? — and the best frontier model evaluated
managed **55.2% overall accuracy**, with specialized memory frameworks
doing no better. A related result on
[temporal validity in retrieval memory](https://arxiv.org/abs/2606.26511)
reports RAG pipelines serving stale facts 15–40% of the time when forced
to commit to an answer. Both study everyday-fact memory, not code — but
code makes the problem *harder*, not easier: the thing invalidating your
memory is a commit another agent pushed an hour ago, at machine speed.
Asking a model to notice is the wrong tool. The invalidation event is
already sitting in version control.

## The parallel-store tax

The structural problem is that these systems keep memory *beside* the
repository. Versioning the memory itself — which the git-like memory
stores do well — tells you how your *notes* evolved. It cannot tell you
that the *code* under a note changed, because the code's history lives in
a different store. So drift between memory and code has to be **detected**
after the fact: re-read the files, re-embed, or ask a model "is this still
true?" That's periodic, probabilistic, and costs tokens — and between
detection passes, the memory is silently wrong.

## A VCS doesn't detect drift. It witnesses it.

Version control already has the thing a memory system is missing: a
structured stream of change events. Every commit arrives with an author, a
timestamp, a diff, and provenance attached. If your knowledge store lives
on the repository's *own* event stream, then "did anything happen to the
code this note is about?" stops being a semantic question and becomes a
history query:

- Anchor each remembered fact (a **belief**) at a changeset, with a scope
  (paths, or tree-sitter-resolved symbols) and optional **evidence
  spans** — the exact lines the claim rests on.
- For every commit after the anchor, intersect the commit's diff with the
  belief's scope, and compare evidence-span content by blob hash.

That's it. Deterministic, offline, zero LLM calls. And it's precise about
what it knows — this distinction is the whole design:

| status | meaning |
|---|---|
| `active` | no commit since the anchor intersects the belief's scope |
| `stale-candidate` | a commit touched the scope, but every evidence span is unchanged — re-verify |
| `invalidated` | an evidence span's content changed or its file was deleted — the belief's grounds are gone |
| `reaffirmed` | a human re-checked the claim and re-anchored it |

Note what the engine does **not** claim: a `stale-candidate` is not
"false" — it means history touched the neighborhood and a re-check is
warranted. Even `invalidated` means the evidence is gone, not that the
claim is disproven. Judging whether new code actually contradicts a claim
needs semantics, and this engine deliberately doesn't do semantics.

This is the wedge gpp is built around. Its knowledge graph (Graphex) sits
on the same changeset stream as the code, so staleness is a query, not a
scan:

```
gpp belief add --claim "token expiry is 24h" --evidence auth/token.rs:7-7
gpp belief stale        # every belief whose scope history has touched
gpp belief bisect <id>  # the first commit that staled it + offending hunk
gpp belief at <cs>      # the belief set as it stood at any changeset
gpp belief log <id>     # full append-only status history
```

And the flat-file example above, witnessed instead of served verbatim:

```
invalidated  ntqd225c  "token expiry is 24h"
    2026-06-03  cs:fhcpef7c  invalidated  — evidence auth/token.rs:7-7 changed

$ gpp belief bisect ntqd225c
INVALIDATED  cs:fhcpef7c  2026-06-03
  "raise token expiry to 7 days"
  cause: evidence auth/token.rs:7-7 changed
 -     7 | pub const EXPIRY_HOURS: u64 = 24;
 +     7 | pub const EXPIRY_HOURS: u64 = 168;
```

The fact comes with its killer attached: which commit, when, and the hunk.

## Validation: axum 0.6 → 0.7, 288 commits, changelog cross-check

Synthetic demos prove plumbing; real history proves the idea. The
`demos/belief-bisect/run-axum-demo.sh` script clones
[axum](https://github.com/tokio-rs/axum), imports its history through
gpp's git bridge pinned at `axum-v0.6.0` (`1b6780cf`), seeds five beliefs
that were true of that commit — with evidence spans in the real source —
then advances to `axum-v0.7.0` (`b7d14d36`, 288 first-parent commits
later), re-imports, and bisects. The only network use is the initial
clone.

Results from the run of 2026-07-11, cross-checked mechanically against
axum's own `CHANGELOG.md` for 0.7.0:

| belief (true at v0.6.0) | bisect verdict | culprit commit | in 0.7.0 changelog? |
|---|---|---|---|
| `Router` is generic over the request body type (`Router<S, B>`) | invalidated | `4e4c2917` — Remove `B` type param ([#1751]) | yes |
| axum re-exports `hyper::Server`; apps start with `axum::Server::bind` | invalidated | `c9796725` — Add `serve` function and remove `Server` re-export ([#1868]) | yes |
| `axum::body::Body` is hyper's `Body` type re-exported | invalidated | `4e4c2917` — Remove `B` type param ([#1751]) | yes |
| request bodies can be streamed with `extract::BodyStream` | invalidated | `4e4c2917` — Remove `B` type param ([#1751]) | yes |
| shared state is extracted with `State<T>` | **holds** (active) | — | — (unchanged in 0.7) |

[#1751]: https://github.com/tokio-rs/axum/pull/1751
[#1868]: https://github.com/tokio-rs/axum/pull/1868

Every invalidated belief bisects to a commit documented as a breaking
change in axum's 0.7.0 changelog, and the control belief — `State<T>`,
which 0.7 kept — correctly survives all 288 commits.

Two details in the output are worth a close look:

- **Evidence spans drift.** The `Router<S, B>` evidence was seeded at
  `routing/mod.rs:64`; by the culprit commit the engine reports it at
  line 59. Five lines of unrelated upstream edits moved the span, and the
  drift tracker followed it without a false invalidation. An edit *above*
  a span moves it; only an edit *inside* it invalidates.
- **Span precision controls verdict precision.** Pinning the evidence to
  the signature line only (`64-64`) means PR [#1806], which rewrote the
  struct's *private fields*, does not fire the invalidation. The verdict
  lands exactly on the commit that removed the `B` parameter, and the
  nine scope-level touches before it are reported as the stale candidates
  they are.

[#1806]: https://github.com/tokio-rs/axum/pull/1806

Timing on that run: importing all 1,251 commits reachable from v0.7.0 took
about 8 seconds; a full `belief bisect` re-scan over the 288-commit range
takes about 0.5 seconds. No model, no network.

## Scaling the validation: five repos, four languages

One repo is an anecdote, so the same methodology — beliefs seeded true at
an old release tag, evidence lines verified against the pinned tree,
bisect across a major version, culprit asserted against a pinned expected
commit — now runs against a matrix
([`demos/belief-bisect/repos/`](https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect)):

| repo | language | range | first-parent commits | result |
|---|---|---|---|---|
| axum | Rust | 0.6.0 → 0.7.0 | 288 | 4 invalidated on changelog commits, control survives |
| flask | Python | 1.1.0 → 2.0.0 | 221 | 3 invalidated on changelog merges (#3554, #3562, #3828), control survives |
| clap | Rust | 3.0.0 → 4.0.0 | 616 | ArgEnum rename caught (#3799); 2 beliefs honestly die at an *undocumented* internal reorg (#3438) |
| zod | TypeScript | 3.0.0 → 4.0.1 | 1237 | nil-UUID fix caught mid-series (#483); 2 beliefs die exactly at the "Zod 4" merge |
| go-redis | Go | 8.0.0 → 9.0.0 | 388 | 3 invalidated on v9-migration changes (#2171, #2244, v9 merge), control survives |

All 21 expectations pass (16 matrix + 5 axum). The matrix surfaces two
behaviors the axum run couldn't:

- **Mid-series fixes are caught, not just major breaks** — zod's uuid
  regex was quietly widened to admit the nil UUID two years before v4;
  the bisect hands you that PR, not the version bump.
- **Undocumented reorgs surface honestly** — clap moved `src/build/*` in
  an internal flatten long before 4.0 removed the APIs. A file-anchored
  belief dies at the move, because that *is* the moment its evidence
  vanished — "grounds gone, not disproven," working as specified, and a
  live demonstration of why evidence-span (and symbol) precision matters.

## Where this sits among the neighbors

The space around this got busy in 2026, so it's worth being precise about
what each neighbor does and doesn't do:

- **Session-capture tools** ([Entire](https://entire.io),
  [re_gent](https://news.ycombinator.com/item?id=48063548)) record *how
  code was written* — agent prompts, checkpoints, provenance, down to
  `entire blame` tracing a line back to its session. That's the
  provenance half of the problem (gpp's timeline layer does this too).
  None of them track whether *what you believe about the code* is still
  true.
- **Codebase knowledge graphs** ([Cognee](https://www.cognee.ai),
  Potpie, GraphRAG-style code memory) rebuild a graph from the code,
  incrementally, keyed on file timestamps. On re-ingestion they can
  *prune* a stale node — but they can't name the commit that staled it,
  say when it happened, or show you what was believed before. The
  history isn't part of the store.
- **Git-like memory stores** (Memoria, DiffMem, Git Context Controller)
  version the *memory* — you can diff your notes. But the notes' history
  and the code's history are separate stores, so code-drift still has to
  be detected semantically, after the fact.
- **Agent-native VCSes** ([Atomic](https://github.com/atomicdotdev/atomic),
  [Oak](https://news.ycombinator.com/item?id=48631726)) rethink the
  version control substrate — patch theory, virtual mounts, token
  efficiency. Closest in spirit to gpp's platform, but neither hosts
  claims about the code, so neither can rule on their staleness.

The missing combination is the one this post is about: knowledge
anchored *on* the code's own history, so invalidation is witnessed by
the same event stream that caused it — deterministically, with the
culprit commit attached.

## What this doesn't do (yet)

Honesty about scope is the point of the design, so, plainly:

- **StaleCandidate ≠ false.** Scope intersection is a re-verify signal,
  not a verdict. Only evidence-span content change or file deletion
  yields `invalidated` — and even that means "grounds gone", not
  "disproven".
- **Beliefs are hand-seeded.** You (or your agent, via the graph-update
  proposal flow) write the claims. There is no automatic belief
  extraction from code or conversation.
- **Symbol coverage is top-level declarations** (via tree-sitter, for
  Rust/Python/TypeScript/Go). Nested items resolve to their enclosing
  declaration.
- **No semantic judgment.** Deciding whether the new code actually
  contradicts the claim is a `SemanticInvalidator` trait stub, reserved
  for a v2 where an LLM can be brought in *on top of* the deterministic
  layer — triaging the candidates history has already found, instead of
  re-reading the repo.

Underneath this sits a full platform — continuous timeline capture, agent
trust scoring, compliance policies, CRDT sync, cost attribution — but
those are other posts. The wedge is this one: **your agent's memory
should be a view over history, not a file beside it.**

## Try it

```bash
cargo install gpp-cli

gpp init --graphex .
gpp belief add --claim "token expiry is 24h" --evidence auth/token.rs:7-7
# ...history happens...
gpp belief stale
gpp belief bisect "token expiry is 24h"
```

Connect an agent over MCP — Claude Code picks this up from a `.mcp.json`
at the repo root:

```json
{
  "mcpServers": {
    "gpp": { "command": "gpp", "args": ["mcp-server", "--stdio"] }
  }
}
```

The agent gets `graphex_query` (project context where stale beliefs are
flagged in-line), `propose_changeset`, and `report_cost` — see
[`docs/MCP.md`](../MCP.md).

The axum demo, the synthetic 7-commit CI test, and an asciinema recording
live in
[`demos/belief-bisect/`](https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect).
Run the real-history validation yourself:

```bash
./demos/belief-bisect/run-axum-demo.sh
```
