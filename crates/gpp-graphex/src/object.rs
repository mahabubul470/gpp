//! Graphex objects — [`GraphNode`], [`GraphEdge`] and their enums.
//!
//! Field layout follows `docs/DATA_MODEL.md`. Node identity is *stable*:
//! `blake3("{node_type}:{name}")`, so editing a node's description keeps the
//! same id (and the same edges) while producing a new encrypted blob.

use std::collections::BTreeMap;

use gpp_core::Hash;
use gpp_history::Author;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    Service,
    Module,
    Concept,
    Convention,
    ExternalSystem,
    Person,
    Policy,
    Schema,
    Glossary,
    Decision,
    Belief,
}

impl NodeType {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Service => "service",
            NodeType::Module => "module",
            NodeType::Concept => "concept",
            NodeType::Convention => "convention",
            NodeType::ExternalSystem => "external-system",
            NodeType::Person => "person",
            NodeType::Policy => "policy",
            NodeType::Schema => "schema",
            NodeType::Glossary => "glossary",
            NodeType::Decision => "decision",
            NodeType::Belief => "belief",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "service" => NodeType::Service,
            "module" => NodeType::Module,
            "concept" => NodeType::Concept,
            "convention" => NodeType::Convention,
            "external-system" | "external" => NodeType::ExternalSystem,
            "person" => NodeType::Person,
            "policy" => NodeType::Policy,
            "schema" => NodeType::Schema,
            "glossary" => NodeType::Glossary,
            "decision" => NodeType::Decision,
            "belief" => NodeType::Belief,
            other => return Err(Error::UnknownNodeType(other.to_string())),
        })
    }
}

/// Access / encryption tier. Ordered: `Public < AgentReadable <
/// AgentRestricted < HumanOnly`. An accessor with max tier `T` may read any
/// node whose tier is `<= T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AccessTier {
    Public,
    AgentReadable,
    AgentRestricted,
    HumanOnly,
}

