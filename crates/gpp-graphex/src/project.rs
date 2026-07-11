//! Context projection — what an agent actually receives.
//!
//! The projection engine resolves a scope, filters nodes by the accessor's
//! max access tier, decrypts the readable ones, flattens them into the
//! structured summary from `docs/GRAPHEX_PROTOCOL.md`, truncates to a token
//! budget, and writes an audit-log entry. Nodes above the accessor's tier are
//! never decrypted and never appear.

use std::collections::BTreeSet;

use gpp_core::Hash;

use crate::error::Result;
use crate::object::{AccessTier, NodeType};
use crate::query::{Pattern, QueryOpts, run};
use crate::store::GraphStore;

/// Result of a projection.
pub struct Projection {
    pub text: String,
    pub nodes: Vec<String>,
    pub truncated: bool,
    pub projection_hash: String,
}

/// ~4 chars per token is the usual rough estimate.
fn est_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

/// Days→civil date (Howard Hinnant), for "Recent Decisions" headings.
fn ymd(us: i64) -> String {
    let days = us / 86_400_000_000;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Project context for an accessor whose maximum readable tier is `max_tier`.
///
/// `scope` optionally narrows to the subgraph a pattern resolves to;
/// `None` projects the whole active graph. `token_budget` caps output size.
#[allow(clippy::too_many_arguments)]
pub fn project(
    store: &GraphStore,
    project_name: &str,
    scope: Option<&Pattern>,
    max_tier: AccessTier,
    accessor_type: &str,
    accessor_id: &str,
    token_budget: usize,
) -> Result<Projection> {
    // Resolve the candidate node set.
    let mut ids: BTreeSet<Hash> = BTreeSet::new();
    if let Some(pat) = scope {
        let opts = QueryOpts {
            depth: 3,
            max_tier: Some(max_tier),
            ..Default::default()
        };
        for p in run(store, pat, &opts)? {
            ids.extend(p.nodes);
        }
        if let Ok(seed) = store.node_id_by_name(&pat.subject) {
            ids.insert(seed);
        }
    } else {
        for m in store.list_nodes(Some(crate::object::NodeState::Active))? {
            ids.insert(m.id);
        }
    }

    // Decrypt the tier-permitted, active nodes.
    let mut services = Vec::new();
    let mut conventions = Vec::new();
    let mut glossary = Vec::new();
    let mut decisions = Vec::new();
    let mut beliefs = Vec::new();
    let mut accessed = Vec::new();

    for id in &ids {
        let Some(meta) = store.node_meta(id)? else {
            continue;
        };
        if meta.state != crate::object::NodeState::Active {
            continue;
        }
        if meta.access_tier > max_tier {
            continue; // scrubbed: never decrypted, never shown
        }
        let node = store.get_node(id)?;
        accessed.push(meta.name.clone());
        match node.node_type {
            NodeType::Service | NodeType::Module | NodeType::ExternalSystem => {
                let mut edges = Vec::new();
                for (rel, nbr) in store.neighbours(id, None)? {
                    if let Some(nm) = store.node_meta(&nbr)?
                        && nm.access_tier <= max_tier
                    {
                        edges.push(format!("{rel} → {}", nm.name));
                    }
                }
                services.push((node.name, node.description, edges));
            }
            NodeType::Convention => conventions.push(node.description),
            NodeType::Glossary => glossary.push((node.name, node.description)),
            NodeType::Decision => decisions.push((node.created_at, node.description)),
            NodeType::Belief => {
                let status = node
                    .belief
                    .as_ref()
                    .map(|b| b.status)
                    .unwrap_or(crate::belief::BeliefStatus::Active);
                beliefs.push((node.description, status));
            }
            _ => conventions.push(format!("{}: {}", node.name, node.description)),
        }
    }

    // Assemble in protocol order, respecting the token budget.
    let mut text = format!("## Project Context: {project_name}\n");
    let mut truncated = false;
    let push_section = |text: &mut String, truncated: &mut bool, body: String| {
        if *truncated {
            return;
        }
        if est_tokens(&(text.clone() + &body)) > token_budget {
            *truncated = true;
            return;
        }
        text.push_str(&body);
    };

    if !services.is_empty() {
        let mut s = String::from("\n### Architecture\n");
        for (name, desc, edges) in &services {
            s.push_str(&format!("- {name}: {desc}\n"));
            for e in edges {
                s.push_str(&format!("  - {e}\n"));
            }
        }
        push_section(&mut text, &mut truncated, s);
    }
    if !conventions.is_empty() {
        let mut s = String::from("\n### Conventions\n");
        for c in &conventions {
            s.push_str(&format!("- {c}\n"));
        }
        push_section(&mut text, &mut truncated, s);
    }
    if !glossary.is_empty() {
        let mut s = String::from("\n### Glossary\n");
        for (n, d) in &glossary {
            s.push_str(&format!("- {n}: {d}\n"));
        }
        push_section(&mut text, &mut truncated, s);
    }
    if !decisions.is_empty() {
        decisions.sort_by_key(|d| std::cmp::Reverse(d.0));
        let mut s = String::from("\n### Recent Decisions\n");
        for (ts, d) in &decisions {
            s.push_str(&format!("- {}: {d}\n", ymd(*ts)));
        }
        push_section(&mut text, &mut truncated, s);
    }
    if !beliefs.is_empty() {
        let mut s = String::from("\n### Beliefs\n");
        for (claim, status) in &beliefs {
            use crate::belief::BeliefStatus;
            match status {
                BeliefStatus::Active | BeliefStatus::Reaffirmed => {
                    s.push_str(&format!("- {claim}\n"));
                }
                BeliefStatus::StaleCandidate => {
                    s.push_str(&format!("- {claim} ⚠ [stale candidate — re-verify]\n"));
                }
                BeliefStatus::Invalidated => {
                    s.push_str(&format!(
                        "- {claim} ✗ [INVALIDATED — do not rely on this]\n"
                    ));
                }
            }
        }
        push_section(&mut text, &mut truncated, s);
    }
    if truncated {
        text.push_str("\n_(context truncated to fit token budget)_\n");
    }

    let projection_hash = blake3::hash(text.as_bytes()).to_hex()[..16].to_string();
    store.log_access(
        accessor_type,
        accessor_id,
        "project",
        &accessed,
        Some(&projection_hash),
        Some(&format!("max_tier={}", max_tier.as_str())),
    )?;

    Ok(Projection {
        text,
        nodes: accessed,
        truncated,
        projection_hash,
    })
}
