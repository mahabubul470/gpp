//! `gpp-rbac` — human permission model (layer 15).
//!
//! Four roles, ordered `Reader < Contributor < Maintainer < Owner`. Roles
//! may expire. Branch-protection rules (glob → min reviewers / require human
//! / min role / allow-agent-merge) gate merges. Every role change is logged
//! to `role_history` (versioned alongside changesets).
//!
//! See `docs/DATA_MODEL.md`, `docs/SECURITY_MODEL.md`, `docs/ROADMAP.md`
//! (Phase 6).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use globset::Glob;
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("rbac database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("unknown role {0:?}")]
    UnknownRole(String),
    #[error("no role assigned to {0:?}")]
    NoRole(String),
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

/// Ordered so `>=` expresses "at least this privileged".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Reader,
    Contributor,
    Maintainer,
    Owner,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Reader => "reader",
            Role::Contributor => "contributor",
            Role::Maintainer => "maintainer",
            Role::Owner => "owner",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "reader" => Role::Reader,
            "contributor" => Role::Contributor,
            "maintainer" => Role::Maintainer,
            "owner" => Role::Owner,
            other => return Err(Error::UnknownRole(other.to_string())),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Assignment {
    pub identity: String,
    pub role: Role,
    pub assigned_by: String,
    pub assigned_at: i64,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct BranchProtection {
    pub branch_pattern: String,
    pub min_reviewers: u32,
    pub require_human: bool,
    pub require_role: Role,
    pub require_policy_pass: bool,
    pub allow_agent_merge: bool,
}

/// What a caller knows about a pending merge, checked against protection.
#[derive(Debug, Clone, Default)]
pub struct MergeRequest {
    pub branch: String,
    pub merger_identity: String,
    pub merger_is_agent: bool,
    pub approvals: u32,
    pub has_human_approval: bool,
    pub policies_passed: bool,
}

/// One `role_history` row: `(changed_at_us, old_role, new_role, changed_by)`.
pub type RoleChange = (i64, Option<String>, String, String);

pub struct RoleStore {
    conn: Connection,
}

impl RoleStore {
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("rbac");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("rbac.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS roles (
                identity TEXT PRIMARY KEY,
                role TEXT NOT NULL DEFAULT 'reader',
                assigned_by TEXT NOT NULL,
                assigned_at INTEGER NOT NULL,
                expires_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS role_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                identity TEXT NOT NULL,
                old_role TEXT,
                new_role TEXT NOT NULL,
                changed_by TEXT NOT NULL,
                changed_at INTEGER NOT NULL,
                reason TEXT,
                changeset_id TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_role_history_identity
                ON role_history(identity);
             CREATE TABLE IF NOT EXISTS branch_protections (
                branch_pattern TEXT PRIMARY KEY,
                min_reviewers INTEGER NOT NULL DEFAULT 1,
                require_human INTEGER NOT NULL DEFAULT 1,
                require_role TEXT NOT NULL DEFAULT 'maintainer',
                require_policy INTEGER NOT NULL DEFAULT 1,
                allow_agent_merge INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn assign(
        &self,
        identity: &str,
        role: Role,
        assigned_by: &str,
        reason: Option<&str>,
        expires_at: Option<i64>,
    ) -> Result<()> {
        let old: Option<String> = self
            .conn
            .query_row(
                "SELECT role FROM roles WHERE identity=?1",
                [identity],
                |r| r.get(0),
            )
            .optional()?;
        let now = now_micros();
        self.conn.execute(
            "INSERT INTO roles (identity, role, assigned_by, assigned_at, expires_at)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(identity) DO UPDATE SET
                role=excluded.role, assigned_by=excluded.assigned_by,
                assigned_at=excluded.assigned_at, expires_at=excluded.expires_at",
            params![identity, role.as_str(), assigned_by, now, expires_at],
        )?;
        self.conn.execute(
            "INSERT INTO role_history
                (identity, old_role, new_role, changed_by, changed_at, reason)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![identity, old, role.as_str(), assigned_by, now, reason],
        )?;
        Ok(())
    }

    pub fn revoke(&self, identity: &str, by: &str) -> Result<()> {
        let old: Option<String> = self
            .conn
            .query_row(
                "SELECT role FROM roles WHERE identity=?1",
                [identity],
                |r| r.get(0),
            )
            .optional()?;
        if old.is_none() {
            return Err(Error::NoRole(identity.to_string()));
        }
        self.conn
            .execute("DELETE FROM roles WHERE identity=?1", [identity])?;
        self.conn.execute(
            "INSERT INTO role_history
                (identity, old_role, new_role, changed_by, changed_at, reason)
             VALUES (?1,?2,'(revoked)',?3,?4,'revoked')",
            params![identity, old, by, now_micros()],
        )?;
        Ok(())
    }

    /// Effective role: the assigned role unless expired (then `Reader`).
    pub fn role_of(&self, identity: &str) -> Result<Role> {
        let row: Option<(String, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT role, expires_at FROM roles WHERE identity=?1",
                [identity],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(Role::Reader),
            Some((_, Some(exp))) if exp <= now_micros() => Ok(Role::Reader),
            Some((r, _)) => Role::parse(&r),
        }
    }

    pub fn list(&self) -> Result<Vec<Assignment>> {
        let mut stmt = self.conn.prepare(
            "SELECT identity, role, assigned_by, assigned_at, expires_at
             FROM roles ORDER BY role DESC, identity",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Assignment {
                identity: r.get(0)?,
                role: Role::parse(&r.get::<_, String>(1)?).unwrap_or(Role::Reader),
                assigned_by: r.get(2)?,
                assigned_at: r.get(3)?,
                expires_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn history(&self, identity: &str) -> Result<Vec<RoleChange>> {
        let mut stmt = self.conn.prepare(
            "SELECT changed_at, old_role, new_role, changed_by FROM role_history
             WHERE identity=?1 ORDER BY changed_at DESC",
        )?;
        let rows = stmt.query_map([identity], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn protect(&self, p: &BranchProtection) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO branch_protections
                (branch_pattern, min_reviewers, require_human, require_role,
                 require_policy, allow_agent_merge)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                p.branch_pattern,
                p.min_reviewers,
                p.require_human as i64,
                p.require_role.as_str(),
                p.require_policy_pass as i64,
                p.allow_agent_merge as i64,
            ],
        )?;
        Ok(())
    }

    pub fn protections(&self) -> Result<Vec<BranchProtection>> {
        let mut stmt = self.conn.prepare(
            "SELECT branch_pattern, min_reviewers, require_human, require_role,
                    require_policy, allow_agent_merge FROM branch_protections",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(BranchProtection {
                branch_pattern: r.get(0)?,
                min_reviewers: r.get(1)?,
                require_human: r.get::<_, i64>(2)? != 0,
                require_role: Role::parse(&r.get::<_, String>(3)?).unwrap_or(Role::Maintainer),
                require_policy_pass: r.get::<_, i64>(4)? != 0,
                allow_agent_merge: r.get::<_, i64>(5)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The protection rule matching `branch` (first glob match), if any.
    pub fn protection_for(&self, branch: &str) -> Result<Option<BranchProtection>> {
        for p in self.protections()? {
            if let Ok(g) = Glob::new(&p.branch_pattern)
                && g.compile_matcher().is_match(branch)
            {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    /// Decide whether a merge is allowed. `Ok(())` = allowed; `Err` explains
    /// the first unmet requirement.
    pub fn can_merge(&self, req: &MergeRequest) -> Result<()> {
        let Some(p) = self.protection_for(&req.branch)? else {
            return Ok(()); // unprotected branch
        };
        if req.merger_is_agent && !p.allow_agent_merge {
            return Err(Error::Other(format!(
                "branch {:?} does not allow agent merges",
                req.branch
            )));
        }
        let role = self.role_of(&req.merger_identity)?;
        if role < p.require_role {
            return Err(Error::Other(format!(
                "{} is {} but {:?} requires {}",
                req.merger_identity,
                role.as_str(),
                req.branch,
                p.require_role.as_str()
            )));
        }
        if req.approvals < p.min_reviewers {
            return Err(Error::Other(format!(
                "{} approval(s), {:?} requires {}",
                req.approvals, req.branch, p.min_reviewers
            )));
        }
        if p.require_human && !req.has_human_approval {
            return Err(Error::Other(format!(
                "branch {:?} requires a human approval",
                req.branch
            )));
        }
        if p.require_policy_pass && !req.policies_passed {
            return Err(Error::Other(format!(
                "branch {:?} requires all policies to pass",
                req.branch
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs() -> (tempfile::TempDir, RoleStore) {
        let d = tempfile::tempdir().unwrap();
        let g = d.path().join(".gpp");
        std::fs::create_dir_all(&g).unwrap();
        (d, RoleStore::open(&g).unwrap())
    }

    #[test]
    fn assign_revoke_and_expiry() {
        let (_d, s) = rs();
        assert_eq!(s.role_of("a@x.io").unwrap(), Role::Reader); // default
        s.assign("a@x.io", Role::Maintainer, "owner@x.io", Some("lead"), None)
            .unwrap();
        assert_eq!(s.role_of("a@x.io").unwrap(), Role::Maintainer);

        // Expired assignment falls back to Reader.
        s.assign("b@x.io", Role::Owner, "owner@x.io", None, Some(1))
            .unwrap();
        assert_eq!(s.role_of("b@x.io").unwrap(), Role::Reader);

        s.revoke("a@x.io", "owner@x.io").unwrap();
        assert_eq!(s.role_of("a@x.io").unwrap(), Role::Reader);
        assert!(s.history("a@x.io").unwrap().len() >= 2);
        assert!(s.revoke("nobody", "x").is_err());
    }

    #[test]
    fn branch_protection_gates_merge() {
        let (_d, s) = rs();
        s.assign("dev@x.io", Role::Contributor, "o", None, None)
            .unwrap();
        s.assign("lead@x.io", Role::Maintainer, "o", None, None)
            .unwrap();
        s.protect(&BranchProtection {
            branch_pattern: "main".into(),
            min_reviewers: 1,
            require_human: true,
            require_role: Role::Maintainer,
            require_policy_pass: true,
            allow_agent_merge: false,
        })
        .unwrap();

        let mut req = MergeRequest {
            branch: "main".into(),
            merger_identity: "dev@x.io".into(),
            approvals: 1,
            has_human_approval: true,
            policies_passed: true,
            ..Default::default()
        };
        assert!(s.can_merge(&req).is_err()); // contributor < maintainer

        req.merger_identity = "lead@x.io".into();
        assert!(s.can_merge(&req).is_ok());

        req.merger_is_agent = true;
        assert!(s.can_merge(&req).is_err()); // agent merge disallowed

        let free = MergeRequest {
            branch: "scratch".into(),
            merger_identity: "dev@x.io".into(),
            ..Default::default()
        };
        assert!(s.can_merge(&free).is_ok()); // unprotected
    }
}
