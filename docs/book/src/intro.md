# Introduction

**gpp (git++)** is an AI-native version control system in Rust. Git was built
for humans making deliberate, sequential commits; gpp is built for a world
where AI agents produce changes continuously, across many files, faster than
humans can review.

Core ideas:

- **Continuous timeline** capture instead of manual `add`/`commit`.
- **Curated changesets** promoted from the timeline, each with *intent*.
- **Graphex** — an encrypted, tier-gated knowledge graph agents query for
  context (they never see raw or over-tier nodes).
- **Trust, policy, cost, anomaly** governance enforced at the storage layer.
- **P2P sync** over Noise, with GitHub/GitLab/Bitbucket as first-class
  targets — Git only ever sees clean commits.

Everything works locally and offline; hosted infrastructure is optional.
