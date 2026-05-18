# Command reference

The authoritative, exhaustive spec lives in
[`docs/CLI_SPEC.md`](https://github.com/gpp-vcs/gpp/blob/main/docs/CLI_SPEC.md).
`gpp <command> --help` is generated from the same definitions.

Quick map:

| Area | Commands |
|---|---|
| Core | `init` `status` `config` |
| History | `timeline` `promote` `log` `diff` `branch` `merge` |
| Git bridge | `git-import` `git-export` `git-bridge` |
| Graphex | `keys` `graphex` `mcp-server` |
| Governance | `trust` `policy` `cost` `anomaly` `audit` |
| Collaboration | `review` `rbac` `inbox` `notify` |
| Decentralized | `sync` `replay` `relay` |
| Remote | `remote` |
| Clients | `ui` `deps` |

API reference (rustdoc):

```bash
cargo doc --workspace --no-deps --open
```
