#!/bin/sh
# Belief Bisect — validation-matrix driver: replay the axum methodology
# against any pinned real-history repo (see repos/*.conf).
#
# A config defines NAME, URL, OLD, NEW (full pinned SHAs — never float) and a
# seed_beliefs() function that calls the `belief` helper below with the
# expected culprit for each claim. The driver imports OLD through the git
# bridge, seeds, advances to NEW, re-imports, bisects every belief, and
# asserts each verdict lands on the expected pinned culprit (or survives).
# Zero LLM/network calls in the engine — the only network use is the clone.
#
# Usage:  demos/belief-bisect/run-repo-demo.sh repos/<name>.conf [workdir]
#         GPP=/path/to/gpp to override the binary (default: release build).
set -eu

CONF=$1
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GPP=${GPP:-"$ROOT/target/release/gpp"}
[ -x "$GPP" ] || { echo "gpp binary not found at $GPP — run: cargo build --release -p gpp-cli" >&2; exit 1; }

# shellcheck source=/dev/null
. "$(cd "$(dirname "$CONF")" && pwd)/$(basename "$CONF")"

WORK=${2:-"$(mktemp -d)"}
SRC="$WORK/$NAME"
REPO="$WORK/$NAME-gpp"
MANIFEST="$WORK/$NAME.manifest"
: > "$MANIFEST"

echo "== $NAME: $OLD -> $NEW"
echo "== workdir: $WORK"

# belief <expected-culprit-sha|SURVIVES> <documented-ref> <claim> [extra gpp belief add args...]
# Records the expectation and seeds the belief (evidence paths are repo-relative).
belief() {
    _exp=$1; _ref=$2; _claim=$3; shift 3
    printf '%s\t%s\t%s\n' "$_exp" "$_ref" "$_claim" >> "$MANIFEST"
    "$GPP" belief add --claim "$_claim" "$@"
}

# --- 1. Clone and pin the old world -----------------------------------------
if [ ! -d "$SRC/.git" ]; then
    echo "== cloning $URL (network; one-time)"
    git clone --quiet "$URL" "$SRC"
fi
git -C "$SRC" rev-parse --verify -q "$OLD^{commit}" >/dev/null \
    || { echo "pinned OLD commit missing upstream — refusing to float" >&2; exit 1; }
git -C "$SRC" checkout -qB main "$OLD"

# --- 2. Import old history into gpp -----------------------------------------
mkdir -p "$REPO"
cd "$REPO"
[ -d .gpp ] || "$GPP" init --graphex
echo "== importing git history up to OLD pin"
"$GPP" git-import "$SRC"

# --- 3. Seed beliefs true at the old pin ------------------------------------
echo "== seeding beliefs"
seed_beliefs

# --- 4. History arrives: advance to NEW and re-import (incremental) ---------
echo "== advancing to NEW pin and re-importing"
git -C "$SRC" checkout -qB main "$NEW"
"$GPP" git-import "$SRC"

# --- 5. Bisect every belief and assert the expected culprit -----------------
echo
echo "==================== gpp belief stale ===================="
"$GPP" belief stale
echo
echo "==================== bisect + expectation check =========="
fail=0
while IFS="$(printf '\t')" read -r exp ref claim; do
    echo "---- $claim"
    "$GPP" belief bisect "$claim" || true
    json=$("$GPP" belief bisect "$claim" --json)
    sha=$(printf '%s' "$json" | sed -n 's/.*"git_commit": "\([0-9a-f]*\)".*/\1/p' | head -1)
    status=$(printf '%s' "$json" | sed -n 's/.*"status": "\([a-z-]*\)".*/\1/p' | head -1)
    if [ "$exp" = SURVIVES ]; then
        # Survival = not invalidated. A scope touch that leaves the evidence
        # span intact is a stale-candidate (re-verify signal) — that counts.
        if [ "$status" != invalidated ]; then
            echo "   OK: survives as '$status' (control) [$ref]"
        else
            echo "   FAIL: expected survival, invalidated at ${sha:-<none>}"; fail=1
        fi
    else
        if [ "$sha" = "$exp" ]; then
            subject=$(git -C "$SRC" log -1 --format='%h %s' "$sha")
            echo "   OK: culprit $subject [$ref]"
        else
            echo "   FAIL: expected $exp, got ${sha:-<none>}"; fail=1
        fi
    fi
    echo
done < "$MANIFEST"

[ "$fail" -eq 0 ] && echo "== $NAME: all expectations met." \
                  || { echo "== $NAME: EXPECTATION FAILURES (see above)."; exit 1; }
echo "repo left at $REPO for exploration (gpp belief log/at/stale)."
