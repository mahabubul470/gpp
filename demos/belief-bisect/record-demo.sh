#!/bin/sh
# Belief Bisect — paced synthetic walkthrough for an asciinema recording.
# Mirrors the tier-1 CI scenario (crates/gpp-cli/tests/belief_bisect.rs):
# seed beliefs → staged refactor → stale → bisect → time-travel. < 2 min.
#
#   asciinema rec -c ./demos/belief-bisect/record-demo.sh belief-bisect.cast
set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GPP=${GPP:-"$ROOT/target/release/gpp"}
[ -x "$GPP" ] || { echo "build first: cargo build --release -p gpp-cli" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

say() { printf '\n\033[1m$ %s\033[0m\n' "$*"; sleep "${PAUSE:-1}"; }
# Run a gpp subcommand, echoing it as `gpp …` (not the full binary path).
run() { say "gpp $*"; "$GPP" "$@"; }

quiet() { "$@" >/dev/null; }

# --- setup (not narrated) ---------------------------------------------------
quiet "$GPP" init --graphex
mkdir -p auth
cat > auth/token.rs <<'EOF'
use crate::jwt;

pub fn issue_token(user: &str) -> String {
    jwt::encode(user, EXPIRY_HOURS)
}

pub const EXPIRY_HOURS: u64 = 24;
EOF
quiet "$GPP" promote -m "seed auth module with JWT issuance"

printf '\033[1;36m== A belief is a claim about the code, anchored to a commit with evidence ==\033[0m\n'
run belief add --claim "token expiry is 24h" \
    --path 'auth/**' --evidence auth/token.rs:7-7
run belief add --claim "auth issues JWTs" \
    --path 'auth/**' --evidence auth/token.rs:3-5

# --- history happens (narrated tersely) --------------------------------------
printf '\n\033[1;36m== Months pass. The code moves on. ==\033[0m\n'; sleep 1
sed -i '1i // SPDX-License-Identifier: MIT' auth/token.rs
quiet "$GPP" promote -m "add license header"
sed -i 's/EXPIRY_HOURS: u64 = 24/EXPIRY_HOURS: u64 = 168/' auth/token.rs
quiet "$GPP" promote -m "raise token expiry to 7 days"
rm auth/token.rs
cat > auth/session.rs <<'EOF'
pub fn create_session(user: &str) -> String {
    format!("session-{user}")
}
EOF
quiet "$GPP" promote -m "migrate JWT issuance to server-side sessions"
echo "  (3 commits: license header · expiry 24h -> 7d · JWT -> sessions)"; sleep 1

# --- the payoff ---------------------------------------------------------------
printf '\n\033[1;36m== Which beliefs still hold? ==\033[0m\n'
run belief stale
sleep 2

printf '\n\033[1;36m== Which commit killed each one? ==\033[0m\n'
run belief bisect "token expiry is 24h"
sleep 2
run belief bisect "auth issues JWTs"
sleep 2

printf '\n\033[1;36m== Time-travel: the belief set as it stood at the first commit ==\033[0m\n'
FIRST=$("$GPP" belief log "token expiry is 24h" --json | sed -n 's/.*"anchor": "\([a-z0-9]*\)".*/\1/p')
run belief at "$FIRST"
sleep 2

printf '\n\033[1;36m== Deterministic. No LLM. Just the history the repo already had. ==\033[0m\n'
sleep 2
