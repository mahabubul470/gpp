# Installation

## From crates.io (recommended)

```bash
cargo install gpp-cli
```

This builds the `gpp` binary. The relay is a separate binary:

```bash
cargo install gpp-relay
```

To track unreleased development instead, install from git:

```bash
cargo install --git https://github.com/mahabubul470/gpp gpp-cli
```

## Prebuilt binaries

Each [GitHub release](https://github.com/mahabubul470/gpp/releases)
attaches archives for Linux x86_64, macOS (Apple Silicon and Intel),
and Windows x86_64 — unpack and put `gpp` (and optionally `gpp-relay`)
on your `PATH`.

## Homebrew

```bash
brew install mahabubul470/tap/gpp
```

## Docker

```bash
docker run --rm -v "$PWD:/work" -w /work ghcr.io/mahabubul470/gpp:latest status
docker run -p 9473:9473 -p 9474:9474 -v gpp-data:/data ghcr.io/mahabubul470/gpp-relay
```

## Install script

```bash
curl -fsSL https://raw.githubusercontent.com/mahabubul470/gpp/main/scripts/install.sh | sh
```

Verify:

```bash
gpp --version
```
