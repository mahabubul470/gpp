#!/bin/sh
# Resumable, idempotent crates.io publish of the whole workspace.
#
# `cargo publish --workspace` aborts when any crate version already exists,
# so a publish interrupted by crates.io's new-crate rate limit (burst, then
# ~1 per 10 min) can't be resumed with it. This walks crate by crate:
# skips versions already on the index, defers crates whose gpp deps aren't
# up yet, sleeps through 429s. Needs CARGO_REGISTRY_TOKEN in the env.
set -u

ver=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
crates=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name')

# Sparse-index lookup (names >= 4 chars: /ab/cd/name).
index_has() {
    name=$1
    p1=$(printf '%s' "$name" | cut -c1-2)
    p2=$(printf '%s' "$name" | cut -c3-4)
    curl -sf "https://index.crates.io/$p1/$p2/$name" 2>/dev/null | grep -q "\"vers\":\"$ver\""
}

for round in $(seq 1 60); do
    missing=0
    for c in $crates; do
        if index_has "$c"; then continue; fi
        missing=$((missing + 1))
        echo "── round $round: publishing $c@$ver"
        if cargo publish -p "$c" --no-verify 2>publish.err; then
            echo "   published $c"
        elif grep -qi "already exists" publish.err; then
            echo "   $c already on index (propagating)"
        elif grep -qi "429\|too many" publish.err; then
            echo "   rate limited — sleeping 10m30s"
            sleep 630
        elif grep -qi "failed to select a version\|not found in registry\|no matching package" publish.err; then
            echo "   deps for $c not on index yet — deferred"
        else
            echo "   unexpected error for $c:"
            cat publish.err
        fi
    done
    if [ "$missing" = "0" ]; then
        echo "ALL $ver CRATES PUBLISHED"
        exit 0
    fi
    echo "── round $round done, $missing crate(s) still missing"
    sleep 60
done
echo "exhausted rounds with crates still missing"
exit 1
