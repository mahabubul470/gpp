//! Minimal MCP server (`gpp mcp-server --stdio`).
//!
//! Speaks JSON-RPC 2.0 over newline-delimited stdio (the MCP stdio
//! transport): `initialize`, `tools/list`, `tools/call`, plus `ping` and
//! notification no-ops. Exposes the Phase 3 Graphex + proposal tools. All
//! reads are tier-gated by `--trust-tier`; proposals go through the SDK so
//! they land as human-approved (`Proposed`) just like any other agent.

use std::io::{BufRead, Write};

use anyhow::Result;
use gpp_graphex::{AccessTier, GraphStore, NodeState, NodeType, Pattern};
use gpp_sdk::{AgentSession, GraphUpdate};
use serde_json::{Value, json};

use crate::repo::Repo;

const PROTOCOL_VERSION: &str = "2025-06-18";
const AGENT_ID: &str = "agent:mcp-client";

pub fn serve_stdio(repo: Repo, max_tier: AccessTier) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut stdout,
                    &json!({"jsonrpc":"2.0","id":Value::Null,
                            "error":{"code":-32700,"message":format!("parse error: {e}")}}),
                )?;
                continue;
            }
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        // Notifications (no id) get no response.
        if id.is_none() {
            continue;
        }
        let id = id.unwrap();

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "gpp", "version": env!("CARGO_PKG_VERSION")},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_specs() })),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                handle_tool_call(&repo, max_tier, &params)
            }
            other => Err(format!("method not found: {other}")),
        };

        let msg = match result {
            Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err(e) => json!({"jsonrpc":"2.0","id":id,
                             "error":{"code":-32601,"message":e}}),
        };
        write_msg(&mut stdout, &msg)?;
    }
    Ok(())
}

fn write_msg(out: &mut impl Write, v: &Value) -> Result<()> {
    out.write_all(serde_json::to_string(v)?.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn tool_specs() -> Vec<Value> {
    let s = |name: &str, desc: &str, props: Value, required: Value| {
        json!({
            "name": name,
            "description": desc,
            "inputSchema": {"type":"object","properties":props,"required":required},
        })
    };
    vec![
        s(
            "graphex_query",
            "Project knowledge-graph context (tier-filtered).",
            json!({"pattern":{"type":"string"},"budget":{"type":"integer"}}),
            json!([]),
        ),
        s(
            "graphex_status",
            "Graph statistics: node/edge counts.",
            json!({}),
            json!([]),
        ),
        s(
            "graphex_glossary",
            "Look up domain glossary terms.",
            json!({"term":{"type":"string"}}),
            json!([]),
        ),
        s(
            "graphex_conventions",
            "List applicable coding conventions.",
            json!({}),
            json!([]),
        ),
        s(
            "propose_changeset",
            "Promote pending timeline entries into a changeset.",
            json!({"message":{"type":"string"},"intent":{"type":"string"}}),
            json!(["message"]),
        ),
        s(
            "propose_graph_update",
            "Propose a new graph node (lands as Proposed for human approval).",
            json!({"node_type":{"type":"string"},"name":{"type":"string"},
                   "description":{"type":"string"},"tier":{"type":"string"}}),
            json!(["node_type", "name", "description"]),
        ),
    ]
}

fn text_result(s: String) -> Value {
    json!({"content":[{"type":"text","text":s}]})
}

fn err_result(s: String) -> Value {
    json!({"content":[{"type":"text","text":s}],"isError":true})
}

fn handle_tool_call(
    repo: &Repo,
    max_tier: AccessTier,
    params: &Value,
) -> std::result::Result<Value, String> {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let a = params.get("arguments").cloned().unwrap_or(json!({}));
    let gpp = repo.gpp_dir();

    let run = || -> anyhow::Result<Value> {
        match name {
            "graphex_status" => {
                let gs = GraphStore::open(&gpp)?;
                let (active, total, edges, _) = gs.stats()?;
                Ok(text_result(format!(
                    "{active} active nodes, {total} total, {edges} edges"
                )))
            }
            "graphex_query" => {
                let gs = GraphStore::open(&gpp)?;
                let pat = a
                    .get("pattern")
                    .and_then(|p| p.as_str())
                    .map(Pattern::parse)
                    .transpose()?;
                let budget = a.get("budget").and_then(|b| b.as_u64()).unwrap_or(8000) as usize;
                let proj = gpp_graphex::project(
                    &gs,
                    &project_name(repo),
                    pat.as_ref(),
                    max_tier,
                    "agent",
                    AGENT_ID,
                    budget,
                )?;
                Ok(text_result(proj.text))
            }
            "graphex_glossary" => {
                let term = a.get("term").and_then(|t| t.as_str()).unwrap_or("");
                Ok(text_result(collect_typed(
                    &gpp,
                    max_tier,
                    NodeType::Glossary,
                    term,
                )?))
            }
            "graphex_conventions" => Ok(text_result(collect_typed(
                &gpp,
                max_tier,
                NodeType::Convention,
                "",
            )?)),
            "propose_changeset" => {
                let msg = a
                    .get("message")
                    .and_then(|m| m.as_str())
                    .ok_or_else(|| anyhow::anyhow!("message is required"))?;
                let intent = a
                    .get("intent")
                    .and_then(|i| i.as_str())
                    .map(gpp_history::IntentType::parse)
                    .unwrap_or(gpp_history::IntentType::AgentProposed);
                let sess = AgentSession::open(&repo.root, AGENT_ID, "MCP Client", max_tier)?;
                let cs = sess.propose_changeset(None, None, msg, intent)?;
                Ok(text_result(format!("changeset cs:{}", cs.short())))
            }
            "propose_graph_update" => {
                let node_type = a
                    .get("node_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("node_type is required"))?;
                let nname = a
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("name is required"))?;
                let desc = a.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let tier = a
                    .get("tier")
                    .and_then(|v| v.as_str())
                    .map(AccessTier::parse)
                    .transpose()?
                    .unwrap_or(AccessTier::AgentReadable);
                let sess = AgentSession::open(&repo.root, AGENT_ID, "MCP Client", max_tier)?;
                let id = sess.propose_graph_update(GraphUpdate::AddNode {
                    node_type: NodeType::parse(node_type)?,
                    name: nname.to_string(),
                    description: desc.to_string(),
                    tier,
                    suggested_edges: vec![],
                })?;
                Ok(text_result(format!(
                    "proposed node {nname:?} (cs:{}) — awaiting human approval",
                    id.short()
                )))
            }
            other => Ok(err_result(format!("unknown tool: {other}"))),
        }
    };

    match run() {
        Ok(v) => Ok(v),
        Err(e) => Ok(err_result(format!("{e:#}"))),
    }
}

fn project_name(repo: &Repo) -> String {
    repo.root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into())
}

/// Collect tier-permitted, active nodes of a type, optionally name-filtered.
fn collect_typed(
    gpp: &std::path::Path,
    max_tier: AccessTier,
    want: NodeType,
    needle: &str,
) -> anyhow::Result<String> {
    let gs = GraphStore::open(gpp)?;
    let mut out = String::new();
    for m in gs.list_nodes(Some(NodeState::Active))? {
        if m.access_tier > max_tier {
            continue;
        }
        if !needle.is_empty() && !m.name.to_lowercase().contains(&needle.to_lowercase()) {
            continue;
        }
        let n = gs.get_node(&m.id)?;
        if n.node_type != want {
            continue;
        }
        out.push_str(&format!("- {}: {}\n", n.name, n.description));
    }
    if out.is_empty() {
        out.push_str("(none)");
    }
    Ok(out)
}
