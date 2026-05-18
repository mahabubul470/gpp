# Installation

## From source (cargo)

```bash
cargo install --git https://github.com/gpp-vcs/gpp gpp-cli
```

This builds the `gpp` binary. The relay is a separate binary:

```bash
cargo install --git https://github.com/gpp-vcs/gpp gpp-relay
```

## Homebrew

```bash
brew install gpp-vcs/tap/gpp
```

## Docker

```bash
docker run --rm -v "$PWD:/work" -w /work ghcr.io/gpp-vcs/gpp:latest status
docker run -p 9473:9473 -p 9474:9474 -v gpp-data:/data ghcr.io/gpp-vcs/gpp-relay
```

## Install script

```bash
curl -fsSL https://raw.githubusercontent.com/gpp-vcs/gpp/main/scripts/install.sh | sh
```

Verify:

```bash
gpp --version
```
