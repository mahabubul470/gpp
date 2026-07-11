//! Belief nodes — VCS-native knowledge staleness.
//!
//! A [`BeliefData`] payload rides on a [`GraphNode`](crate::GraphNode) of
//! type `Belief`: a human-readable claim about the code, a *scope* (which
//! paths/symbols the claim is about), an *anchor* changeset (when the belief
//! was formed) and *evidence* spans (why it was believed, with their blob
//! hashes at the anchor).
//!
//! Because Graphex rides the repository's own event stream, staleness is
//! *witnessed*, not detected: every scope-touching change arrives as a
//! changeset with author, time and diff already attached. The scan engine
//! (`crate::stale`) classifies deterministically — no LLM, no network:
//!
//! * scope intersected but evidence spans untouched → [`BeliefStatus::StaleCandidate`]
//!   (the belief may still be true; the code near it moved)
//! * an evidence span's content changed or its file was deleted →
//!   [`BeliefStatus::Invalidated`] at that changeset
//!
//! History is append-only; `belief at <changeset>` time-travel folds it back.

use std::collections::BTreeMap;

use gpp_core::Hash;
use gpp_history::Author;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::object::{AccessTier, GraphNode, NodeSource, NodeState, NodeType, now_micros};
use crate::store::GraphStore;

/// A symbol within a file, resolved via tree-sitter at scan time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRef {
    /// Repo-relative path of the file holding the symbol.
    pub path: String,
    /// Declared identifier (function, struct, class, … name).
    pub name: String,
}

/// What code a belief is about.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Scope {
    /// Glob patterns or exact repo-relative paths.
    pub paths: Vec<String>,
    /// Optional symbol refinement: when a scoped file changes, only diffs
    /// overlapping these symbols' ranges count (reduces over-triggering).
    pub symbols: Vec<SymbolRef>,
}

/// Why a belief was held: a concrete span of code at the anchor changeset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub path: String,
    /// 1-based inclusive line span at the anchor changeset.
    pub span: (usize, usize),
    /// Blob hash of the whole file at the anchor changeset.
    pub blob_hash: Hash,
}

/// Lifecycle of a belief. Honest naming: scope intersection alone never
/// proves a belief false, so it only ever yields `StaleCandidate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeliefStatus {
    Active,
    StaleCandidate,
    Invalidated,
    Reaffirmed,
}

impl BeliefStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BeliefStatus::Active => "active",
            BeliefStatus::StaleCandidate => "stale-candidate",
            BeliefStatus::Invalidated => "invalidated",
            BeliefStatus::Reaffirmed => "reaffirmed",
        }
    }
}

/// What triggered a status change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cause {
    /// A scoped path changed (no evidence span / symbol overlap).
    ScopeTouched { path: String },
    /// An evidence file changed but the evidence span itself did not.
    EvidenceFileTouched { path: String },
    /// The diff overlapped a scoped symbol's declaration range.
    SymbolTouched { path: String, symbol: String },
    /// The diff overlapped an evidence span — the belief's grounds changed.
    EvidenceChanged { path: String, span: (usize, usize) },
    /// An evidence file was deleted.
    EvidenceDeleted { path: String },
    /// A human/agent re-checked the claim and re-anchored it.
    Reaffirmed { by: String },
}

impl Cause {
    pub fn describe(&self) -> String {
        match self {
            Cause::ScopeTouched { path } => format!("scope path {path} changed"),
            Cause::EvidenceFileTouched { path } => {
                format!("evidence file {path} changed (span untouched)")
            }
            Cause::SymbolTouched { path, symbol } => {
                format!("symbol {symbol} in {path} changed")
            }
            Cause::EvidenceChanged { path, span } => {
                format!("evidence {path}:{}-{} changed", span.0, span.1)
            }
            Cause::EvidenceDeleted { path } => format!("evidence file {path} deleted"),
            Cause::Reaffirmed { by } => format!("reaffirmed by {by}"),
        }
    }
}

/// One append-only history entry: at `changeset`, the belief's status became
/// `to` because of `causes`. `at` is the *changeset's* timestamp (not scan
/// wall-time) so replays and time-travel are deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusChange {
    pub changeset: Hash,
    pub at: i64,
    pub to: BeliefStatus,
    pub causes: Vec<Cause>,
}

/// The belief payload carried by a `NodeType::Belief` graph node.
/// The claim text itself lives in the node's `description`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeliefData {
    pub scope: Scope,
    /// Changeset the belief was (last) anchored at.
    pub anchor: Hash,
    pub evidence: Vec<Evidence>,
    pub status: BeliefStatus,
    /// First changeset whose diff changed/deleted an evidence span.
    pub invalidated_by: Option<Hash>,
    /// Append-only. Never rewritten — time-travel depends on this.
    pub history: Vec<StatusChange>,
}

impl BeliefData {
    /// Append a status change if this (changeset, status) transition isn't
    /// already recorded, keeping scans idempotent. Returns true if appended.
    pub fn record(&mut self, change: StatusChange) -> bool {
        if self
            .history
            .iter()
            .any(|h| h.changeset == change.changeset && h.to == change.to)
        {
            return false;
        }
        if change.to == BeliefStatus::Invalidated && self.invalidated_by.is_none() {
            self.invalidated_by = Some(change.changeset);
        }
        self.history.push(change);
        self.recompute_status();
        true
    }

