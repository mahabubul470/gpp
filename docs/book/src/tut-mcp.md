# Tutorial: Connecting AI agents via MCP

gpp ships an MCP server over stdio. Point your agent tool at it:

```json
{ "mcpServers": { "gpp": { "command": "gpp", "args": ["mcp-server", "--stdio"] } } }
```

Tools exposed: `graphex_query`, `graphex_status`, `graphex_glossary`,
`graphex_conventions`, `propose_changeset`, `propose_graph_update`.

All reads are tier-gated by `--trust-tier` (default `agent-readable`).
Agent-proposed nodes land as **Proposed** and require human approval:

```bash
gpp graphex pending
gpp graphex accept <name>      # or: reject
```

Native (Tier 3) agents can instead use the Rust `gpp-sdk`
(`AgentSession::{query_graphex, propose_changeset, propose_graph_update}`).
