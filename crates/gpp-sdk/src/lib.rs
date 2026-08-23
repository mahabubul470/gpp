//! `gpp-sdk` — the Tier 3 native agent SDK.
//!
//! An [`AgentSession`] gives an AI agent four capabilities, all
//! trust/approval-gated by the layers beneath it:
//!
//! * [`AgentSession::query_graphex`] — pull a tier-filtered context
//!   projection (the agent never sees raw or over-tier nodes).
//! * [`AgentSession::propose_changeset`] — promote timeline entries into a
//!   changeset, attributed to the agent.
//! * [`AgentSession::propose_graph_update`] — propose graph changes; node
//!   additions land as *Proposed* (human-approved), per the Graphex protocol.
//! * [`AgentSession::report_cost`] — attribute real token/compute [`Usage`] to
//!   a changeset, so the cost layer holds actual numbers rather than the zeros
//!   written at promote time.
//! * [`AgentSession::propose_belief`] — record an evidence-anchored belief
//!   about the code. It lands as *Proposed* like any graph update, but is
//!   anchored at HEAD with verified evidence spans from the moment it is
//!   written, so the staleness engine polices it from day one.
//!
//! See `docs/ROADMAP.md` (Phase 3).
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use gpp_core::{Hash, flatten_tree};
use gpp_graphex::{
    AccessTier, GraphEdge, GraphNode, GraphStore, NodeSource, NodeState, NodeType, Pattern, Scope,
    now_micros,
};
use gpp_history::{Author, AuthorType, IntentType, PromoteOptions, RefStore};
use gpp_timeline::Timeline;
use thiserror::Error;