impl AccessTier {
    pub fn as_str(self) -> &'static str {
        match self {
            AccessTier::Public => "public",
            AccessTier::AgentReadable => "agent-readable",
            AccessTier::AgentRestricted => "agent-restricted",
            AccessTier::HumanOnly => "human-only",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "public" => AccessTier::Public,
            "agent-readable" | "agent" => AccessTier::AgentReadable,
            "agent-restricted" | "restricted" => AccessTier::AgentRestricted,
            "human-only" | "human" => AccessTier::HumanOnly,
            other => return Err(Error::UnknownTier(other.to_string())),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeSource {
    HumanCreated,
    AgentProposed {
        agent_id: String,
        approved_by: Option<String>,
    },
    AutoInferred {
        from_changeset: Hash,
    },
    Federated {
        source_project: String,
    },
}

/// Lifecycle state (`docs/GRAPHEX_PROTOCOL.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeState {
    Proposed,
    Active,
    Deprecated,
    Archived,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeState::Proposed => "proposed",
            NodeState::Active => "active",
            NodeState::Deprecated => "deprecated",
            NodeState::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "proposed" => NodeState::Proposed,
            "active" => NodeState::Active,
            "deprecated" => NodeState::Deprecated,
            "archived" => NodeState::Archived,
            other => return Err(Error::Other(format!("unknown node state {other:?}"))),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNode {
    pub node_type: NodeType,
    pub name: String,
    pub description: String,
    pub access_tier: AccessTier,
    pub properties: BTreeMap<String, String>,
    pub created_by: Author,
    pub created_at: i64,
    pub updated_at: i64,
    pub confidence: f32,
    pub validated_at: Option<i64>,
    pub source: NodeSource,
    /// Belief payload — only for `NodeType::Belief` nodes. `default` so
    /// blobs written before this field existed still decode.
    #[serde(default)]
    pub belief: Option<crate::belief::BeliefData>,
}

impl GraphNode {
    /// Stable identity: independent of description / timestamps so an edit
    /// re-encrypts the same logical node and keeps its edges.
    pub fn id(&self) -> Hash {
        Hash::of(format!("{}:{}", self.node_type.as_str(), self.name).as_bytes())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        rmp_serde::to_vec_named(self).map_err(|e| Error::Serde(e.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        rmp_serde::from_slice(bytes).map_err(|e| Error::Serde(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeRelation {
    DependsOn,
    CommunicatesWith,
    OwnedBy,
    ImplementsPolicy,
    Contains,
    Uses,
    Contradicts,
    Supersedes,
    RelatedTo,
    FederatedFrom,
    Custom(String),
}

impl EdgeRelation {
    pub fn as_str(&self) -> String {
        match self {
            EdgeRelation::DependsOn => "depends-on".into(),
            EdgeRelation::CommunicatesWith => "communicates-with".into(),
            EdgeRelation::OwnedBy => "owned-by".into(),
            EdgeRelation::ImplementsPolicy => "implements-policy".into(),
            EdgeRelation::Contains => "contains".into(),
            EdgeRelation::Uses => "uses".into(),
            EdgeRelation::Contradicts => "contradicts".into(),
            EdgeRelation::Supersedes => "supersedes".into(),
            EdgeRelation::RelatedTo => "related-to".into(),
            EdgeRelation::FederatedFrom => "federated-from".into(),
            EdgeRelation::Custom(s) => s.clone(),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "depends-on" => EdgeRelation::DependsOn,
            "communicates-with" => EdgeRelation::CommunicatesWith,
            "owned-by" => EdgeRelation::OwnedBy,
            "implements-policy" => EdgeRelation::ImplementsPolicy,
            "contains" => EdgeRelation::Contains,
            "uses" => EdgeRelation::Uses,
            "contradicts" => EdgeRelation::Contradicts,
            "supersedes" => EdgeRelation::Supersedes,
            "related-to" => EdgeRelation::RelatedTo,
            "federated-from" => EdgeRelation::FederatedFrom,
            other => EdgeRelation::Custom(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from_node: Hash,
    pub to_node: Hash,
    pub relation: EdgeRelation,
    pub properties: BTreeMap<String, String>,
    pub created_by: Author,
    pub created_at: i64,
    pub confidence: f32,
    pub bidirectional: bool,
}

impl GraphEdge {
    /// Edge identity: endpoints + relation (so a relation is unique per pair).
    pub fn id(&self) -> Hash {
        Hash::of(
            format!(
                "{}|{}|{}",
                self.from_node.to_base32(),
                self.relation.as_str(),
                self.to_node.to_base32()
            )
            .as_bytes(),
        )
    }
}

/// Wall-clock time in Unix microseconds (gpp's canonical timestamp unit).
pub fn now_micros() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_id_is_stable_across_edits() {
        let mut n = GraphNode {
            node_type: NodeType::Service,
            name: "orders".into(),
            description: "v1".into(),
            access_tier: AccessTier::Public,
            properties: BTreeMap::new(),
            created_by: Author::human("Dev", "d@e.com"),
            created_at: 1,
            updated_at: 1,
            confidence: 1.0,
            validated_at: None,
            source: NodeSource::HumanCreated,
            belief: None,
        };
        let id1 = n.id();
        n.description = "v2 longer text".into();
        n.updated_at = 999;
        assert_eq!(id1, n.id());
    }

    #[test]
    fn node_roundtrips() {
        let n = GraphNode {
            node_type: NodeType::Concept,
            name: "order-batch".into(),
            description: "group of txns".into(),
            access_tier: AccessTier::AgentReadable,
            properties: BTreeMap::from([("k".into(), "v".into())]),
            created_by: Author::human("Dev", "d@e.com"),
            created_at: 5,
            updated_at: 5,
            confidence: 0.9,
            validated_at: Some(7),
            source: NodeSource::HumanCreated,
            belief: None,
        };
        assert_eq!(GraphNode::decode(&n.encode().unwrap()).unwrap(), n);
    }

    #[test]
    fn tier_ordering() {
        assert!(AccessTier::Public < AccessTier::AgentReadable);
        assert!(AccessTier::AgentReadable < AccessTier::HumanOnly);
        assert_eq!(
            AccessTier::parse("agent").unwrap(),
            AccessTier::AgentReadable
        );
    }
}
