# Belief Bisect — VCS-native knowledge staleness

**Thesis (witness vs. detect).** Systems that keep AI memory *beside* the
repository (a memory bank, a knowledge file, a parallel git-like store) can
only *detect* drift between what they remember and what the code now says —
by re-reading, re-embedding, or asking a model. gpp's knowledge graph lives
on the repository's **own** event stream, so drift is *witnessed*: every
change that touches a belief's scope arrives as a changeset with author,
time, diff and provenance already attached. That turns "is this note still
true?" into a deterministic history query — diff intersection plus blob
hashes, zero LLM calls, zero network — answerable down to the exact commit:

> What did we believe about module X, when did that belief become stale,
> and which commit did it?

```
gpp belief add --claim "token expiry is 24h" --evidence auth/token.rs:7-7
gpp belief stale        # every belief whose scope history has touched
gpp belief bisect <id>  # the first commit that staled it + offending hunk
gpp belief at <cs>      # the belief set as it stood at any changeset
gpp belief log <id>     # full append-only status history
```

## Honest semantics

The engine never claims a belief is *false* — that would need semantics.
It reports exactly what history proves:

| status | meaning |
|---|---|
| `active` | no commit since the anchor intersects the belief's scope |
| `stale-candidate` | a commit touched the scope (or the evidence *file*), but every evidence *span* is unchanged — re-verify |
| `invalidated` | an evidence span's content changed or its file was deleted — the belief's grounds are gone |
| `reaffirmed` | a human re-checked the claim and re-anchored it at a new changeset |

Evidence spans are drift-adjusted commit by commit (an edit *above* a span
moves it; only an edit *inside* it invalidates), and symbol scopes are
re-resolved per commit via tree-sitter. Semantic invalidation (judging
whether the new code actually contradicts the claim) is deliberately out of
scope — a `SemanticInvalidator` trait stub exists for a v2.

## Tier 1 — synthetic repo (CI)

`crates/gpp-cli/tests/belief_bisect.rs` scripts a deterministic 7-commit
repo (JWT auth → expiry change → file split → session migration) and
asserts in CI:

- `belief bisect` on "token expiry is 24h" → the expiry-change commit
  (invalidated, offending hunk shows `24` → `168`);
- `belief bisect` on "auth issues JWTs" → the session-migration commit
  (evidence file deleted), *not* the earlier commits that merely touched
  the file;
- `belief at <C0>` reproduces the original all-active belief set;
- `belief stale` lists both, idempotently across scans.

Run it: `cargo test -p gpp-cli --test belief_bisect`

## Tier 2 — real history: axum 0.6 → 0.7

`./run-axum-demo.sh` clones axum, imports history through the gpp git
bridge pinned at tag `axum-v0.6.0` (`1b6780cf`), seeds five beliefs that
were true of that commit with evidence spans in the real source, advances
to `axum-v0.7.0` (`b7d14d36`, 288 first-parent commits later), re-imports,
and bisects. The only network use is the initial clone; the engine itself
runs fully offline.

### Validation against axum's changelog (run of 2026-07-11)

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
change in axum's own `CHANGELOG.md` for 0.7.0 (the script cross-checks PR
numbers mechanically), and the control belief — `State<T>`, which 0.7 kept
— correctly survives all 288 commits.

Two details worth noticing in the output:

- **Drift**: the `Router<S, B>` evidence was seeded at
  `routing/mod.rs:64`; by the culprit commit the engine reports it at
  line 59 — five lines of upstream edits were tracked without a false
  invalidation.
