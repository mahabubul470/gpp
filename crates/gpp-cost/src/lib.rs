//! `gpp-cost` — token/compute cost attribution (layer 9).
//!
//! One [`CostRecord`] per changeset: tokens, micro-dollars, duration, and
//! (once review completes) lines that survived. Supports time/agent-scoped
//! roll-ups, an efficiency metric (cost per surviving line), and weekly
//! budgets with an alert threshold.
//!
//! Money is integer **micro-dollars** (1 = $0.000001), per project convention.
//!
//! See `docs/DATA_MODEL.md`, `docs/ROADMAP.md` (Phase 4).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("cost database error: {0}")]
    Db(#[from] rusqlite::Error),
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

const WEEK_US: i64 = 7 * 86_400_000_000;

#[derive(Debug, Clone, Default)]
pub struct CostRecord {
    pub changeset_id: String,
    pub agent_id: String,
    pub model_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_microdollars: i64,
    pub duration_ms: i64,
    pub files_touched: i64,
    pub lines_changed: i64,
    /// `None` until review completes.
    pub lines_survived: Option<i64>,
    pub timestamp: i64,
}

/// Aggregate roll-up over a filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CostSummary {
    pub changesets: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub cost_microdollars: i64,
    pub lines_changed: i64,
    pub lines_survived: i64,
}

