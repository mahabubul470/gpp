//! History objects: [`Author`], [`Intent`], [`Changeset`].
//!
//! Field layout follows `docs/DATA_MODEL.md`. Phase 1 populates the subset
//! needed for promote/log; later phases fill in cost, signatures, semantic
//! changes, etc. Bodies are canonical MessagePack, hashed with BLAKE3 by the
//! `gpp-core` object store.

use std::collections::BTreeMap;

use gpp_core::{Error as CoreError, Hash, Object, ObjectType, Result as CoreResult};
use serde::{Deserialize, Serialize};

/// Whether a contribution came from a human or an AI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorType {
    Human,
    Agent,
}

/// Who made a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub author_type: AuthorType,
    pub name: String,
    pub identity: String,
    pub agent_meta: Option<Hash>,
}

impl Author {
    pub fn human(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            author_type: AuthorType::Human,
            name: name.into(),
            identity: email.into(),
            agent_meta: None,
        }
    }
}

/// Why a change was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentType {
    HumanDirected,
    AgentProposed,
    PolicyTriggered,
    ReviewResponse,
    BugFix,
    Feature,
    Refactor,
    Documentation,
    Dependency,
}

impl IntentType {
    /// Parse the `--intent` CLI value; unknown/absent → `HumanDirected`.
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "feature" => IntentType::Feature,
            "bugfix" | "bug" => IntentType::BugFix,
            "refactor" => IntentType::Refactor,
            "docs" | "documentation" => IntentType::Documentation,
            "dependency" | "deps" => IntentType::Dependency,
            _ => IntentType::HumanDirected,
        }
    }
}

/// Captures the intent behind a [`Changeset`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub intent_type: IntentType,
    pub description: String,
    pub prompt: Option<String>,
    pub task_reference: Option<String>,
    pub goal: Option<String>,
    pub constraints: Vec<String>,
    pub timestamp: i64,
}

/// The primary unit of curated history (replaces Git's commit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changeset {
    pub parents: Vec<Hash>,
    pub tree: Hash,
    pub timestamp: i64,
    pub author: Author,
    pub committer: Option<Author>,
    pub intent: Option<Hash>,
    pub timeline_range: Option<(i64, i64)>,
    pub metadata: BTreeMap<String, String>,
}

fn encode<T: Serialize>(v: &T) -> CoreResult<Vec<u8>> {
    rmp_serde::to_vec(v).map_err(|e| CoreError::Serialize(e.to_string()))
}

fn decode<T: for<'de> Deserialize<'de>>(b: &[u8]) -> CoreResult<T> {
    rmp_serde::from_slice(b).map_err(|e| CoreError::Deserialize(e.to_string()))
}

impl Object for Intent {
    const TYPE: ObjectType = ObjectType::Intent;
    fn encode_body(&self) -> CoreResult<Vec<u8>> {
        encode(self)
    }
    fn decode_body(bytes: &[u8]) -> CoreResult<Self> {
        decode(bytes)
    }
}

impl Object for Changeset {
    const TYPE: ObjectType = ObjectType::Changeset;
    fn encode_body(&self) -> CoreResult<Vec<u8>> {
        encode(self)
    }
    fn decode_body(bytes: &[u8]) -> CoreResult<Self> {
        decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changeset_roundtrips_through_body() {
        let cs = Changeset {
            parents: vec![Hash::of(b"p")],
            tree: Hash::of(b"t"),
            timestamp: 42,
            author: Author::human("Dev One", "dev1@example.com"),
            committer: None,
            intent: Some(Hash::of(b"i")),
            timeline_range: Some((1, 9)),
            metadata: BTreeMap::new(),
        };
        let body = cs.encode_body().unwrap();
        assert_eq!(Changeset::decode_body(&body).unwrap(), cs);
        // Deterministic id.
        assert_eq!(cs.id().unwrap(), cs.id().unwrap());
    }

    #[test]
    fn intent_type_parsing() {
        assert_eq!(IntentType::parse("bugfix"), IntentType::BugFix);
        assert_eq!(IntentType::parse("DOCS"), IntentType::Documentation);
        assert_eq!(IntentType::parse("whatever"), IntentType::HumanDirected);
    }
}
