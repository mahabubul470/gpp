//! Path-pattern query engine: `<subject> -> <relation> -> <object>`.
//!
//! `*` is a wildcard for subject/relation/object. `--depth N` does multi-hop
//! traversal. Results are metadata-only (names, types, relations) — node
//! *content* is never decrypted here; that is the projection engine's job
//! and is tier-gated.

use gpp_core::Hash;

use crate::error::{Error, Result};
use crate::object::AccessTier;
use crate::store::{GraphStore, NodeMeta};

/// Parsed query pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub subject: String,
    pub relation: String,
    pub object: String,
}

impl Pattern {
    /// Parse `"a -> rel -> b"`. Each side may be `*`.
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split("->").map(|p| p.trim()).collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(Error::BadQuery(format!(
                "expected `<subject> -> <relation> -> <object>`, got {s:?}"
            )));
        }
        Ok(Pattern {
            subject: parts[0].to_string(),
            relation: parts[1].to_string(),
            object: parts[2].to_string(),
        })
    }

    fn rel_filter(&self) -> Option<&str> {
        if self.relation == "*" {
            None
        } else {
            Some(self.relation.as_str())
        }
    }
}

/// Extra query constraints (CLI flags).
#[derive(Debug, Default, Clone)]
pub struct QueryOpts {
    pub depth: usize,
    pub node_type: Option<String>,
    pub max_tier: Option<AccessTier>,
    pub since: Option<i64>,
}

/// One resolved path: the chain of node ids plus the relations between them.
#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub nodes: Vec<Hash>,
    pub relations: Vec<String>,
}

fn meta_ok(m: &NodeMeta, opts: &QueryOpts) -> bool {
    if let Some(t) = &opts.node_type
        && &m.node_type != t
    {
        return false;
    }
    if let Some(maxt) = opts.max_tier
        && m.access_tier > maxt
    {
        return false;
    }
    if let Some(since) = opts.since
        && m.updated_at < since
    {
        return false;
    }
    true
}

/// Execute a pattern. Returns resolved paths (depth-bounded BFS).
pub fn run(store: &GraphStore, pat: &Pattern, opts: &QueryOpts) -> Result<Vec<ResolvedPath>> {
    let depth = opts.depth.max(1);
    let mut out = Vec::new();

    // Direction: forward from subject, or — if subject is `*` and object is
    // concrete — backward from object.
    let backward = pat.subject == "*" && pat.object != "*";

    let seeds: Vec<Hash> = if backward {
        vec![store.node_id_by_name(&pat.object)?]
    } else if pat.subject == "*" {
        store
            .list_nodes(None)?
            .into_iter()
            .filter(|m| meta_ok(m, opts))
            .map(|m| m.id)
            .collect()
    } else {
        vec![store.node_id_by_name(&pat.subject)?]
    };

    for seed in seeds {
        // BFS keeping the path taken.
        let mut frontier: Vec<ResolvedPath> = vec![ResolvedPath {
            nodes: vec![seed],
            relations: Vec::new(),
        }];
        for _ in 0..depth {
            let mut next = Vec::new();
            for path in &frontier {
                let tail = *path.nodes.last().expect("non-empty");
                let steps = if backward {
                    store.predecessors(&tail, pat.rel_filter())?
                } else {
                    store.neighbours(&tail, pat.rel_filter())?
                };
                for (rel, nbr) in steps {
                    if path.nodes.contains(&nbr) {
                        continue; // no cycles
                    }
                    let mut np = path.clone();
                    np.nodes.push(nbr);
                    np.relations.push(rel);
                    // Endpoint filters.
                    let endpoint = store.node_meta(&nbr)?;
                    let keep = match &endpoint {
                        Some(m) => meta_ok(m, opts),
                        None => false,
                    };
                    if keep {
                        let obj_ok = if backward {
                            pat.subject == "*"
                        } else {
                            pat.object == "*"
                                || endpoint.map(|m| m.name == pat.object).unwrap_or(false)
                        };
                        if obj_ok {
                            out.push(np.clone());
                        }
                    }
                    next.push(np);
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_patterns() {
        let p = Pattern::parse("orders -> depends-on -> *").unwrap();
        assert_eq!(p.subject, "orders");
        assert_eq!(p.relation, "depends-on");
        assert_eq!(p.object, "*");
        assert!(Pattern::parse("bad pattern").is_err());
        assert!(Pattern::parse("a -> b").is_err());
    }
}
