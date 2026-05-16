# gpp (git++)

An AI-native version control system built in Rust. gpp replaces Git's
commit-centric model with a continuous-capture architecture designed for a
world where AI agents are first-class code contributors.

See [`CLAUDE.md`](CLAUDE.md) for project context and [`docs/`](docs/) for the
full specification (architecture, data model, CLI, protocols, roadmap).

## Status

**Phase 1 (Timeline + Basic History)** — complete. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

Implemented:

- `gpp-core` — content-addressed object store (BLAKE3 + zstd); `Blob`, `Tree`,
  `Changeset`, `Intent` object types; atomic idempotent writes; hash-verified reads.
- `gpp-timeline` — SQLite (WAL) timeline DB, working-tree scanner, `.gppignore`
  + configured ignore matching, debounced `notify` watcher, retention pruning.
- `gpp-history` — `Changeset`/`Intent`/`Author` objects, branch `RefStore`,
  promote (timeline → changeset), changeset-DAG walk.
- `gpp-diff` — line-based unified diff + stats (semantic diff is Phase 2).
- `gpp-cli` — the `gpp` binary: `init`, `status`, `config`, `timeline`
  (list/watch/search/prune/export), `promote`, `log`, `diff`, `branch`.
- CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.

Deferred to later phases (rejected with a clear message if invoked): semantic
diff, `promote --interactive/--auto-summarize/--sign`, Git bridge, AI features.
The remaining workspace crates are compiling stubs filled in by later phases.

## Build

```bash
cargo build --release
cargo test --workspace

# Try it (Phase 1 solo-dev flow)
cargo run --bin gpp -- init --graphex
echo "fn main() {}" > main.rs
cargo run --bin gpp -- timeline                 # see continuous capture
cargo run --bin gpp -- promote -m "first cut" --intent feature
cargo run --bin gpp -- log --oneline
cargo run --bin gpp -- diff
cargo run --bin gpp -- branch create feature/x
cargo run --bin gpp -- status
```
