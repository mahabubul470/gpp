//! `gpp-notify` — event system, inbox, and integration backends (layer 16).
//!
//! Typed [`EventType`]s are recorded in an append-only `events` table and
//! fan out to per-recipient `notifications` (the inbox). Configured
//! integration backends (webhook/slack/discord/email) each subscribe to a
//! set of event types; [`Notifier::dispatch`] delivers undispatched events,
//! HMAC-signing outgoing webhooks and logging every attempt.
//!
//! See `docs/DATA_MODEL.md`, `docs/ROADMAP.md` (Phase 6).
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rusqlite::{Connection, params};
use sha2::Sha256;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("notify database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("http error: {0}")]
    Http(String),
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

/// Typed events (`docs/DATA_MODEL.md` § Notification Object).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    ChangesetPromoted,
    ReviewRequested,
    ReviewApproved,
    ReviewRejected,
    ReviewChangesRequested,
    PolicyViolation,
    TrustScoreChanged,
    TrustAgentBlocked,
    AnomalyDetected,
    SyncConflict,
    GraphexUpdateProposed,
    CostBudgetAlert,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::ChangesetPromoted => "changeset.promoted",
            EventType::ReviewRequested => "review.requested",
            EventType::ReviewApproved => "review.approved",
            EventType::ReviewRejected => "review.rejected",
            EventType::ReviewChangesRequested => "review.changes_requested",
            EventType::PolicyViolation => "policy.violation",
            EventType::TrustScoreChanged => "trust.score_changed",
            EventType::TrustAgentBlocked => "trust.agent_blocked",
            EventType::AnomalyDetected => "anomaly.detected",
            EventType::SyncConflict => "sync.conflict",
            EventType::GraphexUpdateProposed => "graphex.update_proposed",
            EventType::CostBudgetAlert => "cost.budget_alert",
        }
    }
}

/// All subscribable event-type strings (for `gpp notify events`).
pub fn event_catalog() -> Vec<&'static str> {
    use EventType::*;
    [
        ChangesetPromoted,
        ReviewRequested,
        ReviewApproved,
        ReviewRejected,
        ReviewChangesRequested,
        PolicyViolation,
        TrustScoreChanged,
        TrustAgentBlocked,
        AnomalyDetected,
        SyncConflict,
        GraphexUpdateProposed,
        CostBudgetAlert,
    ]
    .iter()
    .map(|e| e.as_str())
    .collect()
}

#[derive(Debug, Clone)]
pub struct InboxItem {
    pub notif_id: i64,
    pub event_type: String,
    pub summary: String,
    pub timestamp: i64,
    pub read: bool,
}

/// Abstracts the outbound HTTP call so dispatch is unit-testable offline.
pub trait Sender {
    /// Deliver `payload` to `url`. `signature` is the hex HMAC-SHA256 for
    /// webhook backends (empty otherwise). Returns a short status string.
    fn send(&self, backend: &str, url: &str, payload: &str, signature: &str) -> Result<String>;
}

/// Real backend: blocking `reqwest` POST with `X-Gpp-Signature`.
pub struct HttpSender;

impl Sender for HttpSender {
    fn send(&self, _backend: &str, url: &str, payload: &str, signature: &str) -> Result<String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Gpp-Signature", format!("sha256={signature}"))
            .body(payload.to_string())
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(format!("HTTP {}", resp.status().as_u16()))
    }
}

