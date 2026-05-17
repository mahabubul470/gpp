//! `gpp-trust` — agent reputation scoring and behavioral RBAC (layer 5).
//!
//! Every agent contribution is an *event*. Aggregates (changesets, survived
//! review, regressions) drive a Bayesian-smoothed trust score in `[0,100]`,
//! which maps to a behavioral status (`auto-merge` / `review-required` /
//! `sandboxed` / `blocked`) via configurable thresholds. Humans can override
//! status globally or per module. All transitions are logged.
//!
//! See `docs/DATA_MODEL.md`, `docs/SECURITY_MODEL.md`, `docs/ROADMAP.md`
//! (Phase 4).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("trust database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("unknown trust status {0:?}")]
    UnknownStatus(String),
    #[error("unknown agent {0:?}")]
    UnknownAgent(String),
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

/// Behavioral status, gated by score (or a manual override).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    AutoMerge,
    ReviewRequired,
    Sandboxed,
    Blocked,
}

impl TrustStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustStatus::AutoMerge => "auto-merge",
            TrustStatus::ReviewRequired => "review-required",
            TrustStatus::Sandboxed => "sandboxed",
            TrustStatus::Blocked => "blocked",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "auto-merge" => TrustStatus::AutoMerge,
            "review-required" => TrustStatus::ReviewRequired,
            "sandboxed" => TrustStatus::Sandboxed,
            "blocked" => TrustStatus::Blocked,
            other => return Err(Error::UnknownStatus(other.to_string())),
        })
    }
}

/// Threshold policy (mirrors `[trust]` in `.gpp/config.toml`).
#[derive(Debug, Clone, Copy)]
pub struct TrustPolicy {
    pub auto_merge_min: f64,
    pub review_required_min: f64,
    pub sandbox_min: f64,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            auto_merge_min: 90.0,
            review_required_min: 70.0,
            sandbox_min: 50.0,
        }
    }
}

impl TrustPolicy {
    /// Status implied by a score (ignoring overrides).
    pub fn status_for(&self, score: f64) -> TrustStatus {
        if score >= self.auto_merge_min {
            TrustStatus::AutoMerge
        } else if score >= self.review_required_min {
            TrustStatus::ReviewRequired
        } else if score >= self.sandbox_min {
            TrustStatus::Sandboxed
        } else {
            TrustStatus::Blocked
        }
    }
}

/// One agent's reputation snapshot.
#[derive(Debug, Clone)]
pub struct AgentScore {
    pub agent_id: String,
    pub agent_name: String,
    pub model_id: Option<String>,
    pub trust_score: f64,
    pub total_changesets: i64,
    pub survived_review: i64,
    pub regressions: i64,
    pub first_seen: i64,
    pub last_active: i64,
    pub status: TrustStatus,
    /// `Some` when a human has pinned status (until `override_until`).
    pub override_status: Option<TrustStatus>,
    pub override_reason: Option<String>,
    pub override_until: Option<i64>,
}

impl AgentScore {
    /// Status actually in force right now (override if live, else score-based).
    pub fn effective_status(&self, now: i64) -> TrustStatus {
        match (self.override_status, self.override_until) {
            (Some(s), None) => s,
            (Some(s), Some(until)) if until > now => s,
            _ => self.status,
        }
    }
}

/// Bayesian-smoothed score: a new agent sits at the 50 prior; survived
/// reviews pull up, regressions pull down, with `PRIOR` pseudo-observations
/// so a single early result does not swing the score wildly.
fn compute_score(_total: i64, survived: i64, regressions: i64) -> f64 {
    // Based on *reviewed outcomes* only — merely promoting (pending review)
    // is not a failure. Beta(1,1)-style prior keeps a fresh agent (no
    // outcomes) at 50; regressions are weighted 1.5× survivals. A
    // consistently-surviving agent crosses 90 within ~a dozen reviews.
    let pos = survived as f64;
    let neg = regressions as f64;
    (100.0 * (pos + 1.0) / (pos + 1.5 * neg + 2.0)).clamp(0.0, 100.0)
}

/// One trust-event row: `(timestamp_us, event_type, changeset, details)`.
pub type TrustEventRow = (i64, String, Option<String>, Option<String>);

pub struct TrustStore {
    conn: Connection,
}

