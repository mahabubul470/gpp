# Contributing — gpp (git++)

## Development Setup

### Prerequisites

- Rust toolchain (stable, latest): `rustup update stable`
- SQLite3 development headers: `sudo apt install libsqlite3-dev` (Ubuntu/Debian)
- Tree-sitter CLI: `cargo install tree-sitter-cli`
- Just (task runner): `cargo install just`

### Clone & Build

```bash
git clone https://github.com/gpp-vcs/gpp.git
cd gpp
cargo build
cargo test --workspace
```

### Run From Source

```bash
cargo run --bin gpp -- init --graphex /tmp/test-repo
cargo run --bin gpp -- status
```

### Workspace Layout

The project is a Cargo workspace. Each layer is a separate crate under `crates/`:

```
crates/
├── gpp-core/       # Storage, objects, hashing — no dependencies on other gpp crates
├── gpp-timeline/   # Depends on: gpp-core
├── gpp-history/    # Depends on: gpp-core, gpp-timeline
├── gpp-diff/       # Depends on: gpp-core
├── gpp-graphex/    # Depends on: gpp-core
├── gpp-trust/      # Depends on: gpp-core
├── gpp-policy/     # Depends on: gpp-core
├── gpp-review/     # Depends on: gpp-core, gpp-history, gpp-graphex, gpp-trust
├── gpp-rbac/       # Depends on: gpp-core
├── gpp-notify/     # Depends on: gpp-core, gpp-review, gpp-trust, gpp-policy
├── gpp-remote/     # Depends on: gpp-core, gpp-history, gpp-review, gpp-diff, gpp-git-bridge
├── gpp-sync/       # Depends on: gpp-core, gpp-history, gpp-graphex, gpp-rbac
├── gpp-relay/      # Depends on: gpp-core, gpp-sync
├── gpp-cost/       # Depends on: gpp-core
├── gpp-deps/       # Depends on: gpp-core, gpp-graphex
├── gpp-anomaly/    # Depends on: gpp-core, gpp-trust
├── gpp-replay/     # Depends on: gpp-core, gpp-history, gpp-graphex
├── gpp-sdk/        # Depends on: gpp-core, gpp-graphex, gpp-trust, gpp-history
├── gpp-tui/        # Depends on: all crates (UI over everything)
├── gpp-cli/        # Depends on: all crates
└── gpp-git-bridge/ # Depends on: gpp-core, gpp-history
```

The dependency rule: crates at the bottom of the stack never depend on crates above them. `gpp-core` depends on nothing. `gpp-cli` depends on everything.

## Code Conventions

### Rust Style

- Follow `rustfmt` defaults. Run `cargo fmt` before every commit.
- Follow `clippy` recommendations. Run `cargo clippy --workspace` before every commit.
- No `unwrap()` or `expect()` in library crates. Use `?` propagation with proper error types.
- `unwrap()` is acceptable in tests and in `gpp-cli` for known-good assertions.
- Use `thiserror` for error types in library crates.
- Use `anyhow` for error handling in `gpp-cli`.
- Use `tracing` for logging (not `println!` or `eprintln!`).

### Naming

- Crate names: `gpp-<layer>` (kebab-case)
- Module names: `snake_case`
- Types: `PascalCase`
- Functions/methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- CLI command names: lowercase with hyphens (`git-import`, `mcp-server`)

### Error Types

Each library crate defines its own error enum:

```rust
// gpp-core/src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Object not found: {hash}")]
    ObjectNotFound { hash: String },

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Compression error: {0}")]
    Compression(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
```

### Documentation

- Every public type and function must have a doc comment.
- Use `///` for item docs, `//!` for module docs.
- Include usage examples in doc comments for non-trivial APIs.
- Run `cargo doc --workspace --no-deps` to verify docs build.

### Testing

