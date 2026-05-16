//! The Git ↔ gpp hash-mapping database (`.gpp/git-bridge/map.db`).
//!
//! One SQLite table maps a Git commit oid (hex) to the gpp changeset hash
//! (base32) it was imported as / exported to. The mapping is the source of
//! truth for incremental, idempotent sync in both directions.

use std::path::Path;

use gpp_core::Hash;
use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

pub struct HashMap {
    conn: Connection,
}

impl HashMap {
    /// Open (creating if needed) the mapping DB inside `<gpp_dir>/git-bridge`.
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("git-bridge");
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("map.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS commit_map (
                 git_oid  TEXT PRIMARY KEY,
                 gpp_hash TEXT NOT NULL UNIQUE
             );",
        )?;
        Ok(Self { conn })
    }

    /// gpp changeset hash a Git commit was imported as, if any.
    pub fn gpp_for_commit(&self, git_oid: &str) -> Result<Option<Hash>> {
        let row: Option<String> = self
            .conn
            .query_row(
                "SELECT gpp_hash FROM commit_map WHERE git_oid = ?1",
                [git_oid],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.and_then(|s| Hash::from_base32(&s).ok()))
    }

    /// Git commit oid a gpp changeset was exported to, if any.
    pub fn commit_for_gpp(&self, gpp: &Hash) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT git_oid FROM commit_map WHERE gpp_hash = ?1",
                [gpp.to_base32()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Record a commit ↔ changeset correspondence (idempotent).
    pub fn link(&self, git_oid: &str, gpp: &Hash) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO commit_map (git_oid, gpp_hash) VALUES (?1, ?2)",
            (git_oid, gpp.to_base32()),
        )?;
        Ok(())
    }

    /// Number of linked commits.
    pub fn len(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM commit_map", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }
}
