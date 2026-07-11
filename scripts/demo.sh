#!/bin/sh
# The gpp headline demo, scripted and deterministic. Runs in a temp dir.
#
# Arc (< 30 s recorded): continuous capture → promote with intent →
# semantic diff → a belief goes stale and history names the commit.
#
# Record:  asciinema rec --window-size 100x30 --overwrite \
#            -c ./scripts/demo.sh site/assets/demo.cast
# GIF:     agg --font-size 16 site/assets/demo.cast site/assets/demo.gif
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
GPP=${GPP:-"$ROOT/target/release/gpp"}
[ -x "$GPP" ] || { echo "build first: cargo build --release -p gpp-cli" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

say()  { printf '\n\033[1m$ %s\033[0m\n' "$*"; sleep "${PAUSE:-1}"; }
run()  { say "gpp $*"; "$GPP" "$@"; }
note() { printf '\n\033[1;36m%s\033[0m\n' "$*"; sleep "${PAUSE:-1}"; }
quiet(){ "$@" >/dev/null; }

note "# gpp — version control that remembers, and knows when it's wrong"

# --- 1. capture without commits ----------------------------------------------
run init --graphex
mkdir -p src
cat > src/auth.rs <<'EOF'
pub fn issue_token(user: &str) -> String {
    format!("jwt-{user}")
}

pub const EXPIRY_HOURS: u64 = 24;
EOF
run timeline
run promote -m "seed auth" --intent feature

# --- 2. a rename is one semantic op, not 40 changed lines --------------------
sed -i 's/issue_token/create_token/' src/auth.rs
say "gpp diff   # tree-sitter: a rename is one op"
"$GPP" diff | head -4
quiet "$GPP" promote -m "rename issue_token -> create_token"

# --- 3. record what you believe about the code -------------------------------
run belief add --claim "token expiry is 24h" --evidence src/auth.rs:5-5

# --- 4. months later, the code moves on ---------------------------------------
note "# ...weeks pass; someone changes the expiry..."
sed -i 's/EXPIRY_HOURS: u64 = 24/EXPIRY_HOURS: u64 = 168/' src/auth.rs
quiet "$GPP" promote -m "raise token expiry to 7 days"

# --- 5. the payoff: history names the commit ----------------------------------
run belief stale
sleep 1
run belief bisect "token expiry is 24h"
sleep 2

note "# deterministic — no LLM, no network. github.com/mahabubul470/gpp"
sleep 2
