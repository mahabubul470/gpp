---
name: Good first issue
about: A small, well-scoped task for new contributors
title: "[good first issue] "
labels: ["good first issue"]
---

**Scope:** one crate, < ~100 lines, has an obvious test.

**Context:** <!-- which layer/file, why it matters -->

**Acceptance:**
- [ ] implementation
- [ ] unit test
- [ ] `cargo test --workspace` green, `cargo clippy`/`cargo fmt` clean

See `docs/CONTRIBUTING.md` and `docs/ROADMAP.md` for orientation. Every
crate's `lib.rs` has a module doc pointing at the relevant design doc.
