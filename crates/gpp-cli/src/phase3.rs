//! Phase 3 commands: `gpp keys`, `gpp graphex`, `gpp mcp-server`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use gpp_core::{EntryKind, Hash, ObjectStore, Tree};
use gpp_graphex::{
    AccessTier, GraphEdge, GraphNode, GraphStore, KeyStore, NodeSource, NodeState, NodeType,
    Pattern, QueryOpts, now_micros,
};
use gpp_history::{Author, RefStore};

use crate::cli::{
    FederationAction, GraphexAction, GraphexArgs, KeysAction, KeysArgs, McpServerArgs,
};
use crate::config;
use crate::phase1::parse_time;
use crate::repo::Repo;

fn discover(repo_override: Option<&Path>) -> Result<Repo> {
    let start = match repo_override {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?,
    };
    Repo::discover(&start)
}

pub(crate) fn config_author(repo: &Repo) -> Author {
    let _ = repo;
    if let Ok(path) = config::global_config_path() {
        let doc = config::load_doc(&path).unwrap_or(toml::Value::Table(Default::default()));
        let name = config::get_key(&doc, "user.name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();
        let email = config::get_key(&doc, "user.email")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown@localhost")
            .to_string();
        Author::human(name, email)
    } else {
        Author::human("Unknown", "unknown@localhost")
    }
}

// ---------------------------------------------------------------------------
// gpp keys
// ---------------------------------------------------------------------------

pub fn keys(args: &KeysArgs, repo_override: Option<&Path>) -> Result<()> {
    let repo = discover(repo_override)?;
    let gpp = repo.gpp_dir();
    match &args.action {
        KeysAction::Generate => {
            let ks = KeyStore::generate(&gpp).context("generating key store")?;
            println!("Generated Graphex key hierarchy in {}", gpp.display());
            println!("  master recipient: {}", ks.master_recipient());
            println!(
                "  tier keys: {}",
                ks.present_tiers()
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if ks.passphrase_protected() {
                println!(
                    "  master key: passphrase-wrapped; human-only tier is \
                     passphrase-gated (keep ${} safe — it cannot be recovered)",
                    gpp_graphex::PASSPHRASE_ENV
                );
            } else {
                println!(
                    "  master key: stored unwrapped (set ${} before generate \
                     to passphrase-protect at rest)",
                    gpp_graphex::PASSPHRASE_ENV
                );
            }
            Ok(())
        }
        KeysAction::Rotate => {
            let mut gs = GraphStore::open(&gpp).context("opening Graphex")?;
            let n = gs.rotate_keys().context("rotating keys")?;
            println!("Rotated tier keys and re-encrypted {n} node(s)");
            Ok(())
        }
        KeysAction::Show => {
            if !KeyStore::exists(&gpp) {
                bail!("no key store — run `gpp keys generate`");
            }
            let protected = KeyStore::is_passphrase_protected(&gpp);
            let ks = KeyStore::open(&gpp)?;
            println!("master recipient: {}", ks.master_recipient());
            println!(
                "master key:       {}",
                if protected {
                    "passphrase-wrapped"
                } else {
                    "unwrapped (set $GPP_GRAPHEX_PASSPHRASE to protect)"
                }
            );
            for t in ks.present_tiers() {
                let how = match (t.as_str(), protected) {
                    ("public", _) => "plaintext",
                    ("human-only", true) => "encrypted (passphrase-gated)",
                    _ => "encrypted (master-sealed)",
                };
                println!("  {:<18} {how}", t.as_str());
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// gpp graphex
// ---------------------------------------------------------------------------

pub(crate) fn open_graph(repo: &Repo) -> Result<GraphStore> {
    GraphStore::open(&repo.gpp_dir())
        .map_err(|e| anyhow!("{e} (run `gpp keys generate` or `gpp init --graphex`)"))
}

pub fn graphex(args: &GraphexArgs, repo_override: Option<&Path>, json: bool) -> Result<()> {
    let repo = discover(repo_override)?;

    match &args.action {
        GraphexAction::Status => {
            let gs = open_graph(&repo)?;
            let (active, total, edges, last) = gs.stats()?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "active_nodes": active, "total_nodes": total,
                        "edges": edges, "last_update_us": last
                    })
                );
            } else {
                println!("Graphex: {active} active node(s), {total} total, {edges} edge(s)");
                if last > 0 {
                    println!("  last update: {}us", last);
                }
            }
            Ok(())
        }

        GraphexAction::Query {
            pattern,
            depth,
            node_type,
            tier,
            since,
            format,
        } => {
            let gs = open_graph(&repo)?;
            let pat = Pattern::parse(pattern)?;
            let opts = QueryOpts {
                depth: *depth,
                node_type: node_type.clone(),
                max_tier: tier.as_deref().map(AccessTier::parse).transpose()?,
                since: since.as_deref().map(parse_time).transpose()?,
            };
            let paths = gpp_graphex::query(&gs, &pat, &opts)?;
            if format == "json" || json {
                let arr: Vec<_> = paths
                    .iter()
                    .map(|p| {
                        let names: Vec<String> = p
                            .nodes
                            .iter()
                            .map(|n| gs.name_of(n).unwrap_or_else(|_| n.short()))
                            .collect();
                        serde_json::json!({ "path": names, "relations": p.relations })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!(arr))?);
            } else if paths.is_empty() {
                println!("(no matches)");
            } else {
                for p in &paths {
                    let mut line = String::new();
                    for (i, n) in p.nodes.iter().enumerate() {
                        if i > 0 {
                            line.push_str(&format!(" -[{}]-> ", p.relations[i - 1]));
                        }
                        line.push_str(&gs.name_of(n).unwrap_or_else(|_| n.short()));
                    }
                    println!("{line}");
                }
            }
            Ok(())
        }

        GraphexAction::Project {
            pattern,
            tier,
            budget,
        } => {
            let gs = open_graph(&repo)?;
            let pat = pattern.as_deref().map(Pattern::parse).transpose()?;
            let proj = gpp_graphex::project(
                &gs,
                &project_name(&repo),
                pat.as_ref(),
                AccessTier::parse(tier)?,
                "human",
                "cli",
                *budget,
            )?;
            print!("{}", proj.text);
            Ok(())
        }

        GraphexAction::Add {
            node_type,
            name,
            description,
            tier,
            properties,
        } => {
            let gs = open_graph(&repo)?;
            let mut props = BTreeMap::new();
            for kv in properties {
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| anyhow!("property {kv:?} must be key=value"))?;
                props.insert(k.to_string(), v.to_string());
            }
            let tier = match tier {
                Some(t) => AccessTier::parse(t)?,
                None => default_tier(&repo),
            };
            let t = now_micros();
            let node = GraphNode {
                node_type: NodeType::parse(node_type)?,
                name: name.clone(),
                description: description.clone(),
                access_tier: tier,
                properties: props,
                created_by: config_author(&repo),
                created_at: t,
                updated_at: t,
                confidence: 1.0,
                validated_at: Some(t),
                source: NodeSource::HumanCreated,
                belief: None,
            };
            let id = gs.put_node(&node, NodeState::Active)?;
            println!("Added {} node {name:?} (cs:{})", node_type, id.short());
            Ok(())
        }

        GraphexAction::Link {
            from,
            relation,
            to,
            bidirectional,
        } => {
            let gs = open_graph(&repo)?;
            let from_id = gs.node_id_by_name(from)?;
            let to_id = gs.node_id_by_name(to)?;
            let edge = GraphEdge {
                from_node: from_id,
                to_node: to_id,
                relation: gpp_graphex::EdgeRelation::parse(relation),
                properties: Default::default(),
                created_by: config_author(&repo),
                created_at: now_micros(),
                confidence: 1.0,
                bidirectional: *bidirectional,
            };
            gs.put_edge(&edge)?;
            println!("Linked {from} -[{relation}]-> {to}");
            Ok(())
        }

        GraphexAction::Show { node } => {
            let gs = open_graph(&repo)?;
            let id = gs.node_id_by_name(node)?;
            let n = gs.get_node(&id)?;
            println!("node {} (id cs:{})", n.name, id.short());
            println!("  type:       {}", n.node_type.as_str());
            println!("  tier:       {}", n.access_tier.as_str());
            println!("  confidence: {:.2}", n.confidence);
            println!("  source:     {:?}", n.source);
            println!("  description:\n    {}", n.description);
            if !n.properties.is_empty() {
                println!("  properties:");
                for (k, v) in &n.properties {
                    println!("    {k} = {v}");
                }
            }
            for (rel, nbr) in gs.neighbours(&id, None)? {
                println!(
                    "  -[{rel}]-> {}",
                    gs.name_of(&nbr).unwrap_or_else(|_| nbr.short())
                );
            }
            Ok(())
        }

        GraphexAction::List { state } => {
            let gs = open_graph(&repo)?;
            let st = state.as_deref().map(NodeState::parse).transpose()?;
            let nodes = gs.list_nodes(st)?;
            if nodes.is_empty() {
                println!("(no nodes)");
            }
            for m in nodes {
                println!(
                    "{:<10} {:<14} {:<22} {} cs:{}",
                    m.state.as_str(),
                    m.node_type,
                    m.name,
                    m.access_tier.as_str(),
                    m.id.short()
                );
            }
            Ok(())
        }

        GraphexAction::Pending => {
            let gs = open_graph(&repo)?;
            let pend = gs.list_nodes(Some(NodeState::Proposed))?;
            if pend.is_empty() {
                println!("(no pending proposals)");
            }
            for m in pend {
                println!(
                    "  + [{}] {}  (tier {}, cs:{})",
                    m.node_type,
                    m.name,
                    m.access_tier.as_str(),
                    m.id.short()
                );
            }
            println!("\nApprove with `gpp graphex accept <name>` / reject with `reject`.");
            Ok(())
        }

        GraphexAction::Accept { node } => {
            let gs = open_graph(&repo)?;
            let id = gs.node_id_by_name(node)?;
            gs.set_state(&id, NodeState::Active)?;
            gs.clear_proposal(&id)?;
            println!("Accepted {node:?} → Active");
            Ok(())
        }

        GraphexAction::Reject { node } => {
            let gs = open_graph(&repo)?;
            let id = gs.node_id_by_name(node)?;
            gs.set_state(&id, NodeState::Archived)?;
            gs.clear_proposal(&id)?;
            println!("Rejected {node:?} → Archived");
            Ok(())
        }

        GraphexAction::Audit {
            since,
            accessor,
            limit,
        } => {
            let gs = open_graph(&repo)?;
            let since = since.as_deref().map(parse_time).transpose()?;
            let rows = gs.read_audit(since, accessor.as_deref(), *limit)?;
            if rows.is_empty() {
                println!("(no audit entries)");
            }
            for (ts, atype, aid, action, nodes) in rows {
                println!("{ts}us  {atype:<6} {aid:<22} {action:<14} {nodes}");
            }
            Ok(())
        }

        GraphexAction::Infer => {
            let gs = open_graph(&repo)?;
            let changed = head_changed_paths(&repo)?;
            let existing: BTreeSet<String> =
                gs.list_nodes(None)?.into_iter().map(|m| m.name).collect();
            let suggestions = gpp_graphex::suggest_modules(&changed, &existing);
            if suggestions.is_empty() {
                println!("No new modules inferred from HEAD.");
                return Ok(());
            }
            let from = RefStore::open(&repo.gpp_dir())
                .head_tip()?
                .ok_or_else(|| anyhow!("no changesets yet"))?;
            for s in &suggestions {
                let t = now_micros();
                let node = GraphNode {
                    node_type: NodeType::Module,
                    name: s.name.clone(),
                    description: s.reason.clone(),
                    access_tier: default_tier(&repo),
                    properties: Default::default(),
                    created_by: config_author(&repo),
                    created_at: t,
                    updated_at: t,
                    confidence: 0.5,
                    validated_at: None,
                    source: NodeSource::AutoInferred {
                        from_changeset: from,
                    },
                    belief: None,
                };
                let id = gs.put_node(&node, NodeState::Proposed)?;
                gs.write_proposal(&id, &format!("auto-inferred: {}", s.reason))?;
                println!("  proposed module {:?} (cs:{})", s.name, id.short());
            }
            println!("\nReview with `gpp graphex pending`.");
            Ok(())
        }

        GraphexAction::Federation { action } => {
            let fed_dir = repo.gpp_dir().join("graphex").join("federation");
            std::fs::create_dir_all(&fed_dir)?;
            let sources = fed_dir.join("sources.toml");
            match action {
                FederationAction::Add {
                    project,
                    address,
                    subgraph,
                } => {
                    let mut body = std::fs::read_to_string(&sources).unwrap_or_default();
                    body.push_str(&format!(
                        "[[source]]\nproject = {project:?}\naddress = {address:?}\nsubgraph = {subgraph:?}\n\n"
                    ));
                    std::fs::write(&sources, body)?;
                    println!(
                        "federated source added: {project} ({subgraph}) @ {address}\n\
                         pull it with `gpp sync --graph-only` once added as a peer"
                    );
                    Ok(())
                }
                FederationAction::List => {
                    match std::fs::read_to_string(&sources) {
                        Ok(b) if !b.trim().is_empty() => print!("{b}"),
                        _ => println!("(no federated sources)"),
                    }
                    Ok(())
                }
            }
        }
    }
}

fn project_name(repo: &Repo) -> String {
    repo.root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into())
}

