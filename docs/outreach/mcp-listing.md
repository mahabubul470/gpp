# MCP directory / awesome-list entries

PR-ready copy for MCP server directories and awesome lists
(awesome-mcp-servers and similar). Keep entries exactly as written —
claims are scoped to what the current build does.

## One-line description

> **gpp** — Version-control-native project memory for agents: a knowledge
> graph with staleness-checked beliefs, plus changeset proposal and token
> cost reporting, from an AI-native VCS written in Rust.

Alternate (shorter, for tables with tight columns):

> **gpp** — Project knowledge graph with history-checked staleness,
> changeset proposals, and cost attribution over stdio MCP.

## Short description (2–3 sentences)

> gpp is an AI-native version control system (Rust, single binary) whose
> MCP server gives an agent the project's knowledge graph — architecture,
> conventions, glossary, and *beliefs* whose staleness is checked
> deterministically against repo history, so the agent is warned in-line
> when a recorded fact has been overtaken by a commit — plus tools to
> propose changesets for human review and report its token costs per
> changeset. Local-only stdio transport; graph reads are trust-tier
> gated, encrypted at rest (age + AES-256-GCM), and access-logged.
> Bridges to plain Git, so it works alongside existing GitHub workflows.

## Category / tags

Version Control · Knowledge Graph · Agent Memory · Developer Tools ·
Rust · Local / stdio

## Config snippet

```json
{
  "mcpServers": {
    "gpp": {
      "command": "gpp",
      "args": ["mcp-server", "--stdio"]
    }
  }
}
```

Install: `cargo install gpp-cli`,
then `gpp init --graphex .` in the project. If running from a source
checkout instead of PATH, use `"command": "target/release/gpp"` (this is
the repo's own working `.mcp.json`).

## Tools exposed (current build)

`graphex_query`, `graphex_status`, `graphex_glossary`,
`graphex_conventions`, `propose_changeset`, `propose_graph_update`,
`report_cost`.

## Links

- Repo: https://github.com/mahabubul470/gpp
- MCP setup doc: https://github.com/mahabubul470/gpp/blob/main/docs/MCP.md
- Belief staleness demo (axum 0.6→0.7 validation):
  https://github.com/mahabubul470/gpp/tree/main/demos/belief-bisect
- User guide: https://mahabubul470.github.io/gpp/
