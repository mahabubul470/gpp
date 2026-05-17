//! `gpp-review` — code review workflow (layer 14).
//!
//! A [`Review`] tracks one changeset through `pending → approved /
//! changes_requested / rejected → merged`. Reviewers record decisions;
//! status is recomputed from them (any reject ⇒ rejected; any
//! changes-requested ⇒ changes_requested; ≥1 approval & none blocking ⇒
//! approved). Comments target a file/line or are general. Reviewer
//! suggestions come from RBAC (owners/maintainers).
//!
//! See `docs/DATA_MODEL.md`, `docs/ROADMAP.md` (Phase 6).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gpp_core::Hash;
use gpp_rbac::{Role, RoleStore};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("review database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("rbac error: {0}")]
    Rbac(#[from] gpp_rbac::Error),
    #[error("no review for changeset {0:?}")]
    NotFound(String),
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
pub enum ReviewStatus {
    Pending,
    Approved,
    ChangesRequested,
    Rejected,
    Merged,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewStatus::Pending => "pending",
            ReviewStatus::Approved => "approved",
            ReviewStatus::ChangesRequested => "changes_requested",
            ReviewStatus::Rejected => "rejected",
            ReviewStatus::Merged => "merged",
        }
    }
    fn parse(s: &str) -> ReviewStatus {
        match s {
            "approved" => ReviewStatus::Approved,
            "changes_requested" => ReviewStatus::ChangesRequested,
            "rejected" => ReviewStatus::Rejected,
            "merged" => ReviewStatus::Merged,
            _ => ReviewStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    RequestChanges,
    Reject,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::RequestChanges => "request_changes",
            Decision::Reject => "reject",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Review {
    pub id: String,
    pub changeset: String,
    pub status: ReviewStatus,
    pub requested_by: String,
    pub requested_at: i64,
    pub merged_at: Option<i64>,
    pub merged_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub author_id: String,
    pub author_is_agent: bool,
    pub file_path: Option<String>,
    pub line_number: Option<i64>,
    pub body: String,
    pub created_at: i64,
}

pub struct ReviewStore {
    conn: Connection,
}

impl ReviewStore {
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("reviews");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("reviews.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS reviews (
                id TEXT PRIMARY KEY,
                changeset_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                requested_by TEXT NOT NULL,
                requested_at INTEGER NOT NULL,
                merged_at INTEGER,
                merged_by TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_reviews_changeset
                ON reviews(changeset_id);
             CREATE TABLE IF NOT EXISTS review_decisions (
                review_id TEXT NOT NULL,
                reviewer_id TEXT NOT NULL,
                reviewer_type TEXT NOT NULL,
                decision TEXT NOT NULL,
                reason TEXT,
                decided_at INTEGER NOT NULL,
                PRIMARY KEY (review_id, reviewer_id)
             );
             CREATE TABLE IF NOT EXISTS review_comments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                review_id TEXT NOT NULL,
                author_id TEXT NOT NULL,
                author_type TEXT NOT NULL,
                file_path TEXT,
                line_number INTEGER,
                body TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_review_comments_review
                ON review_comments(review_id);",
        )?;
        Ok(Self { conn })
    }

    fn review_id(changeset: &str, at: i64) -> String {
        Hash::of(format!("review:{changeset}:{at}").as_bytes()).to_base32()
    }

    /// Open a review for a changeset (idempotent: returns the existing one).
    pub fn request(&self, changeset: &str, requested_by: &str) -> Result<Review> {
        if let Some(r) = self.by_changeset(changeset)? {
            return Ok(r);
        }
        let at = now_micros();
        let id = Self::review_id(changeset, at);
        self.conn.execute(
            "INSERT INTO reviews
                (id, changeset_id, status, requested_by, requested_at)
             VALUES (?1,?2,'pending',?3,?4)",
            params![id, changeset, requested_by, at],
        )?;
        Ok(Review {
            id,
            changeset: changeset.to_string(),
            status: ReviewStatus::Pending,
            requested_by: requested_by.to_string(),
            requested_at: at,
            merged_at: None,
            merged_by: None,
        })
    }

    pub fn by_changeset(&self, changeset: &str) -> Result<Option<Review>> {
        self.conn
            .query_row(
                "SELECT id, changeset_id, status, requested_by, requested_at,
                        merged_at, merged_by
                 FROM reviews WHERE changeset_id=?1",
                [changeset],
                Self::row_to_review,
            )
            .optional()
            .map_err(Error::from)
    }

    fn row_to_review(r: &rusqlite::Row) -> rusqlite::Result<Review> {
        Ok(Review {
            id: r.get(0)?,
            changeset: r.get(1)?,
            status: ReviewStatus::parse(&r.get::<_, String>(2)?),
            requested_by: r.get(3)?,
            requested_at: r.get(4)?,
            merged_at: r.get(5)?,
            merged_by: r.get(6)?,
        })
    }

    pub fn list(&self, status: Option<ReviewStatus>) -> Result<Vec<Review>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, changeset_id, status, requested_by, requested_at,
                    merged_at, merged_by
             FROM reviews
             WHERE (?1 IS NULL OR status=?1)
             ORDER BY requested_at DESC",
        )?;
        let rows = stmt.query_map([status.map(|s| s.as_str())], Self::row_to_review)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Record a reviewer's decision and recompute review status.
    pub fn decide(
        &self,
        changeset: &str,
        reviewer_id: &str,
        reviewer_is_agent: bool,
        decision: Decision,
        reason: Option<&str>,
    ) -> Result<ReviewStatus> {
        let review = self
            .by_changeset(changeset)?
            .ok_or_else(|| Error::NotFound(changeset.to_string()))?;
        if review.status == ReviewStatus::Merged {
            return Err(Error::Other("review already merged".into()));
        }
        self.conn.execute(
            "INSERT OR REPLACE INTO review_decisions
                (review_id, reviewer_id, reviewer_type, decision, reason, decided_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                review.id,
                reviewer_id,
                if reviewer_is_agent { "agent" } else { "human" },
                decision.as_str(),
                reason,
                now_micros(),
            ],
        )?;
        let status = self.recompute(&review.id)?;
        self.conn.execute(
            "UPDATE reviews SET status=?1 WHERE id=?2",
            params![status.as_str(), review.id],
        )?;
        Ok(status)
    }

    /// `(approvals, has_human_approval)` for a review.
    pub fn approval_summary(&self, changeset: &str) -> Result<(u32, bool)> {
        let review = self
            .by_changeset(changeset)?
            .ok_or_else(|| Error::NotFound(changeset.to_string()))?;
        let approvals: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM review_decisions
             WHERE review_id=?1 AND decision='approve'",
            [&review.id],
            |r| r.get(0),
        )?;
        let human: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM review_decisions
             WHERE review_id=?1 AND decision='approve' AND reviewer_type='human'",
            [&review.id],
            |r| r.get(0),
        )?;
        Ok((approvals, human > 0))
    }

    fn recompute(&self, review_id: &str) -> Result<ReviewStatus> {
        let mut stmt = self
            .conn
            .prepare("SELECT decision FROM review_decisions WHERE review_id=?1")?;
        let decs: Vec<String> = stmt
            .query_map([review_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if decs.iter().any(|d| d == "reject") {
            return Ok(ReviewStatus::Rejected);
        }
        if decs.iter().any(|d| d == "request_changes") {
            return Ok(ReviewStatus::ChangesRequested);
        }
        if decs.iter().any(|d| d == "approve") {
            return Ok(ReviewStatus::Approved);
        }
        Ok(ReviewStatus::Pending)
    }

    pub fn comment(
        &self,
        changeset: &str,
        author_id: &str,
        author_is_agent: bool,
        file_path: Option<&str>,
        line: Option<i64>,
        body: &str,
    ) -> Result<()> {
        let review = self
            .by_changeset(changeset)?
            .ok_or_else(|| Error::NotFound(changeset.to_string()))?;
        self.conn.execute(
            "INSERT INTO review_comments
                (review_id, author_id, author_type, file_path, line_number,
                 body, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                review.id,
                author_id,
                if author_is_agent { "agent" } else { "human" },
                file_path,
                line,
                body,
                now_micros(),
            ],
        )?;
        Ok(())
    }

    pub fn comments(&self, changeset: &str) -> Result<Vec<Comment>> {
        let Some(review) = self.by_changeset(changeset)? else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT author_id, author_type, file_path, line_number, body, created_at
             FROM review_comments WHERE review_id=?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([review.id], |r| {
            Ok(Comment {
                author_id: r.get(0)?,
                author_is_agent: r.get::<_, String>(1)? == "agent",
                file_path: r.get(2)?,
                line_number: r.get(3)?,
                body: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Mark an approved review merged.
    pub fn merge(&self, changeset: &str, merged_by: &str) -> Result<()> {
        let review = self
            .by_changeset(changeset)?
            .ok_or_else(|| Error::NotFound(changeset.to_string()))?;
        if review.status != ReviewStatus::Approved {
            return Err(Error::Other(format!(
                "review is {} — only an approved review can be merged",
                review.status.as_str()
            )));
        }
        self.conn.execute(
            "UPDATE reviews SET status='merged', merged_at=?1, merged_by=?2 WHERE id=?3",
            params![now_micros(), merged_by, review.id],
        )?;
        Ok(())
    }

    /// Suggested reviewers: current owners and maintainers (RBAC).
    pub fn suggest_reviewers(roles: &RoleStore) -> Result<Vec<String>> {
        Ok(roles
            .list()?
            .into_iter()
            .filter(|a| a.role >= Role::Maintainer)
            .map(|a| a.identity)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rstore() -> (tempfile::TempDir, ReviewStore) {
        let d = tempfile::tempdir().unwrap();
        let g = d.path().join(".gpp");
        std::fs::create_dir_all(&g).unwrap();
        (d, ReviewStore::open(&g).unwrap())
    }

    #[test]
    fn lifecycle_request_approve_merge() {
        let (_d, s) = rstore();
        let r = s.request("cs:abc", "dev@x.io").unwrap();
        assert_eq!(r.status, ReviewStatus::Pending);
        // idempotent
        assert_eq!(s.request("cs:abc", "dev@x.io").unwrap().id, r.id);

        assert_eq!(
            s.decide("cs:abc", "lead@x.io", false, Decision::Approve, None)
                .unwrap(),
            ReviewStatus::Approved
        );
        let (n, human) = s.approval_summary("cs:abc").unwrap();
        assert_eq!(n, 1);
        assert!(human);

        s.merge("cs:abc", "lead@x.io").unwrap();
        assert_eq!(
            s.by_changeset("cs:abc").unwrap().unwrap().status,
            ReviewStatus::Merged
        );
        // Cannot decide on a merged review.
        assert!(
            s.decide("cs:abc", "x", false, Decision::Approve, None)
                .is_err()
        );
    }

    #[test]
    fn reject_and_changes_block_merge() {
        let (_d, s) = rstore();
        s.request("cs:z", "dev").unwrap();
        s.decide("cs:z", "r1", false, Decision::Approve, None)
            .unwrap();
        let st = s
            .decide("cs:z", "r2", true, Decision::RequestChanges, Some("fix"))
            .unwrap();
        assert_eq!(st, ReviewStatus::ChangesRequested);
        assert!(s.merge("cs:z", "lead").is_err());

        let st = s
            .decide("cs:z", "r3", false, Decision::Reject, Some("no"))
            .unwrap();
        assert_eq!(st, ReviewStatus::Rejected);

        s.comment("cs:z", "r3", false, Some("a.rs"), Some(10), "here")
            .unwrap();
        assert_eq!(s.comments("cs:z").unwrap().len(), 1);
        assert_eq!(s.list(Some(ReviewStatus::Rejected)).unwrap().len(), 1);
    }

    #[test]
    fn reviewer_suggestion_from_rbac() {
        let d = tempfile::tempdir().unwrap();
        let g = d.path().join(".gpp");
        std::fs::create_dir_all(&g).unwrap();
        let roles = RoleStore::open(&g).unwrap();
        roles
            .assign("lead@x.io", Role::Maintainer, "o", None, None)
            .unwrap();
        roles
            .assign("dev@x.io", Role::Contributor, "o", None, None)
            .unwrap();
        let s = ReviewStore::suggest_reviewers(&roles).unwrap();
        assert_eq!(s, vec!["lead@x.io"]);
    }
}