- Unit tests go in the same file as the code, in a `#[cfg(test)] mod tests {}` block.
- Integration tests go in `tests/integration/`.
- Test fixtures go in `tests/fixtures/`.
- Use `tempfile` crate for tests that need a filesystem.
- Every bug fix must include a regression test.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_store_and_retrieve_blob() {
        let dir = TempDir::new().unwrap();
        let store = ObjectStore::new(dir.path()).unwrap();

        let content = b"hello world";
        let hash = store.write_blob(content).unwrap();
        let retrieved = store.read_blob(&hash).unwrap();

        assert_eq!(retrieved, content);
    }
}
```

### Benchmarks

Performance-sensitive code should have benchmarks using `criterion`:

```bash
cargo bench --bench storage
```

Benchmark files go in `benches/` at the workspace root.

## Commit Messages

Follow Conventional Commits:

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`, `ci`

Scopes: crate names without `gpp-` prefix: `core`, `timeline`, `history`, `diff`, `graphex`, `trust`, `policy`, `sync`, `cost`, `deps`, `anomaly`, `replay`, `cli`, `sdk`, `git-bridge`

Examples:
```
feat(graphex): add context projection engine
fix(timeline): fix race condition in debounce logic
docs(cli): add examples for gpp promote command
perf(core): switch to memory-mapped blob reads
test(trust): add regression test for score calculation edge case
```

## Pull Request Process

1. Create a branch: `feat/graphex-query-engine` or `fix/timeline-debounce`
2. Write code + tests
3. Run the full check: `just check` (or manually: `cargo fmt && cargo clippy --workspace && cargo test --workspace`)
4. Open PR with description of what and why
5. At least one review approval required
6. Squash merge into main

## Architecture Decision Records

Significant design decisions are recorded in `docs/decisions/`:

```
docs/decisions/
├── 001-blake3-over-sha256.md
├── 002-sqlite-for-indexes.md
├── 003-age-for-encryption.md
├── 004-crdt-sync-over-raft.md
└── ...
```

Template:

```markdown
# ADR-NNN: Title

## Status: Accepted | Deprecated | Superseded by ADR-XXX

## Context
What is the issue that we're seeing that motivates this decision?

## Decision
What is the change that we're proposing?

## Consequences
What becomes easier or harder to do because of this change?
```

## Justfile

Common tasks are defined in a `Justfile`:

```just
# Run all checks (format, lint, test)
check:
    cargo fmt --check
    cargo clippy --workspace -- -D warnings
    cargo test --workspace

# Build release binary
build:
    cargo build --release

# Run the CLI from source
run *ARGS:
    cargo run --bin gpp -- {{ARGS}}

# Generate docs
docs:
    cargo doc --workspace --no-deps --open

# Run benchmarks
bench:
    cargo bench

# Clean build artifacts
clean:
    cargo clean
```

## Issue Labels

| Label | Description |
|-------|-------------|
| `layer:core` | Storage layer |
| `layer:timeline` | Timeline layer |
| `layer:history` | History layer |
| `layer:graphex` | Knowledge graph |
| `layer:trust` | Trust engine |
| `layer:policy` | Policy engine |
| `layer:diff` | Semantic diff |
| `layer:sync` | Sync protocol |
| `layer:cost` | Cost attribution |
| `layer:cli` | CLI interface |
| `layer:tui` | Terminal UI |
| `layer:sdk` | Agent SDK |
| `layer:review` | Code review workflow |
| `layer:rbac` | Human permissions |
| `layer:notify` | Notification system |
| `layer:remote` | Platform integration (GitHub/GitLab/BB) |
| `layer:relay` | Relay node |
| `ext:gh-gpp` | GitHub CLI extension |
| `ext:vscode` | VS Code extension |
| `ext:neovim` | Neovim plugin |
| `good-first-issue` | Good for newcomers |
| `help-wanted` | Extra attention needed |
| `bug` | Something isn't working |
| `enhancement` | New feature or improvement |
| `performance` | Performance improvement |
| `documentation` | Documentation improvement |
| `security` | Security-related |

## License

MIT License. All contributions must be compatible with MIT.
