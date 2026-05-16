//! SQLite timeline database (WAL mode). Schema mirrors
//! `docs/DATA_MODEL.md` § Timeline Schema, plus a `workspace_state` table
//! used by the scanner to detect changes between captures.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::error::Result;
use crate::model::{AuthorKind, ChangeType, EntryFilter, EntryView, FileChange};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS timeline_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp   INTEGER NOT NULL,
    author_type TEXT NOT NULL CHECK(author_type IN ('human', 'agent')),
    author_id   TEXT NOT NULL,
    source      TEXT NOT NULL CHECK(source IN ('editor','cli','agent-sdk','fs-watch','import')),
    summary     TEXT,
    parent_id   INTEGER REFERENCES timeline_entries(id),
    promoted_to TEXT
);
CREATE INDEX IF NOT EXISTS idx_timeline_timestamp ON timeline_entries(timestamp);
CREATE INDEX IF NOT EXISTS idx_timeline_author ON timeline_entries(author_id);

CREATE TABLE IF NOT EXISTS timeline_files (
    entry_id    INTEGER NOT NULL REFERENCES timeline_entries(id) ON DELETE CASCADE,
    file_path   TEXT NOT NULL,
    blob_hash   TEXT NOT NULL,
    change_type TEXT NOT NULL CHECK(change_type IN ('add','modify','delete','rename')),
    old_hash    TEXT,
    old_path    TEXT,
    PRIMARY KEY (entry_id, file_path)
);
CREATE INDEX IF NOT EXISTS idx_timeline_files_path ON timeline_files(file_path);

