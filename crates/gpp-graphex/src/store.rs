//! The Graphex store: object-store-backed encrypted nodes + a SQLite
//! adjacency/metadata index + lifecycle + the access audit log.
//!
//! Node *content* lives in `.gpp/objects/` as an encrypted [`crate::crypto`]
//! envelope (a plain gpp-core `Blob`); `graph.db` only holds the metadata
//! needed for queries plus a pointer to the current blob. Editing a node
//! writes a new blob and re-points the row — old blobs stay in history.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpp_core::{Blob, Hash, ObjectStore};
use rusqlite::{Connection, OptionalExtension, params};

use crate::crypto::{open as open_env, seal};
use crate::error::{Error, Result};
use crate::keys::KeyStore;
use crate::object::{AccessTier, GraphEdge, GraphNode, NodeState, now_micros};

pub struct GraphStore {
    conn: Connection,
    objects: ObjectStore,
    keys: KeyStore,
    gpp_dir: PathBuf,
}

/// One audit-log row: `(timestamp_us, accessor_type, accessor_id, action,
/// nodes_accessed_json)`.
pub type AuditRow = (i64, String, String, String, String);

/// A node's index row (cheap to list without decrypting content).
#[derive(Debug, Clone)]
pub struct NodeMeta {
    pub id: Hash,
    pub node_type: String,
    pub name: String,
    pub access_tier: AccessTier,
    pub state: NodeState,
    pub created_at: i64,
    pub updated_at: i64,
    pub confidence: f32,
}

fn schema() -> &'static str {
    "PRAGMA journal_mode=WAL;
     CREATE TABLE IF NOT EXISTS graph_nodes (
        hash        TEXT PRIMARY KEY,
        node_type   TEXT NOT NULL,
        name        TEXT NOT NULL,
        access_tier TEXT NOT NULL DEFAULT 'public',
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        confidence  REAL NOT NULL DEFAULT 1.0,
        state       TEXT NOT NULL DEFAULT 'active',
        blob        TEXT NOT NULL
     );
     CREATE INDEX IF NOT EXISTS idx_graph_nodes_type ON graph_nodes(node_type);
     CREATE INDEX IF NOT EXISTS idx_graph_nodes_name ON graph_nodes(name);
     CREATE TABLE IF NOT EXISTS graph_edges (
        hash        TEXT PRIMARY KEY,
        from_node   TEXT NOT NULL,
        to_node     TEXT NOT NULL,
        relation    TEXT NOT NULL,
        bidirectional INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL,
        confidence  REAL NOT NULL DEFAULT 1.0,
        meta        TEXT
     );
     CREATE INDEX IF NOT EXISTS idx_graph_edges_from ON graph_edges(from_node);
     CREATE INDEX IF NOT EXISTS idx_graph_edges_to ON graph_edges(to_node);
     CREATE INDEX IF NOT EXISTS idx_graph_edges_relation ON graph_edges(relation);
     CREATE TABLE IF NOT EXISTS graph_access_log (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        timestamp   INTEGER NOT NULL,
        accessor_type TEXT NOT NULL,
        accessor_id TEXT NOT NULL,
        action      TEXT NOT NULL,
        nodes_accessed TEXT NOT NULL,
        projection_hash TEXT,
        details     TEXT
     );
     CREATE INDEX IF NOT EXISTS idx_graph_access_time ON graph_access_log(timestamp);"
}

