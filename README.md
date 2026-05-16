# gpp (git++)

An AI-native version control system built in Rust. gpp replaces Git's
commit-centric model with a continuous-capture architecture designed for a
world where AI agents are first-class code contributors.

See [`CLAUDE.md`](CLAUDE.md) for project context and [`docs/`](docs/) for the
full specification (architecture, data model, CLI, protocols, roadmap).

## Status

**Phase 0 (Foundation)** — in progress. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

Implemented:

- `gpp-core` — content-addressed object store (BLAKE3 + zstd), `Blob` / `Tree`
  objects, atomic idempotent writes, hash-verified reads.
- `gpp-cli` — the `gpp` binary with `init`, `status`, and `config`.
- CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.

All other crates in the workspace are compiling stubs filled in by later phases.

## Build

```bash
cargo build --release
cargo test --workspace

# Try it
cargo run --bin gpp -- init --graphex
cargo run --bin gpp -- status
cargo run --bin gpp -- config get trust.auto_merge_min
```
