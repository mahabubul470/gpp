//! `gpp-anomaly` — agent behavior pattern detection (layer 11).
//!
//! Detection rules run against a [`ChangesetFacts`] at promotion time:
//!
//! * `large-changeset` — `lines_changed` over a threshold.
//! * `unusual-scope` — `files_touched` over a threshold.
//! * `burst-activity` — too many changesets by one agent in a window.
//!
//! Hits become `anomaly_events` with a severity; they can be listed,
//! filtered, and resolved. Thresholds live in a `anomaly_rules` table and
//! are tunable via [`AnomalyStore::configure`].
//!
//! See `docs/DATA_MODEL.md`, `docs/ROADMAP.md` (Phase 4).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("anomaly database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("unknown rule {0:?}")]
    UnknownRule(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn now_micros() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Review,
    Block,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Review => "review",
            Severity::Block => "block",
        }
    }
}

/// Inputs a single promotion presents to the detector.
#[derive(Debug, Clone, Default)]
pub struct ChangesetFacts {
    pub agent_id: Option<String>,
    pub changeset: String,
    pub files_touched: i64,
    pub lines_changed: i64,
    /// Changesets by this author within the burst window (caller-supplied).
    pub recent_changesets_in_window: i64,
}

#[derive(Debug, Clone)]
pub struct AnomalyEvent {
    pub id: i64,
    pub timestamp: i64,
    pub rule_id: String,
    pub severity: String,
    pub agent_id: Option<String>,
    pub changeset: Option<String>,
    pub description: String,
    pub resolved: bool,
}

#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub rule_id: String,
    pub threshold: i64,
    pub severity: Severity,
    pub enabled: bool,
}

const DEFAULTS: &[(&str, i64, Severity)] = &[
    ("large-changeset", 800, Severity::Review),
    ("unusual-scope", 25, Severity::Review),
    ("burst-activity", 20, Severity::Warning),
];

pub struct AnomalyStore {
    conn: Connection,
}