impl CostSummary {
    /// Micro-dollars per surviving line (`None` if nothing survived yet).
    pub fn cost_per_survived_line(&self) -> Option<f64> {
        if self.lines_survived <= 0 {
            None
        } else {
            Some(self.cost_microdollars as f64 / self.lines_survived as f64)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CostFilter {
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BudgetStatus {
    pub module_pattern: String,
    pub weekly_limit: i64,
    pub spent_this_week: i64,
    pub alert_threshold: f64,
    pub alerting: bool,
}

pub struct CostStore {
    conn: Connection,
}

impl CostStore {
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("cost");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("cost.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS cost_records (
                changeset_id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cached_tokens INTEGER NOT NULL DEFAULT 0,
                cost_microdollars INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                files_touched INTEGER NOT NULL DEFAULT 0,
                lines_changed INTEGER NOT NULL DEFAULT 0,
                lines_survived INTEGER,
                timestamp INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_cost_agent ON cost_records(agent_id);
             CREATE INDEX IF NOT EXISTS idx_cost_timestamp ON cost_records(timestamp);
             CREATE TABLE IF NOT EXISTS cost_budgets (
                module_pattern TEXT PRIMARY KEY,
                weekly_limit INTEGER NOT NULL,
                alert_threshold REAL NOT NULL DEFAULT 0.8
             );",
        )?;
        Ok(Self { conn })
    }

    /// Insert or replace the cost record for a changeset.
    pub fn record(&self, r: &CostRecord) -> Result<()> {
        let ts = if r.timestamp == 0 {
            now_micros()
        } else {
            r.timestamp
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO cost_records
                (changeset_id, agent_id, model_id, input_tokens, output_tokens,
                 cached_tokens, cost_microdollars, duration_ms, files_touched,
                 lines_changed, lines_survived, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                r.changeset_id,
                r.agent_id,
                r.model_id,
                r.input_tokens,
                r.output_tokens,
                r.cached_tokens,
                r.cost_microdollars,
                r.duration_ms,
                r.files_touched,
                r.lines_changed,
                r.lines_survived,
                ts,
            ],
        )?;
        Ok(())
    }

    /// Set surviving-line count once a changeset's review concludes.
    pub fn set_lines_survived(&self, changeset_id: &str, lines: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE cost_records SET lines_survived=?1 WHERE changeset_id=?2",
            params![lines, changeset_id],
        )?;
        Ok(())
    }

    pub fn summarize(&self, f: &CostFilter) -> Result<CostSummary> {
        self.conn
            .query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(input_tokens),0),
                        COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(cached_tokens),0),
                        COALESCE(SUM(cost_microdollars),0),
                        COALESCE(SUM(lines_changed),0),
                        COALESCE(SUM(lines_survived),0)
                 FROM cost_records
                 WHERE (?1 IS NULL OR timestamp>=?1)
                   AND (?2 IS NULL OR timestamp<=?2)
                   AND (?3 IS NULL OR agent_id=?3)",
                params![f.since, f.until, f.agent],
                |r| {
                    Ok(CostSummary {
                        changesets: r.get(0)?,
                        input_tokens: r.get(1)?,
                        output_tokens: r.get(2)?,
                        cached_tokens: r.get(3)?,
                        cost_microdollars: r.get(4)?,
                        lines_changed: r.get(5)?,
                        lines_survived: r.get(6)?,
                    })
                },
            )
            .map_err(Error::from)
    }

    /// `(agent_id, model_id, cost_microdollars, changesets)` grouped,
    /// most-expensive first.
    pub fn breakdown(&self, f: &CostFilter) -> Result<Vec<(String, String, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT agent_id, model_id, COALESCE(SUM(cost_microdollars),0), COUNT(*)
             FROM cost_records
             WHERE (?1 IS NULL OR timestamp>=?1)
               AND (?2 IS NULL OR timestamp<=?2)
               AND (?3 IS NULL OR agent_id=?3)
             GROUP BY agent_id, model_id
             ORDER BY 3 DESC",
        )?;
        let rows = stmt.query_map(params![f.since, f.until, f.agent], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_budget(
        &self,
        module_pattern: &str,
        weekly_limit: i64,
        alert_threshold: f64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cost_budgets
                (module_pattern, weekly_limit, alert_threshold) VALUES (?1,?2,?3)",
            params![module_pattern, weekly_limit, alert_threshold],
        )?;
        Ok(())
    }

    /// Budget usage for the current rolling week (all spend counts toward
    /// every budget; per-path attribution arrives with the review layer).
    pub fn budget_status(&self) -> Result<Vec<BudgetStatus>> {
        let week_start = now_micros() - WEEK_US;
        let spent: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(cost_microdollars),0) FROM cost_records
                 WHERE timestamp>=?1",
                [week_start],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);

        let mut stmt = self
            .conn
            .prepare("SELECT module_pattern, weekly_limit, alert_threshold FROM cost_budgets")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (pat, limit, thr) = row?;
            out.push(BudgetStatus {
                alerting: limit > 0 && spent as f64 >= limit as f64 * thr,
                module_pattern: pat,
                weekly_limit: limit,
                spent_this_week: spent,
                alert_threshold: thr,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs() -> (tempfile::TempDir, CostStore) {
        let d = tempfile::tempdir().unwrap();
        let gpp = d.path().join(".gpp");
        std::fs::create_dir_all(&gpp).unwrap();
        let s = CostStore::open(&gpp).unwrap();
        (d, s)
    }

    #[test]
    fn record_summarize_and_efficiency() {
        let (_d, s) = cs();
        s.record(&CostRecord {
            changeset_id: "cs1".into(),
            agent_id: "agent:a".into(),
            model_id: "opus".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_microdollars: 30_000,
            lines_changed: 120,
            timestamp: 1_000,
            ..Default::default()
        })
        .unwrap();
        s.set_lines_survived("cs1", 100).unwrap();

        let sum = s.summarize(&CostFilter::default()).unwrap();
        assert_eq!(sum.changesets, 1);
        assert_eq!(sum.cost_microdollars, 30_000);
        assert_eq!(sum.lines_survived, 100);
        assert_eq!(sum.cost_per_survived_line(), Some(300.0));
    }

    #[test]
    fn agent_filter_and_breakdown() {
        let (_d, s) = cs();
        for (i, a) in ["agent:a", "agent:b", "agent:a"].iter().enumerate() {
            s.record(&CostRecord {
                changeset_id: format!("cs{i}"),
                agent_id: (*a).into(),
                model_id: "m".into(),
                cost_microdollars: 10_000,
                timestamp: now_micros(),
                ..Default::default()
            })
            .unwrap();
        }
        let only_a = s
            .summarize(&CostFilter {
                agent: Some("agent:a".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(only_a.changesets, 2);
        assert_eq!(only_a.cost_microdollars, 20_000);
        assert_eq!(s.breakdown(&CostFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn budget_alerts_at_threshold() {
        let (_d, s) = cs();
        s.set_budget("**", 100_000, 0.8).unwrap();
        s.record(&CostRecord {
            changeset_id: "c".into(),
            agent_id: "a".into(),
            model_id: "m".into(),
            cost_microdollars: 85_000,
            timestamp: now_micros(),
            ..Default::default()
        })
        .unwrap();
        let b = s.budget_status().unwrap();
        assert_eq!(b.len(), 1);
        assert!(b[0].alerting, "85k >= 80% of 100k should alert");
    }
}