impl TrustStore {
    /// Open (creating if needed) `<gpp_dir>/trust/trust.db`.
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("trust");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("trust.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS agent_scores (
                agent_id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                model_id TEXT,
                trust_score REAL NOT NULL DEFAULT 50.0,
                total_changesets INTEGER NOT NULL DEFAULT 0,
                survived_review INTEGER NOT NULL DEFAULT 0,
                regressions INTEGER NOT NULL DEFAULT 0,
                first_seen INTEGER NOT NULL,
                last_active INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'sandboxed',
                override_status TEXT,
                override_reason TEXT,
                override_until INTEGER
             );
             CREATE TABLE IF NOT EXISTS agent_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                changeset TEXT,
                details TEXT,
                timestamp INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_agent_events_agent
                ON agent_events(agent_id, timestamp);
             CREATE TABLE IF NOT EXISTS module_overrides (
                agent_id TEXT NOT NULL,
                module_pattern TEXT NOT NULL,
                status TEXT NOT NULL,
                reason TEXT,
                PRIMARY KEY (agent_id, module_pattern)
             );",
        )?;
        Ok(Self { conn })
    }

    fn ensure_agent(&self, agent_id: &str, agent_name: &str, model: Option<&str>) -> Result<()> {
        let now = now_micros();
        self.conn.execute(
            "INSERT INTO agent_scores
                (agent_id, agent_name, model_id, first_seen, last_active, status)
             VALUES (?1,?2,?3,?4,?4,'sandboxed')
             ON CONFLICT(agent_id) DO UPDATE SET
                agent_name=excluded.agent_name,
                model_id=COALESCE(excluded.model_id, agent_scores.model_id),
                last_active=excluded.last_active",
            params![agent_id, agent_name, model, now],
        )?;
        Ok(())
    }

    /// Record an event and recompute the agent's score + score-based status.
    ///
    /// `event_type`: `changeset_merged` (+survived), `changeset_rejected`,
    /// `regression` (+regression), `changeset_promoted` (+total), or any
    /// custom audit string. Returns the new score.
    pub fn record_event(
        &self,
        agent_id: &str,
        agent_name: &str,
        model: Option<&str>,
        event_type: &str,
        changeset: Option<&str>,
        details: Option<&str>,
    ) -> Result<f64> {
        self.ensure_agent(agent_id, agent_name, model)?;
        let now = now_micros();
        self.conn.execute(
            "INSERT INTO agent_events (agent_id, event_type, changeset, details, timestamp)
             VALUES (?1,?2,?3,?4,?5)",
            params![agent_id, event_type, changeset, details, now],
        )?;
        match event_type {
            "changeset_promoted" => self.bump(agent_id, "total_changesets")?,
            "changeset_merged" => self.bump(agent_id, "survived_review")?,
            "regression" => self.bump(agent_id, "regressions")?,
            _ => {}
        }
        let (total, survived, regr): (i64, i64, i64) = self.conn.query_row(
            "SELECT total_changesets, survived_review, regressions
             FROM agent_scores WHERE agent_id=?1",
            [agent_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let score = compute_score(total, survived, regr);
        let status = TrustPolicy::default().status_for(score).as_str();
        self.conn.execute(
            "UPDATE agent_scores SET trust_score=?1, status=?2, last_active=?3
             WHERE agent_id=?4",
            params![score, status, now, agent_id],
        )?;
        Ok(score)
    }

    fn bump(&self, agent_id: &str, col: &str) -> Result<()> {
        // `col` is never user input — it is one of three literals above.
        self.conn.execute(
            &format!("UPDATE agent_scores SET {col}={col}+1 WHERE agent_id=?1"),
            [agent_id],
        )?;
        Ok(())
    }

    pub fn score(&self, agent_id: &str) -> Result<Option<AgentScore>> {
        self.conn
            .query_row(
                "SELECT agent_id, agent_name, model_id, trust_score, total_changesets,
                        survived_review, regressions, first_seen, last_active, status,
                        override_status, override_reason, override_until
                 FROM agent_scores WHERE agent_id=?1",
                [agent_id],
                Self::row_to_score,
            )
            .optional()
            .map_err(Error::from)
    }

    fn row_to_score(r: &rusqlite::Row) -> rusqlite::Result<AgentScore> {
        Ok(AgentScore {
            agent_id: r.get(0)?,
            agent_name: r.get(1)?,
            model_id: r.get(2)?,
            trust_score: r.get(3)?,
            total_changesets: r.get(4)?,
            survived_review: r.get(5)?,
            regressions: r.get(6)?,
            first_seen: r.get(7)?,
            last_active: r.get(8)?,
            status: TrustStatus::parse(&r.get::<_, String>(9)?).unwrap_or(TrustStatus::Sandboxed),
            override_status: r
                .get::<_, Option<String>>(10)?
                .and_then(|s| TrustStatus::parse(&s).ok()),
            override_reason: r.get(11)?,
            override_until: r.get(12)?,
        })
    }

    pub fn list(&self) -> Result<Vec<AgentScore>> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_id, agent_name, model_id, trust_score, total_changesets,
                    survived_review, regressions, first_seen, last_active, status,
                    override_status, override_reason, override_until
             FROM agent_scores ORDER BY trust_score DESC",
        )?;
        let rows = stmt.query_map([], Self::row_to_score)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Pin a status manually (logged as a `manual_override` event).
    pub fn override_status(
        &self,
        agent_id: &str,
        status: TrustStatus,
        reason: &str,
        until: Option<i64>,
    ) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE agent_scores
             SET override_status=?1, override_reason=?2, override_until=?3
             WHERE agent_id=?4",
            params![status.as_str(), reason, until, agent_id],
        )?;
        if n == 0 {
            return Err(Error::UnknownAgent(agent_id.to_string()));
        }
        self.conn.execute(
            "INSERT INTO agent_events (agent_id, event_type, changeset, details, timestamp)
             VALUES (?1,'manual_override',NULL,?2,?3)",
            params![
                agent_id,
                format!("status={} reason={}", status.as_str(), reason),
                now_micros()
            ],
        )?;
        Ok(())
    }

    pub fn reset(&self, agent_id: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE agent_scores
             SET trust_score=50.0, total_changesets=0, survived_review=0, regressions=0,
                 status='sandboxed', override_status=NULL, override_reason=NULL,
                 override_until=NULL
             WHERE agent_id=?1",
            [agent_id],
        )?;
        if n == 0 {
            return Err(Error::UnknownAgent(agent_id.to_string()));
        }
        self.conn.execute(
            "INSERT INTO agent_events (agent_id, event_type, changeset, details, timestamp)
             VALUES (?1,'reset',NULL,NULL,?2)",
            params![agent_id, now_micros()],
        )?;
        Ok(())
    }

    /// `(timestamp, event_type, changeset, details)` newest first.
    pub fn history(
        &self,
        agent_id: &str,
        since: Option<i64>,
        limit: usize,
    ) -> Result<Vec<TrustEventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, event_type, changeset, details FROM agent_events
             WHERE agent_id=?1 AND (?2 IS NULL OR timestamp>=?2)
             ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![agent_id, since, limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_module_override(
        &self,
        agent_id: &str,
        module_pattern: &str,
        status: TrustStatus,
        reason: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO module_overrides
                (agent_id, module_pattern, status, reason) VALUES (?1,?2,?3,?4)",
            params![agent_id, module_pattern, status.as_str(), reason],
        )?;
        Ok(())
    }

    /// Effective status for an agent, considering (in priority order) a live
    /// global override, then the score-derived status.
    pub fn effective_status(&self, agent_id: &str) -> Result<TrustStatus> {
        let s = self
            .score(agent_id)?
            .ok_or_else(|| Error::UnknownAgent(agent_id.to_string()))?;
        Ok(s.effective_status(now_micros()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> (tempfile::TempDir, TrustStore) {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        let s = TrustStore::open(&gpp).unwrap();
        (d, s)
    }

    #[test]
    fn new_agent_starts_at_prior() {
        let (_d, s) = ts();
        s.record_event("agent:x", "X", Some("m"), "changeset_promoted", None, None)
            .unwrap();
        let sc = s.score("agent:x").unwrap().unwrap();
        assert!((sc.trust_score - 50.0).abs() < 12.0);
        assert_eq!(sc.total_changesets, 1);
    }

    #[test]
    fn survival_raises_regression_lowers() {
        let (_d, s) = ts();
        for _ in 0..12 {
            s.record_event("a", "A", None, "changeset_promoted", None, None)
                .unwrap();
            s.record_event("a", "A", None, "changeset_merged", None, None)
                .unwrap();
        }
        let high = s.score("a").unwrap().unwrap();
        assert!(high.trust_score > 90.0, "{}", high.trust_score);
        assert_eq!(high.effective_status(now_micros()), TrustStatus::AutoMerge);

        for _ in 0..10 {
            s.record_event("a", "A", None, "regression", None, None)
                .unwrap();
        }
        let low = s.score("a").unwrap().unwrap();
        assert!(low.trust_score < high.trust_score);
    }

    #[test]
    fn override_takes_priority_until_expiry() {
        let (_d, s) = ts();
        // Build a high score so the score-based status is *not* Blocked.
        for _ in 0..12 {
            s.record_event("a", "A", None, "changeset_promoted", None, None)
                .unwrap();
            s.record_event("a", "A", None, "changeset_merged", None, None)
                .unwrap();
        }
        assert_eq!(s.effective_status("a").unwrap(), TrustStatus::AutoMerge);

        s.override_status("a", TrustStatus::Blocked, "compromised", None)
            .unwrap();
        assert_eq!(s.effective_status("a").unwrap(), TrustStatus::Blocked);

        // Expired override falls back to the (high) score-based status.
        s.override_status("a", TrustStatus::Blocked, "temp", Some(1))
            .unwrap();
        assert_eq!(s.effective_status("a").unwrap(), TrustStatus::AutoMerge);
        assert!(s.history("a", None, 10).unwrap().len() >= 3);
    }
}
