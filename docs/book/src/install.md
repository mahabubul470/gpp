# Installation

## From source (cargo)

```bash
cargo install --git https://github.com/mahabubul470/gpp gpp-cli
```

This builds the `gpp` binary. The relay is a separate binary:

```bash
cargo install --git https://github.com/mahabubul470/gpp gpp-relay
```

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
