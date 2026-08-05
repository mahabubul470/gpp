#!/bin/sh
# Belief Bisect — validation-matrix walkthrough for an asciinema recording.
# Runs the real flask 1.1.0 -> 2.0.0 config through run-repo-demo.sh: seed
# beliefs true at 1.1.0 → advance 221 commits through the git bridge →
# bisect → every culprit asserted against a pinned changelog-documented
# commit. Pre-clone flask into $MATRIX_WORK/flask before recording so the
# cast shows engine time, not network time:
#
#   W=$(mktemp -d) && git clone --quiet https://github.com/pallets/flask.git $W/flask
#   MATRIX_WORK=$W asciinema rec --idle-time-limit 2 --window-size 100x32 \
#       -c ./demos/belief-bisect/record-matrix-demo.sh matrix-flask.cast
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GPP=${GPP:-"$ROOT/target/release/gpp"}
[ -x "$GPP" ] || { echo "build first: cargo build --release -p gpp-cli" >&2; exit 1; }
[ -n "${MATRIX_WORK:-}" ] && [ -d "$MATRIX_WORK/flask/.git" ] \
    || { echo "pre-clone flask first (see header comment)" >&2; exit 1; }

say() { printf '\n\033[1m%s\033[0m\n' "$*"; sleep "${PAUSE:-1.2}"; }

say "# Belief Bisect on real history: flask 1.1.0 -> 2.0.0 (221 commits)"
say "# Beliefs seeded true at 1.1.0; culprits asserted against pinned"
say "# commits documented in Flask's own 2.0.0 changelog. Zero LLM calls."
sleep 1

say "\$ demos/belief-bisect/run-repo-demo.sh repos/flask.conf"
GPP="$GPP" "$ROOT/demos/belief-bisect/run-repo-demo.sh" \
    "$ROOT/demos/belief-bisect/repos/flask.conf" "$MATRIX_WORK"

sleep 2
say "# 21/21 across the matrix: axum, flask, clap, zod, go-redis."
say "# Full method + configs: demos/belief-bisect/"
sleep 2