impl AnomalyStore {
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("anomaly");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("anomaly.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS anomaly_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                rule_id TEXT NOT NULL,
                severity TEXT NOT NULL,
                agent_id TEXT,
                changeset TEXT,
                description TEXT NOT NULL,
                details TEXT,
                resolved INTEGER NOT NULL DEFAULT 0,
                resolved_by TEXT,
                resolved_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_anomaly_time ON anomaly_events(timestamp);
             CREATE TABLE IF NOT EXISTS anomaly_rules (
                rule_id TEXT PRIMARY KEY,
                threshold INTEGER NOT NULL,
                severity TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1
             );",
        )?;
        let store = Self { conn };
        for (id, thr, sev) in DEFAULTS {
            store.conn.execute(
                "INSERT OR IGNORE INTO anomaly_rules (rule_id, threshold, severity, enabled)
                 VALUES (?1,?2,?3,1)",
                params![id, thr, sev.as_str()],
            )?;
        }
        Ok(store)
    }

    fn rule(&self, id: &str) -> Result<Option<(i64, String, bool)>> {
        self.conn
            .query_row(
                "SELECT threshold, severity, enabled FROM anomaly_rules WHERE rule_id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(Error::from)
    }

    /// Run all enabled rules against `facts`, recording any hits. Returns the
    /// descriptions of anomalies raised.
    pub fn detect(&self, facts: &ChangesetFacts) -> Result<Vec<String>> {
        let mut raised = Vec::new();
        let checks = [
            ("large-changeset", facts.lines_changed, "lines changed"),
            ("unusual-scope", facts.files_touched, "files touched"),
            (
                "burst-activity",
                facts.recent_changesets_in_window,
                "changesets in window",
            ),
        ];
        for (rule_id, value, unit) in checks {
            let Some((threshold, sev, enabled)) = self.rule(rule_id)? else {
                continue;
            };
            if enabled && value > threshold {
                let desc = format!("{rule_id}: {value} {unit} (threshold {threshold})");
                self.conn.execute(
                    "INSERT INTO anomaly_events
                        (timestamp, rule_id, severity, agent_id, changeset, description, details)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        now_micros(),
                        rule_id,
                        sev,
                        facts.agent_id,
                        facts.changeset,
                        desc,
                        serde_json::json!({"value": value, "threshold": threshold}).to_string(),
                    ],
                )?;
                raised.push(desc);
            }
        }
        Ok(raised)
    }

    /// List events, optionally only unresolved / by agent / since a time.
    pub fn list(
        &self,
        unresolved_only: bool,
        agent: Option<&str>,
        since: Option<i64>,
        limit: usize,
    ) -> Result<Vec<AnomalyEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, rule_id, severity, agent_id, changeset, description, resolved
             FROM anomaly_events
             WHERE (?1=0 OR resolved=0)
               AND (?2 IS NULL OR agent_id=?2)
               AND (?3 IS NULL OR timestamp>=?3)
             ORDER BY timestamp DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![unresolved_only as i64, agent, since, limit as i64],
            |r| {
                Ok(AnomalyEvent {
                    id: r.get(0)?,
                    timestamp: r.get(1)?,
                    rule_id: r.get(2)?,
                    severity: r.get(3)?,
                    agent_id: r.get(4)?,
                    changeset: r.get(5)?,
                    description: r.get(6)?,
                    resolved: r.get::<_, i64>(7)? != 0,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn resolve(&self, id: i64, resolved_by: &str, reason: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE anomaly_events
             SET resolved=1, resolved_by=?1, resolved_at=?2,
                 details=COALESCE(details,'')||?3
             WHERE id=?4",
            params![
                resolved_by,
                now_micros(),
                format!(" resolved: {reason}"),
                id
            ],
        )?;
        if n == 0 {
            return Err(Error::Other(format!("no anomaly with id {id}")));
        }
        Ok(())
    }

    pub fn rules(&self) -> Result<Vec<RuleConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT rule_id, threshold, severity, enabled FROM anomaly_rules ORDER BY rule_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(RuleConfig {
                rule_id: r.get(0)?,
                threshold: r.get(1)?,
                severity: match r.get::<_, String>(2)?.as_str() {
                    "block" => Severity::Block,
                    "review" => Severity::Review,
                    "warning" => Severity::Warning,
                    _ => Severity::Info,
                },
                enabled: r.get::<_, i64>(3)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Tune a rule's threshold and/or enabled flag.
    pub fn configure(
        &self,
        rule_id: &str,
        threshold: Option<i64>,
        enabled: Option<bool>,
    ) -> Result<()> {
        if self.rule(rule_id)?.is_none() {
            return Err(Error::UnknownRule(rule_id.to_string()));
        }
        if let Some(t) = threshold {
            self.conn.execute(
                "UPDATE anomaly_rules SET threshold=?1 WHERE rule_id=?2",
                params![t, rule_id],
            )?;
        }
        if let Some(e) = enabled {
            self.conn.execute(
                "UPDATE anomaly_rules SET enabled=?1 WHERE rule_id=?2",
                params![e as i64, rule_id],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AnomalyStore) {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        let s = AnomalyStore::open(&gpp).unwrap();
        (d, s)
    }

    #[test]
    fn detects_large_and_scope_then_resolves() {
        let (_d, s) = store();
        let raised = s
            .detect(&ChangesetFacts {
                agent_id: Some("agent:a".into()),
                changeset: "cs1".into(),
                files_touched: 40,
                lines_changed: 1500,
                recent_changesets_in_window: 2,
            })
            .unwrap();
        assert_eq!(raised.len(), 2); // large-changeset + unusual-scope
        let open = s.list(true, None, None, 10).unwrap();
        assert_eq!(open.len(), 2);

        s.resolve(open[0].id, "dev", "expected during refactor")
            .unwrap();
        assert_eq!(s.list(true, None, None, 10).unwrap().len(), 1);
    }

    #[test]
    fn configure_changes_behavior() {
        let (_d, s) = store();
        // Disable large-changeset; nothing should fire for big line counts.
        s.configure("large-changeset", None, Some(false)).unwrap();
        let raised = s
            .detect(&ChangesetFacts {
                changeset: "cs".into(),
                files_touched: 2,
                lines_changed: 99_999,
                ..Default::default()
            })
            .unwrap();
        assert!(raised.is_empty());

        // Lower burst threshold and trip it.
        s.configure("burst-activity", Some(3), None).unwrap();
        let raised = s
            .detect(&ChangesetFacts {
                changeset: "cs2".into(),
                recent_changesets_in_window: 5,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(raised.len(), 1);
        assert!(s.rules().unwrap().iter().any(|r| !r.enabled));
    }

    #[test]
    fn unknown_rule_config_errors() {
        let (_d, s) = store();
        assert!(s.configure("nope", Some(1), None).is_err());
    }
}
