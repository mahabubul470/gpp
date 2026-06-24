# Tutorial: Connecting AI agents via MCP

gpp ships an MCP server over stdio. Point your agent tool at it. For
**Claude Code**, drop this in `.mcp.json` at your repo root (or merge it into an
existing one):

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

Other MCP clients use the same `command`/`args`. To raise what the agent may
read, pass a trust tier: `"args": ["mcp-server", "--stdio", "--trust-tier", "raw"]`
(default is `agent-readable`).

On connect, the server returns an `instructions` block that teaches the agent
the gpp workflow, so you don't have to explain it in your prompt.

## The agent workflow

1. **Get context before editing** — `graphex_query` projects the
   knowledge graph (architecture, modules, conventions), tier-filtered to what
   the agent's trust level permits. `graphex_glossary` and
   `graphex_conventions` answer narrower questions.
2. **Edit files normally** — the timeline captures every change; no tool call
   needed.
3. **Propose a changeset** — `propose_changeset` with a `message` and `intent`
   (`feature`/`fix`/`refactor`/`test`/`docs`/`chore`). It returns the
   changeset id.
4. **Report cost** — `report_cost` with that id and your token usage. This is
   how gpp attributes *real* cost; without it the changeset is recorded as
   free. Reports accumulate, so multi-turn work can call it repeatedly.
5. **Propose knowledge** — `propose_graph_update` to suggest a durable fact
   (module, invariant, glossary term). It lands as **Proposed**, never applied
   silently.

Tools exposed: `graphex_query`, `graphex_status`, `graphex_glossary`,
`graphex_conventions`, `propose_changeset`, `propose_graph_update`,
`report_cost`.

All reads are tier-gated by `--trust-tier` (default `agent-readable`).
Agent-proposed nodes require human approval:

```bash
gpp graphex pending
gpp graphex accept <name>      # or: reject
```

## Reporting cost without MCP

Any tool — not just MCP clients — can report usage through the CLI, so a
Tier-1 agent (or a wrapper script) can attribute cost too:

```bash
# After the agent's changeset is promoted (HEAD, a short id, or a full hash):
gpp cost --report HEAD --model claude-opus-4-8 \
    --input 1500 --output 300 --cost-micro 22000
gpp cost --json          # roll-up; reports accumulate onto the record
```

(`--cost-micro` is integer micro-dollars: 1 = $0.000001.)

## Native (Tier 3) agents

Native agents use the Rust `gpp-sdk` directly:

```rust
let sess = AgentSession::open(".", "agent:claude", "Claude", AccessTier::AgentReadable)?;
let ctx = sess.query_graphex(None, 8_000)?;            // context
let cs  = sess.propose_changeset(None, None, "add retry queue", IntentType::Feature)?;
sess.report_cost(&cs, "claude-opus-4-8", &Usage {       // real cost
    input_tokens: 1500, output_tokens: 300, cost_microdollars: 22_000,
    ..Default::default()
})?;
```