impl GraphStore {
    /// Open Graphex for the repo whose `.gpp/` dir is `gpp_dir`.
    /// Requires a key store (`gpp keys generate`).
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let keys = KeyStore::open(gpp_dir)?;
        let graphex = gpp_dir.join("graphex");
        std::fs::create_dir_all(graphex.join("pending"))?;
        let conn = Connection::open(graphex.join("graph.db"))?;
        conn.execute_batch(schema())?;
        Ok(Self {
            conn,
            objects: ObjectStore::open(gpp_dir),
            keys,
            gpp_dir: gpp_dir.to_path_buf(),
        })
    }

    // ---- nodes -----------------------------------------------------------

    /// Insert or replace a node, sealing its content for its tier.
    pub fn put_node(&self, node: &GraphNode, state: NodeState) -> Result<Hash> {
        let id = node.id();
        let tier = node.access_tier;
        let key = self.keys.tier_key(tier)?;
        let envelope = seal(&node.encode()?, &key, KeyStore::is_encrypted(tier))?;
        let blob = self.objects.write(&Blob::new(envelope))?;

        let existing_created: Option<i64> = self
            .conn
            .query_row(
                "SELECT created_at FROM graph_nodes WHERE hash = ?1",
                [id.to_base32()],
                |r| r.get(0),
            )
            .optional()?;
        let created_at = existing_created.unwrap_or(node.created_at);

        self.conn.execute(
            "INSERT INTO graph_nodes
                (hash, node_type, name, access_tier, created_at, updated_at, confidence, state, blob)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(hash) DO UPDATE SET
                node_type=excluded.node_type, name=excluded.name,
                access_tier=excluded.access_tier, updated_at=excluded.updated_at,
                confidence=excluded.confidence, state=excluded.state, blob=excluded.blob",
            params![
                id.to_base32(),
                node.node_type.as_str(),
                node.name,
                tier.as_str(),
                created_at,
                node.updated_at,
                node.confidence as f64,
                state.as_str(),
                blob.to_base32(),
            ],
        )?;
        Ok(id)
    }

    /// Decrypt and decode a node by id.
    pub fn get_node(&self, id: &Hash) -> Result<GraphNode> {
        let (tier_s, blob_s): (String, String) = self
            .conn
            .query_row(
                "SELECT access_tier, blob FROM graph_nodes WHERE hash = ?1",
                [id.to_base32()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::NodeNotFound(id.short()))?;
        let tier = AccessTier::parse(&tier_s)?;
        let blob_hash =
            Hash::from_base32(&blob_s).map_err(|e| Error::Other(format!("bad blob ref: {e}")))?;
        let envelope = self.objects.read::<Blob>(&blob_hash)?.content;
        let key = self.keys.tier_key(tier)?;
        GraphNode::decode(&open_env(&envelope, &key)?)
    }

    pub fn node_id_by_name(&self, name: &str) -> Result<Hash> {
        let s: Option<String> = self
            .conn
            .query_row(
                "SELECT hash FROM graph_nodes WHERE name = ?1 ORDER BY updated_at DESC LIMIT 1",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        s.and_then(|s| Hash::from_base32(&s).ok())
            .ok_or_else(|| Error::NodeNotFound(name.to_string()))
    }

    /// List node metadata, optionally filtered by state.
    pub fn list_nodes(&self, state: Option<NodeState>) -> Result<Vec<NodeMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT hash, node_type, name, access_tier, state, created_at, updated_at, confidence
             FROM graph_nodes
             WHERE (?1 IS NULL OR state = ?1)
             ORDER BY name",
        )?;
        let rows = stmt.query_map([state.map(|s| s.as_str())], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, f64>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (h, ty, name, tier, state, ca, ua, conf) = row?;
            out.push(NodeMeta {
                id: Hash::from_base32(&h).map_err(|e| Error::Other(e.to_string()))?,
                node_type: ty,
                name,
                access_tier: AccessTier::parse(&tier)?,
                state: NodeState::parse(&state)?,
                created_at: ca,
                updated_at: ua,
                confidence: conf as f32,
            });
        }
        Ok(out)
    }

    pub fn set_state(&self, id: &Hash, state: NodeState) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE graph_nodes SET state=?1, updated_at=?2 WHERE hash=?3",
            params![state.as_str(), now_micros(), id.to_base32()],
        )?;
        if n == 0 {
            return Err(Error::NodeNotFound(id.short()));
        }
        Ok(())
    }

    // ---- edges -----------------------------------------------------------

    pub fn put_edge(&self, edge: &GraphEdge) -> Result<Hash> {
        let id = edge.id();
        let meta = serde_json::json!({
            "created_by": edge.created_by,
            "properties": edge.properties,
        })
        .to_string();
        self.conn.execute(
            "INSERT OR REPLACE INTO graph_edges
                (hash, from_node, to_node, relation, bidirectional, created_at, confidence, meta)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                id.to_base32(),
                edge.from_node.to_base32(),
                edge.to_node.to_base32(),
                edge.relation.as_str(),
                edge.bidirectional as i64,
                edge.created_at,
                edge.confidence as f64,
                meta,
            ],
        )?;
        Ok(id)
    }

    /// Outgoing (and bidirectional incoming) neighbours of a node:
    /// `(relation, neighbour_id)`.
    pub fn neighbours(&self, id: &Hash, relation: Option<&str>) -> Result<Vec<(String, Hash)>> {
        let key = id.to_base32();
        let mut stmt = self.conn.prepare(
            "SELECT relation, to_node FROM graph_edges
                 WHERE from_node = ?1 AND (?2 IS NULL OR relation = ?2)
             UNION
             SELECT relation, from_node FROM graph_edges
                 WHERE to_node = ?1 AND bidirectional = 1 AND (?2 IS NULL OR relation = ?2)",
        )?;
        let rows = stmt.query_map(params![key, relation], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (rel, to) = row?;
            if let Ok(h) = Hash::from_base32(&to) {
                out.push((rel, h));
            }
        }
        Ok(out)
    }

    /// Nodes that point *at* `id` (for `* -> rel -> node` queries).
    pub fn predecessors(&self, id: &Hash, relation: Option<&str>) -> Result<Vec<(String, Hash)>> {
        let mut stmt = self.conn.prepare(
            "SELECT relation, from_node FROM graph_edges
             WHERE to_node = ?1 AND (?2 IS NULL OR relation = ?2)",
        )?;
        let rows = stmt.query_map(params![id.to_base32(), relation], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (rel, from) = row?;
            if let Ok(h) = Hash::from_base32(&from) {
                out.push((rel, h));
            }
        }
        Ok(out)
    }

    /// Index metadata for one node (no decryption).
    pub fn node_meta(&self, id: &Hash) -> Result<Option<NodeMeta>> {
        self.conn
            .query_row(
                "SELECT hash, node_type, name, access_tier, state, created_at, updated_at, confidence
                 FROM graph_nodes WHERE hash = ?1",
                [id.to_base32()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, f64>(7)?,
                    ))
                },
            )
            .optional()?
            .map(|(h, ty, name, tier, state, ca, ua, conf)| {
                Ok(NodeMeta {
                    id: Hash::from_base32(&h).map_err(|e| Error::Other(e.to_string()))?,
                    node_type: ty,
                    name,
                    access_tier: AccessTier::parse(&tier)?,
                    state: NodeState::parse(&state)?,
                    created_at: ca,
                    updated_at: ua,
                    confidence: conf as f32,
                })
            })
            .transpose()
    }

    pub fn name_of(&self, id: &Hash) -> Result<String> {
        self.conn
            .query_row(
                "SELECT name FROM graph_nodes WHERE hash=?1",
                [id.to_base32()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NodeNotFound(id.short()))
    }

    // ---- proposals / lifecycle ------------------------------------------

    /// Record a pending proposal sidecar (so `graphex pending` can show it).
    pub fn write_proposal(&self, id: &Hash, payload: &str) -> Result<()> {
        let p = self
            .gpp_dir
            .join("graphex")
            .join("pending")
            .join(format!("{}.proposal", id.to_base32()));
        std::fs::write(p, payload)?;
        Ok(())
    }

    pub fn clear_proposal(&self, id: &Hash) -> Result<()> {
        let p = self
            .gpp_dir
            .join("graphex")
            .join("pending")
            .join(format!("{}.proposal", id.to_base32()));
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }

    // ---- audit -----------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn log_access(
        &self,
        accessor_type: &str,
        accessor_id: &str,
        action: &str,
        nodes: &[String],
        projection_hash: Option<&str>,
        details: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO graph_access_log
                (timestamp, accessor_type, accessor_id, action, nodes_accessed, projection_hash, details)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                now_micros(),
                accessor_type,
                accessor_id,
                action,
                serde_json::to_string(nodes).unwrap_or_else(|_| "[]".into()),
                projection_hash,
                details,
            ],
        )?;
        Ok(())
    }

    /// `(timestamp, accessor_type, accessor_id, action, nodes_json)` rows,
    /// newest first, optionally since a time / for one accessor.
    pub fn read_audit(
        &self,
        since: Option<i64>,
        accessor: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AuditRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, accessor_type, accessor_id, action, nodes_accessed
             FROM graph_access_log
             WHERE (?1 IS NULL OR timestamp >= ?1)
               AND (?2 IS NULL OR accessor_id = ?2)
             ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![since, accessor, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- stats / maintenance --------------------------------------------

    /// `(active_nodes, total_nodes, edges, last_update_us)`.
    pub fn stats(&self) -> Result<(usize, usize, usize, i64)> {
        let active: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM graph_nodes WHERE state='active'",
            [],
            |r| r.get(0),
        )?;
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM graph_nodes", [], |r| r.get(0))?;
        let edges: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM graph_edges", [], |r| r.get(0))?;
        let last: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(updated_at),0) FROM graph_nodes",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok((active as usize, total as usize, edges as usize, last))
    }

    /// Re-encrypt every node after a tier-key rotation. Reads with the old
    /// key store, regenerates keys, rewrites every node blob.
    pub fn rotate_keys(&mut self) -> Result<usize> {
        let metas = self.list_nodes(None)?;
        let mut nodes: Vec<(GraphNode, NodeState)> = Vec::new();
        for m in &metas {
            nodes.push((self.get_node(&m.id)?, m.state));
        }
        self.keys.regenerate_tier_keys()?;
        for (n, st) in &nodes {
            self.put_node(n, *st)?;
        }
        Ok(nodes.len())
    }

    pub fn keys(&self) -> &KeyStore {
        &self.keys
    }
}

/// Convenience for tests / inference: an `Active`, human-created node.
pub fn active_node(
    node_type: crate::object::NodeType,
    name: &str,
    description: &str,
    tier: AccessTier,
    author: gpp_history::Author,
) -> GraphNode {
    let t = now_micros();
    GraphNode {
        node_type,
        name: name.to_string(),
        description: description.to_string(),
        access_tier: tier,
        properties: BTreeMap::new(),
        created_by: author,
        created_at: t,
        updated_at: t,
        confidence: 1.0,
        validated_at: None,
        source: crate::object::NodeSource::HumanCreated,
    }
}
