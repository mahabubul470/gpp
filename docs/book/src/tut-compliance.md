# Tutorial: Compliance for regulated industries

```bash
gpp policy templates                       # secrets-scan, pci-dss, soc2
gpp policy template pci-dss                # install it
gpp policy template secrets-scan
gpp policy check                           # run against the working tree
```

Policies are enforced at **promotion** — a `block`-severity hit aborts
`gpp promote` before any changeset object is written, so a leaked key never
enters history. Branch protection adds review gates:

```bash
gpp rbac assign lead@acme.io maintainer
gpp rbac protect main --min-reviewers 2 --require-human true --require-role maintainer
```

Audit across every layer (trust, anomaly, cost, graphex access):

```bash
gpp audit --include-cost --include-graphex
```

The `gpp-policy-check` / `gpp-trust-gate` / `gpp-audit-report` GitHub
Actions (and the GitLab template) run the same checks in CI.
