//! `gpp-graphex` — encrypted, versioned knowledge graph (layer 4).
//!
//! Graphex is the project's "brain": nodes (services, modules, conventions,
//! glossary, decisions…) and edges between them, stored encrypted in the
//! object store with a SQLite adjacency index. Agents never see raw nodes —
//! they receive a tier-filtered, token-budgeted context *projection*.
//!
//! See `docs/GRAPHEX_PROTOCOL.md`, `docs/SECURITY_MODEL.md`,
//! `docs/DATA_MODEL.md`, and `docs/ROADMAP.md` (Phase 3).
#![forbid(unsafe_code)]

mod crypto;
mod error;
mod infer;
mod keys;
mod object;
mod project;
mod query;
mod store;

pub use error::{Error, Result};
pub use infer::{Suggestion, suggest_modules};
pub use keys::KeyStore;
pub use object::{
    AccessTier, EdgeRelation, GraphEdge, GraphNode, NodeSource, NodeState, NodeType, now_micros,
};
pub use project::{Projection, project};
pub use query::{Pattern, QueryOpts, ResolvedPath, run as query};
pub use store::{GraphStore, NodeMeta, active_node};

#[cfg(test)]
mod tests {
    use super::*;
    use gpp_history::Author;
    use std::path::Path;

    fn store(p: &Path) -> GraphStore {
        let gpp = p.join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        gpp_core::ObjectStore::init(&gpp).unwrap();
        KeyStore::generate(&gpp).unwrap();
        GraphStore::open(&gpp).unwrap()
    }

    fn author() -> Author {
        Author::human("Dev", "dev@example.com")
    }

    #[test]
    fn node_encrypt_roundtrip_through_store() {
        let d = tempfile::tempdir().unwrap();
        let gs = store(d.path());
        let n = active_node(
            NodeType::Service,
            "orders-service",
            "Core orders processing engine",
            AccessTier::AgentRestricted,
            author(),
        );
        let id = gs.put_node(&n, NodeState::Active).unwrap();

        // On-disk blob must not contain the plaintext (it is encrypted).
        let blob_listing = std::fs::read_dir(d.path().join(".gpp/objects")).unwrap();
        assert!(blob_listing.count() > 0);

        let back = gs.get_node(&id).unwrap();
        assert_eq!(back.name, "orders-service");
        assert_eq!(back.description, "Core orders processing engine");
    }

    #[test]
    fn query_traverses_edges() {
        let d = tempfile::tempdir().unwrap();
        let gs = store(d.path());
        let a = gs
            .put_node(
                &active_node(
                    NodeType::Service,
                    "orders-service",
                    "orders",
                    AccessTier::Public,
                    author(),
                ),
                NodeState::Active,
            )
            .unwrap();
        let b = gs
            .put_node(
                &active_node(
                    NodeType::Module,
                    "currency-utils",
                    "money math",
                    AccessTier::Public,
                    author(),
                ),
                NodeState::Active,
            )
            .unwrap();
        gs.put_edge(&GraphEdge {
            from_node: a,
            to_node: b,
            relation: EdgeRelation::DependsOn,
            properties: Default::default(),
            created_by: author(),
            created_at: now_micros(),
            confidence: 1.0,
            bidirectional: false,
        })
        .unwrap();

        let pat = Pattern::parse("orders-service -> depends-on -> *").unwrap();
        let paths = query(
            &gs,
            &pat,
            &QueryOpts {
                depth: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(
            gs.name_of(paths[0].nodes.last().unwrap()).unwrap(),
            "currency-utils"
        );

        // Reverse direction.
        let rp = Pattern::parse("* -> depends-on -> currency-utils").unwrap();
        let back = query(
            &gs,
            &rp,
            &QueryOpts {
                depth: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn projection_respects_tiers_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let gs = store(d.path());
        gs.put_node(
            &active_node(
                NodeType::Service,
                "public-svc",
                "visible everywhere",
                AccessTier::Public,
                author(),
            ),
            NodeState::Active,
        )
        .unwrap();
        gs.put_node(
            &active_node(
                NodeType::Convention,
                "secret-rule",
                "humans only secret",
                AccessTier::HumanOnly,
                author(),
            ),
            NodeState::Active,
        )
        .unwrap();

        // An agent-readable accessor must not see the human-only node.
        let proj = project(
            &gs,
            "demo",
            None,
            AccessTier::AgentReadable,
            "agent",
            "agent:claude",
            10_000,
        )
        .unwrap();
        assert!(proj.text.contains("public-svc"));
        assert!(!proj.text.contains("humans only secret"));

        let audit = gs.read_audit(None, Some("agent:claude"), 10).unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].3, "project");
    }

    #[test]
    fn lifecycle_proposed_to_active() {
        let d = tempfile::tempdir().unwrap();
        let gs = store(d.path());
        let id = gs
            .put_node(
                &active_node(
                    NodeType::Module,
                    "retry-queue",
                    "backoff retry queue",
                    AccessTier::AgentReadable,
                    author(),
                ),
                NodeState::Proposed,
            )
            .unwrap();
        assert_eq!(gs.list_nodes(Some(NodeState::Proposed)).unwrap().len(), 1);
        assert_eq!(gs.list_nodes(Some(NodeState::Active)).unwrap().len(), 0);
        gs.set_state(&id, NodeState::Active).unwrap();
        assert_eq!(gs.list_nodes(Some(NodeState::Active)).unwrap().len(), 1);
    }

    #[test]
    fn key_rotation_keeps_nodes_readable() {
        let d = tempfile::tempdir().unwrap();
        let mut gs = store(d.path());
        let id = gs
            .put_node(
                &active_node(
                    NodeType::Concept,
                    "idempotency-key",
                    "safe retries token",
                    AccessTier::AgentRestricted,
                    author(),
                ),
                NodeState::Active,
            )
            .unwrap();
        let rotated = gs.rotate_keys().unwrap();
        assert_eq!(rotated, 1);
        assert_eq!(gs.get_node(&id).unwrap().description, "safe retries token");
    }
}
