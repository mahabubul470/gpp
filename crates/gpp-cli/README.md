# gpp — AI-native version control (CLI)

`gpp` is an AI-native version control system in Rust: continuous change
capture, curated changesets with intent and provenance, and an encrypted
in-repo knowledge graph whose *beliefs* are staleness-checked against the
repo's own history — `gpp belief bisect` names the commit that invalidated
a recorded fact. Bridges to plain Git, so it works alongside GitHub.

```bash
cargo install gpp-cli

gpp init --graphex
gpp belief add --claim "token expiry is 24h" --evidence src/auth.rs:7-7
# ...history happens...
gpp belief bisect "token expiry is 24h"
# INVALIDATED  cs:fhcpef7c  "raise token expiry to 7 days"  (+ offending hunk)
```

## MCP server

The binary ships a Model Context Protocol server. Agents get
`graphex_query` (project context where every belief carries a freshness
envelope), `propose_belief` (agent-written, evidence-anchored beliefs that
history polices), `propose_changeset`, and `report_cost`:

```json
{
  "mcpServers": {
    "gpp": { "command": "gpp", "args": ["mcp-server", "--stdio"] }
  }
}
```

## Links

- Repository: <https://github.com/mahabubul470/gpp>
- User guide: <https://mahabubul470.github.io/gpp/>
- MCP setup: <https://github.com/mahabubul470/gpp/blob/main/docs/MCP.md>
- Validation (belief bisect across axum/flask/clap/zod/go-redis):
  <https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect>
- MCP Registry name: `mcp-name: io.github.mahabubul470/gpp`

License: MIT