- **Span precision controls verdict precision**: pinning the evidence to
  the signature line only (`64-64`) means PR [#1806], which rewrote the
  struct's *private fields*, does not fire the invalidation — the verdict
  lands exactly on the commit that removed the `B` parameter. Nine
  scope-level touches before it are reported as the stale candidates they
  are.

[#1806]: https://github.com/tokio-rs/axum/pull/1806

Timing on this run: importing all 1,251 commits reachable from v0.7.0 took
~8 s; a full `belief bisect` re-scan over the 288-commit range takes ~0.5 s;
the synthetic tier scans in milliseconds.

## Tier 2 at scale — the validation matrix (run of 2026-08-06)

`./run-repo-demo.sh repos/<name>.conf` replays the same methodology
against four more real repos — one per supported tree-sitter grammar —
each pinned at a major-version boundary. Every config seeds beliefs true
at the old tag (evidence lines verified against the pinned tree), advances
across the major version, bisects, and **asserts** the culprit is the
expected pinned commit. All 16 expectations across the matrix pass:

| repo (lang) | range (first-parent) | belief | verdict → culprit | documented where |
|---|---|---|---|---|
| flask (Python) | 1.1.0 → 2.0.0 (221) | supports Python 2 via `_compat` | invalidated → `cd8a3745` (#3554) | 2.0.0 changelog |
| | | `flask.json` prefers simplejson | invalidated → `e69b49bd` (#3562) | 2.0.0 changelog |
| | | `send_file` builds its response itself | invalidated → `bbb273bb` (#3828) | 2.0.0 changelog |
| | | Blueprint defers via `record(lambda …)` | **survives** (span drifts 294→369) | control |
| clap (Rust) | v3.0.0 → v4.0.0 (616) | derive enums implement `ArgEnum` | invalidated → `912a6290` (#3799) | rename commit; removal #4127 in 4.0.0 changelog |
| | | builder entry point is `struct App` | invalidated → `c422ed24` (#3438) | *undocumented internal reorg* — see below |
| | | `Arg::takes_value` opts into values | invalidated → `c422ed24` (#3438) | same reorg |
| | | dual-licensed Apache-2.0 OR MIT | **survives** (active) | control |
| zod (TypeScript) | v3.0.0 → v4.0.1 (1237) | `uuid()` rejects the nil UUID | invalidated → `b70e143d` (#483) | PR title: allow nil UUID |
| | | `ZodError.errors` aliases `.issues` | invalidated → `85928549` (Zod 4, #4074) | v4 migration guide |
| | | core is single-file `src/types.ts` | invalidated → `85928549` (Zod 4, #4074) | the Zod 4 merge itself |
| | | editor conventions in `.editorconfig` | **survives** (active) | control |
| go-redis (Go) | v8.0.0 → v9.0.0 (388) | conn lifetime option is `MaxConnAge` | invalidated → `3d1e2e5b` (#2171) | v9 migration: → `ConnMaxLifetime` |
| | | hooks are `BeforeProcess`/`AfterProcess` | invalidated → `180f107a` (#2244) | v9 migration: new Hook design |
| | | `Pipeline` has a `Close` method | invalidated → `0aa94538` (v9 merge) | v9: removed |
| | | missing key returns `redis.Nil` | **survives** (stale-candidate) | control |

Three verdict flavors the matrix demonstrates, beyond axum's:

- **Surgical in-series fixes** are caught, not just major-version breaks:
  zod's nil-UUID widening (#483) and flask's simplejson removal (#3562)
  land mid-series with the exact PR attached.
- **Mass invalidation at a rewrite is the correct answer**: both zod
  beliefs anchored in `src/` die at the "Zod 4" merge — the commit where
  every v3 file-anchored fact genuinely stopped being true.
- **Undocumented reorgs surface honestly**: clap's internal flatten
  (#3438) moved `src/build/*` long before 4.0 removed the APIs. The
  engine reports the reorg — the true moment the evidence vanished. A
  file-anchored belief cannot see through a file move; that is exactly
  the "grounds gone, not disproven" semantics, and why span/symbol
  precision matters.
- The two controls that survive as `stale-candidate` (not `active`) show
  the re-verify signal working: their evidence *files* churned, their
  evidence *spans* never did.

## Contrast: flat memory file vs. witnessed belief

The same fact, six months and one refactor later.

**CLAUDE.md-style memory** (served verbatim, no signal anything changed):

```markdown
## Auth notes
- token expiry is 24h            <- silently false since commit fhcpef7c
```

**gpp belief stale** (the same fact, with its killer attached):

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

No benchmark claim intended — the value is operational: audit, staleness,
and time-travel over knowledge, with provenance the repo already had.

## Recording

`belief-bisect.cast` (18 s, 100×30) is the recorded walkthrough — play it
with `asciinema play demos/belief-bisect/belief-bisect.cast`. To re-record
after output changes:

```
asciinema rec --window-size 100x30 --overwrite \
    -c ./demos/belief-bisect/record-demo.sh demos/belief-bisect/belief-bisect.cast
```
