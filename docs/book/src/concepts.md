# Concepts

| Layer | What it does |
|---|---|
| Storage | Content-addressed, zstd-compressed object store (BLAKE3) |
| Timeline | Continuous file-change capture (SQLite index) |
| History | Curated changesets promoted from the timeline, with intent |
| Graphex | Encrypted, tier-gated knowledge graph + context projection |
| Trust | Agent reputation → behavioral status (auto-merge … blocked) |
| Policy | Compliance-as-code, enforced at promotion |
| Cost | Token/compute attribution per changeset |
| Anomaly | Behavioral detection (scope/burst/size) |
| Diff | Tree-sitter semantic diff (Rust/Python/TS/Go) |
| Sync | Noise P2P; objects/refs/policies/graphex |
| Replay | Reproducible environment snapshots |
| Review/RBAC/Notify | Reviews, roles, events + inbox + webhooks |
| Remote | GitHub/GitLab/Bitbucket PRs, plain Git push |

Access tiers (Graphex): `public < agent-readable < agent-restricted <
human-only`. An accessor only ever sees nodes at or below its tier; the rest
are never decrypted.
