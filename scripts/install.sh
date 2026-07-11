#!/bin/sh
# gpp installer. Builds from source with cargo (the supported path until
# prebuilt binaries are published). Usage:
#   curl -fsSL https://raw.githubusercontent.com/mahabubul470/gpp/main/scripts/install.sh | sh
set -eu

REPO="${GPP_REPO:-https://github.com/mahabubul470/gpp}"

err() { echo "install: $*" >&2; exit 1; }
command -v cargo >/dev/null 2>&1 || err "cargo not found — install Rust from https://rustup.rs"

echo "Installing gpp from ${REPO} (this builds from source)…"
cargo install --git "$REPO" gpp-cli
if [ "${GPP_WITH_RELAY:-0}" = "1" ]; then
  cargo install --git "$REPO" gpp-relay
fi

if command -v gpp >/dev/null 2>&1; then
  echo "Installed: $(gpp --version)"
else
  err "gpp not on PATH — add ~/.cargo/bin to PATH"
fi
