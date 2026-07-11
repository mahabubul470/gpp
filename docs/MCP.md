# MCP — connecting AI clients to gpp

gpp ships a built-in [Model Context Protocol](https://modelcontextprotocol.io)
server. Any MCP client (Claude Code, Claude Desktop, Cursor, or your own
JSON-RPC client) can query the repository's knowledge graph, propose
changesets, and report token costs — with access filtered by trust tier and
every read logged.

```bash
gpp mcp-server --stdio
```

Transport is JSON-RPC 2.0 over newline-delimited stdio (the MCP stdio
transport, protocol version `2025-06-18`). **Only stdio is implemented** —
`--port` is reserved and currently errors. There is no network listener;
the server reads stdin and writes stdout of the process your MCP client
spawns, nothing else.

Run it from inside a gpp repository (`gpp init --graphex .` first if you
haven't).

## Tools

These are the tools the current build serves
(`crates/gpp-cli/src/mcp.rs`):

| Tool | Arguments | What it does |
|---|---|---|
| `graphex_query` | `pattern` (optional), `budget` (tokens, default 8000) | Project a tier-filtered slice of the knowledge graph as text: architecture, modules, decisions — and a **Beliefs** section where stale or invalidated beliefs carry ⚠/✗ flags, so the agent is warned in-context when a recorded fact has been overtaken by history. |
| `graphex_status` | — | Node/edge counts for the graph. |
| `graphex_glossary` | `term` (optional filter) | Domain glossary lookup. |
| `graphex_conventions` | — | Coding conventions recorded in the graph. |
| `propose_changeset` | `message` (required), `intent` (feature/fix/refactor/test/docs/chore) | Promote the pending timeline entries into a changeset. Returns the changeset id. Agent proposals land for human review like any other agent's work. |
| `propose_graph_update` | `node_type`, `name` (required), `description`, `tier` | Suggest a new knowledge-graph node. It lands in the `Proposed` state for a human to approve — never applied silently. |
| `report_cost` | `changeset` (required), `model`, `input_tokens`, `output_tokens`, `cached_tokens`, `cost_microdollars` | Attribute token/compute usage to a changeset. Reports accumulate; costs are integer micro-dollars (1 = $0.000001). |

`docs/GRAPHEX_PROTOCOL.md` specifies a larger planned tool surface
(timeline, exploration branches, trust, policy, review). The seven above
are what `tools/list` returns today — anything else is spec, not build.

The server's `initialize` response includes workflow instructions, so a
connecting agent learns the query → edit → propose → report-cost loop
without you explaining it.

## Setup

### Claude Code

Drop a `.mcp.json` at the repository root. This is the working config this
repo itself uses:

```json
{
  "mcpServers": {
    "gpp": {
      "command": "target/release/gpp",
      "args": ["mcp-server", "--stdio"]
    }
  }
}
```

If you installed via `cargo install`, use `"command": "gpp"` instead of the
`target/release` path.

### Claude Desktop

Claude Desktop doesn't inherit a working directory, so give it an absolute
binary path and pass the repository explicitly via `--repo` — or point
`command` at a wrapper script that `cd`s first. In
`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "gpp": {
      "command": "/home/you/.cargo/bin/gpp",
      "args": ["mcp-server", "--stdio", "--repo", "/home/you/src/yourproject"]
    }
  }
}
```

### Generic stdio client

Any process that speaks newline-delimited JSON-RPC works:

```bash
$ gpp mcp-server --stdio
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"graphex_query","arguments":{"pattern":"auth","budget":4000}}}
```

One request per line; one response per line. Notifications (requests
without an `id`) are accepted and ignored, and `ping` is supported.

## Security notes

- **Local only.** stdio transport, no network listener, no ports. The
  server can only be reached by the client process that spawned it.
- **Tier-gated reads.** Every read is filtered by
  `--trust-tier` (default `agent-readable`). Nodes above the tier —
  `agent-restricted`, `human-only` — are never projected to the agent, in
  any tool.
- **Encrypted at rest.** Graph nodes are individually encrypted
  (AES-256-GCM per node, keys wrapped by the repository's age master key)
  and stored as content-addressed blobs. The SQLite index holds metadata
  only.
- **Audited.** Every projection is recorded in the append-only graph
  access log (accessor, nodes touched, timestamp). Inspect with
  `gpp graphex audit` or fold it into `gpp audit --include-graphex`.
- **No silent writes.** Agent-originated changesets and graph updates land
  as proposals for human review; the MCP path has no way to bypass that.

## What an agent session looks like

1. **Query context.** The agent calls `graphex_query` (plus
   `graphex_conventions` / `graphex_glossary` as needed) and gets a
   tier-filtered projection — including recorded beliefs with their
   current staleness status, so it isn't working from silently outdated
   notes.
2. **Edit normally.** File changes are captured continuously by the
   timeline; no tool calls needed.
3. **Propose.** When a unit of work is done, `propose_changeset` with a
   clear message and intent. The changeset id comes back.
4. **Report cost.** Immediately, `report_cost` with that id and the token
   usage. Skipping this leaves the changeset recorded as free; reports
   accumulate, so calling per-turn is fine.
5. **Contribute knowledge.** Durable facts learned along the way go
   through `propose_graph_update` and wait for human approval.

For the staleness machinery behind step 1 — beliefs, evidence spans, and
`gpp belief bisect` — see [`demos/belief-bisect/`](../demos/belief-bisect/)
and the `gpp belief` section of [`CLI_SPEC.md`](CLI_SPEC.md).
