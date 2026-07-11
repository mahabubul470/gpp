#!/bin/sh
# Belief Bisect — tier-2 demo on real history: axum 0.6 → 0.7.
#
# Imports axum's git history through the gpp git bridge, seeds beliefs that
# were TRUE at the pinned v0.6.0 commit (with evidence spans in the real
# source), advances the branch to v0.7.0, and asks the staleness engine which
# commits killed which beliefs. Cross-checks the culprits against axum's own
# changelog. The core path makes zero LLM/network calls — the only network
# use in this script is the initial `git clone`.
#
# Usage:  demos/belief-bisect/run-axum-demo.sh [workdir]
#         GPP=/path/to/gpp to override the binary (default: release build).
set -eu

# Pinned upstream commits — never float (handoff §6).
AXUM_V060=1b6780cf6cfe28fa24744bb2d9581cd01577c464   # tag axum-v0.6.0
AXUM_V070=b7d14d3602c401c7f0ece6b51e995d82ddccb1e1   # tag axum-v0.7.0

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GPP=${GPP:-"$ROOT/target/release/gpp"}
WORK=${1:-"$(mktemp -d)"}
AXUM="$WORK/axum"
REPO="$WORK/axum-gpp"

[ -x "$GPP" ] || { echo "gpp binary not found at $GPP — run: cargo build --release -p gpp-cli" >&2; exit 1; }

echo "== workdir: $WORK"
mkdir -p "$WORK"

# --- 1. Clone axum and pin the 0.6 world ------------------------------------
if [ ! -d "$AXUM/.git" ]; then
    echo "== cloning axum (network; one-time)"
    git clone --quiet https://github.com/tokio-rs/axum.git "$AXUM"
fi
git -C "$AXUM" rev-parse --verify -q "$AXUM_V060^{commit}" >/dev/null \
    || { echo "pinned v0.6.0 commit missing upstream — refusing to float" >&2; exit 1; }
git -C "$AXUM" checkout -qB main "$AXUM_V060"

# --- 2. Import 0.6-era history into gpp -------------------------------------
mkdir -p "$REPO"
cd "$REPO"
[ -d .gpp ] || { "$GPP" init --graphex; }
echo "== importing git history up to v0.6.0 (first import takes a minute)"
"$GPP" git-import "$AXUM"

# --- 3. Seed beliefs that were true at v0.6.0 -------------------------------
# Evidence lines verified against the pinned commit:
#   axum/src/routing/mod.rs:64      pub struct Router<S = (), B = Body> {
#   axum/src/lib.rs:471             pub use hyper::Server;
#   axum/src/body/mod.rs:11         pub use hyper::body::Body;
#   axum/src/extract/request_parts.rs:136  pub struct BodyStream(
#
# Note the Router evidence pins the *signature line only* (64-64): commits
# that rework the struct's private fields (e.g. #1806) then read as noise,
# and the invalidation lands exactly on the commit that removed the B
# param. Span precision controls verdict precision.
echo "== seeding beliefs anchored at v0.6.0"
"$GPP" belief add \
    --claim "Router is generic over the request body type (Router<S, B>)" \
    --symbol axum/src/routing/mod.rs:Router \
    --evidence axum/src/routing/mod.rs:64-64
"$GPP" belief add \
    --claim "axum re-exports hyper::Server; apps start with axum::Server::bind" \
    --evidence axum/src/lib.rs:471-471
"$GPP" belief add \
    --claim "axum::body::Body is hyper's Body type re-exported" \
    --evidence axum/src/body/mod.rs:11-11
"$GPP" belief add \
    --claim "request bodies can be streamed with extract::BodyStream" \
    --evidence axum/src/extract/request_parts.rs:136-140
"$GPP" belief add \
    --claim "shared state is extracted with State<T>" \
    --symbol axum/src/extract/state.rs:State

# --- 4. History arrives: advance to v0.7.0 and re-import (incremental) ------
echo "== advancing to v0.7.0 and re-importing"
git -C "$AXUM" checkout -qB main "$AXUM_V070"
"$GPP" git-import "$AXUM"

# --- 5. Ask the engine ------------------------------------------------------
echo
echo "==================== gpp belief stale ===================="
"$GPP" belief stale
echo
echo "==================== bisect each belief =================="
for b in "Router is generic over the request body type (Router<S, B>)" \
         "axum re-exports hyper::Server; apps start with axum::Server::bind" \
         "axum::body::Body is hyper's Body type re-exported" \
         "request bodies can be streamed with extract::BodyStream" \
         "shared state is extracted with State<T>"; do
    echo "---- $b"
    "$GPP" belief bisect "$b"
    echo
done

# --- 6. Ground truth: map culprits to axum's changelog ----------------------
echo "==================== changelog cross-check ================"
echo "culprit git SHAs reported above vs axum/CHANGELOG.md (0.7.0 section):"
for b in "Router is generic over the request body type (Router<S, B>)" \
         "axum re-exports hyper::Server; apps start with axum::Server::bind" \
         "axum::body::Body is hyper's Body type re-exported" \
         "request bodies can be streamed with extract::BodyStream"; do
    sha=$("$GPP" belief bisect "$b" --json | sed -n 's/.*"git_commit": "\([0-9a-f]*\)".*/\1/p' | head -1)
    [ -n "$sha" ] || continue
    subject=$(git -C "$AXUM" log -1 --format='%h %s' "$sha")
    pr=$(printf '%s' "$subject" | sed -n 's/.*(#\([0-9]*\)).*/\1/p')
    inlog=no
    [ -n "$pr" ] && git -C "$AXUM" show "$AXUM_V070:axum/CHANGELOG.md" | grep -q "#$pr" && inlog=yes
    printf '  %-70.70s -> %s  [in 0.7.0 changelog: %s]\n' "$b" "$subject" "$inlog"
done

echo
echo "done. repo left at $REPO for exploration (gpp belief log/at/stale)."
