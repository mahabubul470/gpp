# Policy template marketplace

Built-in templates ship in the `gpp` binary and are installable with:

```bash
gpp policy templates                 # list
gpp policy template secrets-scan     # install into .gpp/policies/
```

The canonical source of each built-in lives here for review/auditing:

| Template | Purpose |
|---|---|
| `secrets-scan.policy` | Block committed AWS keys / PEM keys; warn on hard-coded secrets |
| `pci-dss.policy` | Block card PANs in source; require human review of payment paths |
| `soc2.policy` | Require human review of production config; warn on huge changesets |

## Contributing a template

A policy is TOML with `pattern` (regex on file content) and/or `changeset`
rules (author / files / review). Add a `*.policy` file here, validate it,
and open a PR:

```bash
gpp policy validate policies/my-rule.policy
```

Custom org policies don't need to live here — `gpp policy add <file>`
installs any `.policy` into a repo, and shared policies sync between peers
(local overrides do not).
