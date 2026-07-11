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