CREATE TABLE IF NOT EXISTS timeline_retention (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    pruned_before   INTEGER NOT NULL,
    pruned_at       INTEGER NOT NULL,
    entries_removed INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace_state (
    file_path TEXT PRIMARY KEY,
    blob_hash TEXT NOT NULL,
    size      INTEGER NOT NULL,
    mtime_us  INTEGER NOT NULL
);
"#;

pub struct TimelineDb {
    conn: Connection,
}

impl TimelineDb {
    /// Open (creating if needed) the timeline DB at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Id of the most recent entry, if any (used as the next entry's parent).
    pub fn last_entry_id(&self) -> Result<Option<i64>> {
        let id = self
            .conn
            .query_row("SELECT MAX(id) FROM timeline_entries", [], |r| {
                r.get::<_, Option<i64>>(0)
            })?;
        Ok(id)
    }

    /// Insert one entry with its file changes, in a single transaction.
    pub fn insert_entry(
        &mut self,
        timestamp: i64,
        author_kind: AuthorKind,
        author_id: &str,
        source: &str,
        summary: Option<&str>,
        files: &[FileChange],
    ) -> Result<i64> {
        let parent = self.last_entry_id()?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO timeline_entries
                (timestamp, author_type, author_id, source, summary, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                timestamp,
                author_kind.as_str(),
                author_id,
                source,
                summary,
                parent
            ],
        )?;
        let entry_id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO timeline_files
                    (entry_id, file_path, blob_hash, change_type, old_hash, old_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for f in files {
                stmt.execute(params![
                    entry_id,
                    f.path,
                    f.blob_hash.map(|h| h.to_base32()).unwrap_or_default(),
                    f.change.as_str(),
                    f.old_hash.map(|h| h.to_base32()),
                    f.old_path,
                ])?;
            }
        }
        tx.commit()?;
        Ok(entry_id)
    }

    /// Query entries newest-first, applying `filter`.
    pub fn query_entries(&self, filter: &EntryFilter) -> Result<Vec<EntryView>> {
        let sql =
            "SELECT id, timestamp, author_type, author_id, source, summary, parent_id, promoted_to
             FROM timeline_entries
             WHERE (:since IS NULL OR timestamp >= :since)
               AND (:until IS NULL OR timestamp <= :until)
               AND (:author IS NULL OR author_id = :author)
             ORDER BY id DESC LIMIT :limit";

        let mut stmt = self.conn.prepare(sql)?;
        let limit = filter.limit.unwrap_or(20) as i64;
        let rows = stmt.query_map(
            rusqlite::named_params! {
                ":since": filter.since,
                ":until": filter.until,
                ":author": filter.author,
                ":limit": limit,
            },
            |r| {
                Ok(EntryView {
                    id: r.get(0)?,
                    timestamp: r.get(1)?,
                    author_kind: AuthorKind::from_db(&r.get::<_, String>(2)?),
                    author_id: r.get(3)?,
                    source: r.get(4)?,
                    summary: r.get(5)?,
                    parent_id: r.get(6)?,
                    promoted_to: r.get(7)?,
                    files: Vec::new(),
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let mut e = row?;
            e.files = self.entry_files(e.id)?;
            if let Some(pat) = &filter.file_glob
                && !e.files.iter().any(|f| pat.is_match(&f.path))
            {
                continue;
            }
            out.push(e);
        }
        Ok(out)
    }

    /// File changes recorded for one entry.
    pub fn entry_files(&self, entry_id: i64) -> Result<Vec<FileChange>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, blob_hash, change_type, old_hash, old_path
             FROM timeline_files WHERE entry_id = ?1 ORDER BY file_path",
        )?;
        let rows = stmt.query_map([entry_id], |r| {
            Ok(FileChange {
                path: r.get(0)?,
                blob_hash: parse_hash(r.get::<_, String>(1)?.as_str()),
                change: ChangeType::from_db(&r.get::<_, String>(2)?),
                old_hash: r.get::<_, Option<String>>(3)?.and_then(|s| parse_hash(&s)),
                old_path: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Mark a set of entries as promoted into `changeset`.
    pub fn mark_promoted(&self, entry_ids: &[i64], changeset: &str) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("UPDATE timeline_entries SET promoted_to = ?1 WHERE id = ?2")?;
        for id in entry_ids {
            stmt.execute(params![changeset, id])?;
        }
        Ok(())
    }

    /// Ids (ascending) of unpromoted entries within an inclusive id range.
    pub fn unpromoted_in_range(&self, from: Option<i64>, to: Option<i64>) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM timeline_entries
             WHERE promoted_to IS NULL
               AND (?1 IS NULL OR id >= ?1)
               AND (?2 IS NULL OR id <= ?2)
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![from, to], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Delete entries older than `cutoff_us`, recording the action. Returns
    /// the number of entries removed.
    pub fn prune(&self, cutoff_us: i64, now_us: i64) -> Result<u64> {
        let removed = self.conn.execute(
            "DELETE FROM timeline_entries WHERE timestamp < ?1",
            [cutoff_us],
        )?;
        if removed > 0 {
            self.conn.execute(
                "INSERT INTO timeline_retention (pruned_before, pruned_at, entries_removed)
                 VALUES (?1, ?2, ?3)",
                params![cutoff_us, now_us, removed as i64],
            )?;
        }
        Ok(removed as u64)
    }

    // -- workspace_state -----------------------------------------------------

    pub fn load_state(&self) -> Result<HashMap<String, StateRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT file_path, blob_hash, size, mtime_us FROM workspace_state")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                StateRow {
                    blob_hash: r.get(1)?,
                    size: r.get(2)?,
                    mtime_us: r.get(3)?,
                },
            ))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (k, v) = row?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn upsert_state(&self, path: &str, row: &StateRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO workspace_state (file_path, blob_hash, size, mtime_us)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_path) DO UPDATE SET
                blob_hash=excluded.blob_hash, size=excluded.size, mtime_us=excluded.mtime_us",
            params![path, row.blob_hash, row.size, row.mtime_us],
        )?;
        Ok(())
    }

    pub fn delete_state(&self, path: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM workspace_state WHERE file_path = ?1", [path])?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct StateRow {
    pub blob_hash: String,
    pub size: i64,
    pub mtime_us: i64,
}

fn parse_hash(s: &str) -> Option<gpp_core::Hash> {
    if s.is_empty() {
        None
    } else {
        gpp_core::Hash::from_base32(s).ok()
    }
}
