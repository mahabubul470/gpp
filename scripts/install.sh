#!/bin/sh
# gpp installer. Installs the published crates.io release with cargo;
# set GPP_FROM_GIT=1 to build the latest development snapshot instead.
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mahabubul470/gpp/main/scripts/install.sh | sh
set -eu

REPO="${GPP_REPO:-https://github.com/mahabubul470/gpp}"

err() { echo "install: $*" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || err "cargo not found — install Rust from https://rustup.rs"

if [ "${GPP_FROM_GIT:-0}" = "1" ]; then
  echo "Installing gpp from ${REPO} (development snapshot, builds from source)…"
  cargo install --git "$REPO" gpp-cli
  [ "${GPP_WITH_RELAY:-0}" = "1" ] && cargo install --git "$REPO" gpp-relay
else
  echo "Installing gpp from crates.io (builds from source)…"
  cargo install gpp-cli
  [ "${GPP_WITH_RELAY:-0}" = "1" ] && cargo install gpp-relay
fi

if command -v gpp >/dev/null 2>&1; then
  echo "Installed: $(gpp --version)"
else
  err "gpp not on PATH — add ~/.cargo/bin to PATH"
fi