    /// Effective status = the belief's base state overridden by the worst
    /// recorded outcome since its last (re)anchor.
    fn recompute_status(&mut self) {
        let mut status = BeliefStatus::Active;
        for h in &self.history {
            status = match h.to {
                BeliefStatus::Reaffirmed => BeliefStatus::Reaffirmed,
                BeliefStatus::Invalidated => BeliefStatus::Invalidated,
                BeliefStatus::StaleCandidate if status != BeliefStatus::Invalidated => {
                    BeliefStatus::StaleCandidate
                }
                _ => status,
            };
        }
        self.status = status;
    }

    /// The belief's status as it stood at `ancestors` — the set of changesets
    /// reachable from some historical commit. Folds only history entries
    /// witnessed by those changesets. `None` if the belief did not exist yet
    /// (anchor not reachable).
    pub fn status_at(&self, ancestors: &std::collections::HashSet<Hash>) -> Option<BeliefStatus> {
        if !ancestors.contains(&self.anchor) && !self.was_anchored_within(ancestors) {
            return None;
        }
        let mut status = BeliefStatus::Active;
        for h in &self.history {
            if !ancestors.contains(&h.changeset) {
                continue;
            }
            status = match h.to {
                BeliefStatus::Reaffirmed => BeliefStatus::Reaffirmed,
                BeliefStatus::Invalidated => BeliefStatus::Invalidated,
                BeliefStatus::StaleCandidate if status != BeliefStatus::Invalidated => {
                    BeliefStatus::StaleCandidate
                }
                _ => status,
            };
        }
        Some(status)
    }

    /// A reaffirm moves `anchor` forward; the belief still existed at older
    /// commits if any earlier history entry (including the original anchor,
    /// recorded as the first entry) is reachable.
    fn was_anchored_within(&self, ancestors: &std::collections::HashSet<Hash>) -> bool {
        self.history
            .iter()
            .any(|h| ancestors.contains(&h.changeset))
    }
}

/// v2 seam (out of scope for v1): semantic invalidation that judges whether a
/// `StaleCandidate` is *actually* contradicted by the new code. Deliberately
/// a trait so an LLM-backed implementation can plug in without touching the
/// deterministic core; no implementation ships today.
pub trait SemanticInvalidator {
    /// Given the claim and the offending diff excerpt, return `Some(true)`
    /// if the change contradicts the claim, `Some(false)` if it does not,
    /// or `None` when undecidable.
    fn contradicts(&self, claim: &str, diff_excerpt: &str) -> Option<bool>;
}

// ---------------------------------------------------------------------------
// Store operations (thin veneer over GraphStore's node API)
// ---------------------------------------------------------------------------

/// Create a new belief node anchored at `anchor`, returning its node id.
/// The claim is both the node name (identity) and description.
#[allow(clippy::too_many_arguments)]
pub fn add_belief(
    store: &GraphStore,
    claim: &str,
    scope: Scope,
    anchor: Hash,
    anchor_at: i64,
    evidence: Vec<Evidence>,
    tier: AccessTier,
    author: Author,
) -> Result<Hash> {
    let data = BeliefData {
        scope,
        anchor,
        evidence,
        status: BeliefStatus::Active,
        invalidated_by: None,
        history: vec![StatusChange {
            changeset: anchor,
            at: anchor_at,
            to: BeliefStatus::Active,
            causes: vec![],
        }],
    };
    let t = now_micros();
    let node = GraphNode {
        node_type: NodeType::Belief,
        name: claim.to_string(),
        description: claim.to_string(),
        access_tier: tier,
        properties: BTreeMap::new(),
        created_by: author,
        created_at: t,
        updated_at: t,
        confidence: 1.0,
        validated_at: Some(t),
        source: NodeSource::HumanCreated,
        belief: Some(data),
    };
    store.put_node(&node, NodeState::Active)
}

/// Load every belief node (decrypts each; belief counts are small).
pub fn list_beliefs(store: &GraphStore) -> Result<Vec<(Hash, GraphNode)>> {
    let mut out = Vec::new();
    for meta in store.list_nodes(None)? {
        if meta.node_type == NodeType::Belief.as_str() {
            out.push((meta.id, store.get_node(&meta.id)?));
        }
    }
    Ok(out)
}

/// Resolve a belief by full/short node id or exact claim text.
pub fn resolve_belief(store: &GraphStore, spec: &str) -> Result<(Hash, GraphNode)> {
    if let Ok(h) = Hash::from_base32(spec)
        && let Ok(n) = store.get_node(&h)
        && n.node_type == NodeType::Belief
    {
        return Ok((h, n));
    }
    let all = list_beliefs(store)?;
    let matches: Vec<_> = all
        .into_iter()
        .filter(|(id, n)| id.to_base32().starts_with(&spec.to_ascii_lowercase()) || n.name == spec)
        .collect();
    match matches.len() {
        0 => Err(Error::NodeNotFound(spec.to_string())),
        1 => Ok(matches.into_iter().next().expect("len checked")),
        n => Err(Error::Other(format!(
            "belief id {spec:?} is ambiguous ({n} matches) — use more characters"
        ))),
    }
}

/// Persist an updated belief payload (same node id — identity is the claim).
pub fn save_belief(store: &GraphStore, node: &GraphNode) -> Result<Hash> {
    let mut node = node.clone();
    node.updated_at = now_micros();
    store.put_node(&node, NodeState::Active)
}