pub use gpp_cost::Usage;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a gpp repository (no .gpp/ found from {0})")]
    NotARepo(String),
    #[error("graphex error: {0}")]
    Graphex(#[from] gpp_graphex::Error),
    #[error("store error: {0}")]
    Core(#[from] gpp_core::Error),
    #[error("history error: {0}")]
    History(#[from] gpp_history::Error),
    #[error("timeline error: {0}")]
    Timeline(String),
    #[error("cost error: {0}")]
    Cost(#[from] gpp_cost::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// A graph change an agent can propose.
#[derive(Debug, Clone)]
pub enum GraphUpdate {
    AddNode {
        node_type: NodeType,
        name: String,
        description: String,
        tier: AccessTier,
        suggested_edges: Vec<(String, String, String)>, // (from, relation, to)
    },
    AddEdge {
        from: String,
        relation: String,
        to: String,
    },
}

/// A native agent's handle on a gpp repository.
pub struct AgentSession {
    gpp_dir: PathBuf,
    root: PathBuf,
    agent_id: String,
    agent_name: String,
    max_tier: AccessTier,
}

fn discover(start: &Path) -> Result<PathBuf> {
    let start = start
        .canonicalize()
        .map_err(|_| Error::NotARepo(start.display().to_string()))?;
    for dir in start.ancestors() {
        if dir.join(".gpp").is_dir() {
            return Ok(dir.to_path_buf());
        }
    }
    Err(Error::NotARepo(start.display().to_string()))
}

impl AgentSession {
    /// Open a session rooted at (or above) `start`. `max_tier` is the highest
    /// tier this agent is permitted to read (normally derived from its trust
    /// score; the trust layer arrives in Phase 4).
    pub fn open(
        start: &Path,
        agent_id: impl Into<String>,
        agent_name: impl Into<String>,
        max_tier: AccessTier,
    ) -> Result<Self> {
        let root = discover(start)?;
        Ok(Self {
            gpp_dir: root.join(".gpp"),
            root,
            agent_id: agent_id.into(),
            agent_name: agent_name.into(),
            max_tier,
        })
    }

    /// Propose an evidence-anchored belief about the code.
    ///
    /// `evidence` entries are `PATH:START-END` (1-based, inclusive) and
    /// `symbols` are `PATH:NAME`; every path must exist in the HEAD tree and
    /// every span must fit its file — the same bar `gpp belief add` holds a
    /// human to. The belief is anchored at HEAD and lands as
    /// [`NodeState::Proposed`]: invisible to projections until a human
    /// approves it, but scanned for staleness from the moment it exists.
    /// Returns `(node id, anchor changeset, parser warnings)`.
    pub fn propose_belief(
        &self,
        claim: &str,
        paths: &[String],
        symbols: &[String],
        evidence: &[String],
        tier: AccessTier,
    ) -> Result<(Hash, Hash, Vec<String>)> {
        let claim = claim.trim();
        if claim.is_empty() {
            return Err(Error::Other("a belief needs a claim".into()));
        }
        if paths.is_empty() && symbols.is_empty() && evidence.is_empty() {
            return Err(Error::Other(
                "a belief needs a scope: at least one path, symbol or evidence span".into(),
            ));
        }
        if tier > self.max_tier {
            return Err(Error::Other(format!(
                "cannot propose at tier {} above this session's {}",
                tier.as_str(),
                self.max_tier.as_str()
            )));
        }
        let refs = RefStore::open(&self.gpp_dir);
        let anchor = refs
            .head_tip()?
            .ok_or_else(|| Error::Other("no changesets yet — promote first".into()))?;
        let gs = GraphStore::open(&self.gpp_dir)?;
        let cs: gpp_history::Changeset = gs.objects().read(&anchor)?;
        let files = flatten_tree(gs.objects(), &cs.tree)?;
        let symbols = symbols
            .iter()
            .map(|s| gpp_graphex::parse_symbol_spec(s))
            .collect::<gpp_graphex::Result<Vec<_>>>()?;
        let warnings = gpp_graphex::verify_symbols(gs.objects(), &files, &symbols)?;
        let evidence = gpp_graphex::verify_evidence(gs.objects(), &files, evidence)?;
        let scope = Scope {
            paths: paths.to_vec(),
            symbols,
        };
        let id = gpp_graphex::add_belief_proposed(
            &gs,
            claim,
            scope,
            anchor,
            cs.timestamp,
            evidence,
            tier,
            self.author(),
            &self.agent_id,
        )?;
        Ok((id, anchor, warnings))
    }

    fn author(&self) -> Author {
        Author {
            author_type: AuthorType::Agent,
            name: self.agent_name.clone(),
            identity: self.agent_id.clone(),
            agent_meta: None,
        }
    }

    fn ignore_patterns(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join(".gppignore"))
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn project_name(&self) -> String {
        self.root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    }

    /// Pull a context projection. `pattern` optionally scopes to a subgraph
    /// (`"orders -> depends-on -> *"`); `None` projects the active graph.
    pub fn query_graphex(&self, pattern: Option<&str>, token_budget: usize) -> Result<String> {
        let gs = GraphStore::open(&self.gpp_dir)?;
        let pat = pattern.map(Pattern::parse).transpose()?;
        let proj = gpp_graphex::project(
            &gs,
            self.project_name().as_str(),
            pat.as_ref(),
            self.max_tier,
            "agent",
            &self.agent_id,
            token_budget,
        )?;
        Ok(proj.text)
    }

    /// Promote unpromoted timeline entries in `[from,to]` into a changeset
    /// attributed to this agent. Returns the changeset hash.
    pub fn propose_changeset(
        &self,
        from: Option<i64>,
        to: Option<i64>,
        message: impl Into<String>,
        intent: IntentType,
    ) -> Result<Hash> {
        let mut tl = Timeline::open(&self.root, self.ignore_patterns())
            .map_err(|e| Error::Timeline(e.to_string()))?;
        let refs = RefStore::open(&self.gpp_dir);
        let outcome = gpp_history::promote(
            &mut tl,
            &refs,
            PromoteOptions {
                from,
                to,
                message: message.into(),
                intent_type: intent,
                task: None,
                author: self.author(),
            },
        )?;
        Ok(outcome.changeset)
    }

    /// Propose a graph update. `AddNode` lands as **Proposed** (a sidecar in
    /// `.gpp/graphex/pending/`); `AddEdge` is applied when both endpoints
    /// exist (edges carry no secret content). Returns the affected id.
    pub fn propose_graph_update(&self, update: GraphUpdate) -> Result<Hash> {
        let gs = GraphStore::open(&self.gpp_dir)?;
        match update {
            GraphUpdate::AddNode {
                node_type,
                name,
                description,
                tier,
                suggested_edges,
            } => {
                let t = now_micros();
                let node = GraphNode {
                    node_type,
                    name: name.clone(),
                    description,
                    access_tier: tier,
                    properties: Default::default(),
                    created_by: self.author(),
                    created_at: t,
                    updated_at: t,
                    confidence: 0.7,
                    validated_at: None,
                    source: NodeSource::AgentProposed {
                        agent_id: self.agent_id.clone(),
                        approved_by: None,
                    },
                    belief: None,
                };
                let id = gs.put_node(&node, NodeState::Proposed)?;
                let payload = serde_json::json!({
                    "proposed_by": self.agent_id,
                    "node": name,
                    "suggested_edges": suggested_edges,
                })
                .to_string();
                gs.write_proposal(&id, &payload)?;
                gs.log_access(
                    "agent",
                    &self.agent_id,
                    "propose_update",
                    &[name],
                    None,
                    Some("add_node"),
                )?;
                Ok(id)
            }
            GraphUpdate::AddEdge { from, relation, to } => {
                let from_id = gs.node_id_by_name(&from)?;
                let to_id = gs.node_id_by_name(&to)?;
                let edge = GraphEdge {
                    from_node: from_id,
                    to_node: to_id,
                    relation: gpp_graphex::EdgeRelation::parse(&relation),
                    properties: Default::default(),
                    created_by: self.author(),
                    created_at: now_micros(),
                    confidence: 0.7,
                    bidirectional: false,
                };
                let id = gs.put_edge(&edge)?;
                gs.log_access(
                    "agent",
                    &self.agent_id,
                    "propose_update",
                    &[from, to],
                    None,
                    Some("add_edge"),
                )?;
                Ok(id)
            }
        }
    }

    /// Attribute real token/compute [`Usage`] to a changeset this agent
    /// produced (normally the hash returned by [`Self::propose_changeset`]).
    ///
    /// Reports accumulate, so an agent whose work spans several turns can call
    /// this repeatedly; the counts sum and `model_id` fills in over the
    /// `"unknown"` placeholder the promote-time recorder wrote. This is the
    /// Tier-3 path that turns the cost layer from structurally-complete into
    /// numerically-real.
    pub fn report_cost(&self, changeset: &Hash, model_id: &str, usage: &Usage) -> Result<()> {
        let cs = gpp_cost::CostStore::open(&self.gpp_dir)?;
        cs.add_usage(&changeset.to_base32(), &self.agent_id, model_id, usage)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(root: &Path) {
        let gpp = root.join(".gpp");
        std::fs::create_dir_all(gpp.join("refs")).unwrap();
        gpp_core::ObjectStore::init(&gpp).unwrap();
        std::fs::write(gpp.join("HEAD"), "ref: refs/main\n").unwrap();
        gpp_graphex::KeyStore::generate(&gpp).unwrap();
    }

    #[test]
    fn changeset_and_graph_proposal_flow() {
        let d = tempfile::tempdir().unwrap();
        init_repo(d.path());
        let sess = AgentSession::open(
            d.path(),
            "agent:claude",
            "Claude",
            AccessTier::AgentReadable,
        )
        .unwrap();

        // Agent edits a file, then proposes a changeset.
        std::fs::write(d.path().join("feature.rs"), "fn f() {}\n").unwrap();
        let cs = sess
            .propose_changeset(None, None, "add feature", IntentType::Feature)
            .unwrap();
        let store = gpp_core::ObjectStore::open(&d.path().join(".gpp"));
        let rec: gpp_history::Changeset = store.read(&cs).unwrap();
        assert_eq!(rec.author.author_type, AuthorType::Agent);

        // Agent proposes a node — it must be Proposed, not Active.
        let id = sess
            .propose_graph_update(GraphUpdate::AddNode {
                node_type: NodeType::Module,
                name: "retry-queue".into(),
                description: "backoff retry queue".into(),
                tier: AccessTier::AgentReadable,
                suggested_edges: vec![],
            })
            .unwrap();
        let gs = GraphStore::open(&d.path().join(".gpp")).unwrap();
        assert_eq!(gs.list_nodes(Some(NodeState::Proposed)).unwrap().len(), 1);
        assert_eq!(gs.list_nodes(Some(NodeState::Active)).unwrap().len(), 0);
        assert!(
            d.path()
                .join(".gpp/graphex/pending")
                .join(format!("{}.proposal", id.to_base32()))
                .exists()
        );

        // Projection is tier-filtered and queryable by the agent.
        let ctx = sess.query_graphex(None, 5_000).unwrap();
        assert!(ctx.contains("Project Context"));

        // Agent reports its token usage for the changeset it proposed.
        sess.report_cost(
            &cs,
            "claude-opus-4-8",
            &Usage {
                input_tokens: 1200,
                output_tokens: 340,
                cost_microdollars: 18_000,
                ..Default::default()
            },
        )
        .unwrap();
        let cost = gpp_cost::CostStore::open(&d.path().join(".gpp")).unwrap();
        let rec = cost.get(&cs.to_base32()).unwrap().expect("cost recorded");
        assert_eq!(rec.input_tokens, 1200);
        assert_eq!(rec.cost_microdollars, 18_000);
        assert_eq!(rec.model_id, "claude-opus-4-8");
        assert_eq!(rec.agent_id, "agent:claude");
    }
    #[test]
    fn propose_belief_lands_proposed_and_stays_proposed_through_scans() {
        let d = tempfile::tempdir().unwrap();
        init_repo(d.path());
        let sess = AgentSession::open(
            d.path(),
            "agent:claude",
            "Claude",
            AccessTier::AgentReadable,
        )
        .unwrap();
        let gpp = d.path().join(".gpp");

        // No changesets yet -> nothing to anchor to.
        let err = sess
            .propose_belief(
                "x",
                &[],
                &[],
                &["a.rs:1-1".into()],
                AccessTier::AgentReadable,
            )
            .unwrap_err();
        assert!(err.to_string().contains("no changesets"));

        std::fs::write(
            d.path().join("auth.rs"),
            "pub const EXPIRY_HOURS: u64 = 24;\npub fn issue() {}\n",
        )
        .unwrap();
        let c0 = sess
            .propose_changeset(None, None, "seed", IntentType::Feature)
            .unwrap();

        // Evidence is verified against the anchor tree, same bar as the CLI.
        let err = sess
            .propose_belief(
                "expiry is 24h",
                &[],
                &[],
                &["auth.rs:9-9".into()],
                AccessTier::AgentReadable,
            )
            .unwrap_err();
        assert!(err.to_string().contains("only 3 line(s)"), "{err}");
        let err = sess
            .propose_belief("no scope", &[], &[], &[], AccessTier::AgentReadable)
            .unwrap_err();
        assert!(err.to_string().contains("needs a scope"));
        let err = sess
            .propose_belief(
                "over tier",
                &[],
                &[],
                &["auth.rs:1-1".into()],
                AccessTier::HumanOnly,
            )
            .unwrap_err();
        assert!(err.to_string().contains("above this session"));

        let (id, anchor, warnings) = sess
            .propose_belief(
                "token expiry is 24h",
                &[],
                &["auth.rs:issue".into()],
                &["auth.rs:1-1".into()],
                AccessTier::AgentReadable,
            )
            .unwrap();
        assert_eq!(anchor, c0);
        assert!(warnings.is_empty(), "{warnings:?}");

        let gs = GraphStore::open(&gpp).unwrap();
        let meta = gs.node_meta(&id).unwrap().unwrap();
        assert_eq!(meta.state, NodeState::Proposed);
        assert!(
            gpp.join("graphex/pending")
                .join(format!("{}.proposal", id.to_base32()))
                .exists()
        );
        let node = gs.get_node(&id).unwrap();
        assert!(matches!(node.source, NodeSource::AgentProposed { .. }));
        let b = node.belief.as_ref().unwrap();
        assert_eq!(b.evidence.len(), 1);
        assert_eq!(b.scope.symbols[0].name, "issue");

        // Proposed beliefs are invisible to projections until approved.
        let ctx = sess.query_graphex(None, 5_000).unwrap();
        assert!(!ctx.contains("token expiry is 24h"));

        // History moves: the evidence line changes. The scan must record the
        // invalidation *without* promoting the node to Active.
        std::fs::write(
            d.path().join("auth.rs"),
            "pub const EXPIRY_HOURS: u64 = 168;\npub fn issue() {}\n",
        )
        .unwrap();
        let c1 = sess
            .propose_changeset(None, None, "raise expiry", IntentType::BugFix)
            .unwrap();
        let (node, hits) = gpp_graphex::scan_and_record(&gs, &id, c1).unwrap();
        let b = node.belief.as_ref().unwrap();
        assert_eq!(b.status, gpp_graphex::BeliefStatus::Invalidated);
        assert_eq!(hits[0].changeset, c1);
        assert_eq!(
            gs.node_meta(&id).unwrap().unwrap().state,
            NodeState::Proposed,
            "a scan must never approve an agent-proposed belief"
        );

        // Once approved, the projection carries the freshness envelope.
        gs.set_state(&id, NodeState::Active).unwrap();
        let ctx = sess.query_graphex(None, 5_000).unwrap();
        assert!(
            ctx.contains("token expiry is 24h ✗ [INVALIDATED at cs:"),
            "{ctx}"
        );
        assert!(ctx.contains(&c1.short()), "{ctx}");
    }
}