pub(crate) fn default_tier(repo: &Repo) -> AccessTier {
    config::load_doc(&repo.config_path())
        .ok()
        .and_then(|d| {
            config::get_key(&d, "graphex.default_access_tier")
                .and_then(|v| v.as_str().map(str::to_string))
        })
        .and_then(|s| AccessTier::parse(&s).ok())
        .unwrap_or(AccessTier::AgentReadable)
}

/// Files whose blob differs between HEAD and its first parent (all files if
/// HEAD is the root changeset).
fn head_changed_paths(repo: &Repo) -> Result<Vec<String>> {
    let refs = RefStore::open(&repo.gpp_dir());
    match refs.head_tip()? {
        Some(tip) => changed_paths(repo, &tip),
        None => Ok(Vec::new()),
    }
}

/// Paths that differ between `changeset` and its first parent (added or
/// modified). Used by inference and by Graphex-driven reviewer assignment.
pub(crate) fn changed_paths(repo: &Repo, changeset: &Hash) -> Result<Vec<String>> {
    let store = ObjectStore::open(&repo.gpp_dir());
    let cs: gpp_history::Changeset = store.read(changeset)?;
    let new = flatten(&store, &cs.tree)?;
    let old = match cs.parents.first() {
        Some(p) => {
            let pc: gpp_history::Changeset = store.read(p)?;
            flatten(&store, &pc.tree)?
        }
        None => BTreeMap::new(),
    };
    Ok(new
        .iter()
        .filter(|(k, v)| old.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect())
}

fn flatten(store: &ObjectStore, root: &Hash) -> Result<BTreeMap<String, Hash>> {
    fn walk(
        store: &ObjectStore,
        h: &Hash,
        prefix: &str,
        out: &mut BTreeMap<String, Hash>,
    ) -> Result<()> {
        let tree: Tree = store.read(h)?;
        for e in tree.entries {
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            match e.kind {
                EntryKind::Directory => walk(store, &e.hash, &path, out)?,
                EntryKind::File | EntryKind::Symlink => {
                    out.insert(path, e.hash);
                }
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(store, root, "", &mut out)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// gpp mcp-server
// ---------------------------------------------------------------------------

pub fn mcp_server(args: &McpServerArgs, repo_override: Option<&Path>) -> Result<()> {
    if !args.stdio || args.port.is_some() {
        bail!("only --stdio transport is implemented (TCP transport is a later phase)");
    }
    let repo = discover(repo_override)?;
    let max_tier =
        AccessTier::parse(&args.trust_tier).map_err(|e| anyhow!("bad --trust-tier: {e}"))?;
    crate::mcp::serve_stdio(repo, max_tier)
}