fn sign(secret: &str, payload: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub struct Notifier {
    conn: Connection,
}

impl Notifier {
    pub fn open(gpp_dir: &Path) -> Result<Self> {
        let dir = gpp_dir.join("notify");
        std::fs::create_dir_all(&dir).map_err(|e| Error::Other(e.to_string()))?;
        let conn = Connection::open(dir.join("notify.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                actor_id TEXT NOT NULL,
                actor_type TEXT NOT NULL,
                target_type TEXT NOT NULL,
                target_id TEXT NOT NULL,
                summary TEXT NOT NULL,
                details TEXT,
                dispatched INTEGER NOT NULL DEFAULT 0
             );
             CREATE INDEX IF NOT EXISTS idx_events_undispatched
                ON events(dispatched);
             CREATE TABLE IF NOT EXISTS notifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id INTEGER NOT NULL,
                recipient_id TEXT NOT NULL,
                read INTEGER NOT NULL DEFAULT 0,
                read_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_notifications_recipient
                ON notifications(recipient_id);
             CREATE TABLE IF NOT EXISTS integrations (
                backend TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                secret TEXT,
                events TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS integration_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id INTEGER NOT NULL,
                backend TEXT NOT NULL,
                status TEXT NOT NULL,
                response TEXT,
                sent_at INTEGER NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    /// Emit an event and fan it out to `recipients`' inboxes.
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &self,
        event_type: EventType,
        actor_id: &str,
        actor_is_agent: bool,
        target_type: &str,
        target_id: &str,
        summary: &str,
        recipients: &[String],
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO events
                (event_type, timestamp, actor_id, actor_type, target_type,
                 target_id, summary, dispatched)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0)",
            params![
                event_type.as_str(),
                now_micros(),
                actor_id,
                if actor_is_agent { "agent" } else { "human" },
                target_type,
                target_id,
                summary,
            ],
        )?;
        let event_id = self.conn.last_insert_rowid();
        for r in recipients {
            self.conn.execute(
                "INSERT INTO notifications (event_id, recipient_id) VALUES (?1,?2)",
                params![event_id, r],
            )?;
        }
        Ok(event_id)
    }

    pub fn add_integration(
        &self,
        backend: &str,
        url: &str,
        secret: Option<&str>,
        events: &[&str],
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO integrations (backend, url, secret, events)
             VALUES (?1,?2,?3,?4)",
            params![backend, url, secret, events.join(",")],
        )?;
        Ok(())
    }

    pub fn remove_integration(&self, backend: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM integrations WHERE backend=?1", [backend])?;
        Ok(())
    }

    pub fn integrations(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT backend, url, events FROM integrations ORDER BY backend")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Deliver every undispatched event to subscribed backends.
    pub fn dispatch(&self, sender: &dyn Sender) -> Result<usize> {
        let events: Vec<(i64, String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, event_type, summary FROM events
                 WHERE dispatched=0 ORDER BY id",
            )?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let integrations: Vec<(String, String, Option<String>, String)> = {
            let mut s = self
                .conn
                .prepare("SELECT backend, url, secret, events FROM integrations")?;
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut delivered = 0;
        for (eid, etype, summary) in &events {
            for (backend, url, secret, subs) in &integrations {
                let subscribed = subs == "*" || subs.split(',').any(|s| s.trim() == etype);
                if !subscribed {
                    continue;
                }
                let payload = serde_json::json!({
                    "event": etype, "summary": summary, "id": eid,
                })
                .to_string();
                let signature = secret
                    .as_deref()
                    .map(|s| sign(s, &payload))
                    .unwrap_or_default();
                let (status, resp) = match sender.send(backend, url, &payload, &signature) {
                    Ok(r) => ("sent", r),
                    Err(e) => ("failed", e.to_string()),
                };
                self.conn.execute(
                    "INSERT INTO integration_log
                        (event_id, backend, status, response, sent_at)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![eid, backend, status, resp, now_micros()],
                )?;
                if status == "sent" {
                    delivered += 1;
                }
            }
            self.conn
                .execute("UPDATE events SET dispatched=1 WHERE id=?1", [eid])?;
        }
        Ok(delivered)
    }

    // ---- inbox -----------------------------------------------------------

    pub fn inbox(&self, recipient: &str, limit: usize) -> Result<Vec<InboxItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, e.event_type, e.summary, e.timestamp, n.read
             FROM notifications n JOIN events e ON e.id=n.event_id
             WHERE n.recipient_id=?1
             ORDER BY e.timestamp DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![recipient, limit as i64], |r| {
            Ok(InboxItem {
                notif_id: r.get(0)?,
                event_type: r.get(1)?,
                summary: r.get(2)?,
                timestamp: r.get(3)?,
                read: r.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn unread_count(&self, recipient: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE recipient_id=?1 AND read=0",
            [recipient],
            |r| r.get(0),
        )?)
    }

    pub fn ack(&self, notif_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE notifications SET read=1, read_at=?1 WHERE id=?2",
            params![now_micros(), notif_id],
        )?;
        Ok(())
    }

    pub fn ack_all(&self, recipient: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE notifications SET read=1, read_at=?1
             WHERE recipient_id=?2 AND read=0",
            params![now_micros(), recipient],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSender {
        calls: Mutex<Vec<(String, String, String)>>, // (backend, payload, signature)
    }
    impl Sender for RecordingSender {
        fn send(
            &self,
            backend: &str,
            _url: &str,
            payload: &str,
            signature: &str,
        ) -> Result<String> {
            self.calls.lock().unwrap().push((
                backend.to_string(),
                payload.to_string(),
                signature.to_string(),
            ));
            Ok("HTTP 200".into())
        }
    }

    fn nf() -> (tempfile::TempDir, Notifier) {
        let d = tempfile::tempdir().unwrap();
        let g = d.path().join(".gpp");
        std::fs::create_dir_all(&g).unwrap();
        (d, Notifier::open(&g).unwrap())
    }

    #[test]
    fn emit_inbox_and_ack() {
        let (_d, n) = nf();
        n.emit(
            EventType::ChangesetPromoted,
            "dev@x.io",
            false,
            "changeset",
            "cs:abc",
            "promoted cs:abc",
            &["lead@x.io".into(), "qa@x.io".into()],
        )
        .unwrap();
        assert_eq!(n.unread_count("lead@x.io").unwrap(), 1);
        let inbox = n.inbox("lead@x.io", 10).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].event_type, "changeset.promoted");
        n.ack(inbox[0].notif_id).unwrap();
        assert_eq!(n.unread_count("lead@x.io").unwrap(), 0);
        assert_eq!(n.ack_all("qa@x.io").unwrap(), 1);
    }

    #[test]
    fn dispatch_filters_by_subscription_and_signs() {
        let (_d, n) = nf();
        n.add_integration(
            "webhook",
            "https://ci.example/hook",
            Some("topsecret"),
            &["changeset.promoted"],
        )
        .unwrap();
        n.add_integration(
            "slack",
            "https://hooks.slack/x",
            None,
            &["review.requested"],
        )
        .unwrap();

        n.emit(
            EventType::ChangesetPromoted,
            "dev",
            false,
            "changeset",
            "cs:1",
            "promoted",
            &[],
        )
        .unwrap();

        let rec = RecordingSender::default();
        let delivered = n.dispatch(&rec).unwrap();
        assert_eq!(delivered, 1); // only the subscribed webhook
        let calls = rec.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "webhook");
        assert_eq!(calls[0].2, sign("topsecret", &calls[0].1)); // valid HMAC

        drop(calls);
        // Re-dispatch is a no-op (event already dispatched).
        assert_eq!(n.dispatch(&rec).unwrap(), 0);
    }
}
