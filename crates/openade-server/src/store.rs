//! SQLite persistence for the workspace server.
//!
//! Every row is org-scoped so the same binary can serve one org
//! (self-hosted, the default) or many (a hosted deployment) without a
//! schema change. Sessions are stored **harness-neutral only**: OpenADE's
//! transcript event JSON + artifact markdown + session metadata — never a
//! vendor CLI's native session file, which cannot cross machines or
//! harnesses.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::signal::{DismissReason, OutcomeKind, SignalIn};

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
}

/// A member token (the secret itself is only returned at mint time).
#[derive(Debug, Clone, Serialize)]
pub struct TokenInfo {
    pub id: i64,
    pub name: String,
}

/// A workspace: the shared knowledge hub sessions are uploaded to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub created_at: String,
}

/// An uploaded session record (harness-neutral).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSession {
    pub id: i64,
    pub workspace_id: i64,
    pub title: String,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub summary: String,
    /// Shared-by display name (the member token's name).
    pub shared_by: String,
    pub uploaded_at: String,
    /// What reality decided about this session's work, once known
    /// (`merged` / `closed` / `reverted`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

/// A stored signal (the normalized record behind an inbox item).
#[derive(Debug, Clone, Serialize)]
pub struct StoredSignal {
    pub id: i64,
    pub source: String,
    pub source_ref: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub evidence: serde_json::Value,
    pub fingerprint: String,
    pub join_keys: serde_json::Value,
    pub affected_count: Option<i64>,
    pub first_seen: String,
    pub last_seen: String,
}

/// An inbox item: the triage unit (v1: one signal fingerprint → one item).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: i64,
    pub fingerprint: String,
    pub title: String,
    pub severity: String,
    #[serde(default)]
    pub summary: String,
    /// `new` | `accepted` | `dismissed`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismiss_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Users/accounts impacted (summed across the item's signals).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected_count: Option<i64>,
    pub last_seen: String,
}

/// Full detail for one inbox item: signals + outcome history.
#[derive(Debug, Clone, Serialize)]
pub struct InboxItemDetail {
    pub item: InboxItem,
    pub signals: Vec<StoredSignal>,
    /// Prior outcomes anchored to the item's fingerprints, newest first.
    pub outcomes: Vec<OutcomeRecord>,
}

/// What reality decided about an inbox item's work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub item_id: i64,
    pub kind: String,
    pub occurred_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// What one ingested signal did to the inbox.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct IngestOutcome {
    /// True on first sight of the fingerprint; false on a recurrence.
    pub inserted: bool,
    pub item_id: i64,
    /// True when the recurrence reopened a dismissed item (impact grew).
    pub escalated: bool,
}

/// Full session detail: the listing row plus content.
#[derive(Debug, Clone, Serialize)]
pub struct SharedSessionDetail {
    #[serde(flatten)]
    pub session: SharedSession,
    /// Knowledge artifact markdown.
    pub markdown: String,
    /// Harness-neutral transcript events (JSON array).
    pub events: serde_json::Value,
}

/// The workspace store, org-scoped on every query.
pub struct Store {
    conn: std::sync::Mutex<Connection>,
}

/// The single org a self-hosted server serves.
pub const DEFAULT_ORG: i64 = 1;

impl Store {
    /// Open (and migrate) the database at `data_dir/server.db`.
    pub fn open(data_dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(data_dir).ok();
        let conn = Connection::open(data_dir.join("server.db"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS orgs (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL
             );
             INSERT OR IGNORE INTO orgs (id, name) VALUES (1, 'default');
             CREATE TABLE IF NOT EXISTS tokens (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                token TEXT NOT NULL UNIQUE,
                revoked INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS workspaces (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                workspace_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                harness TEXT NOT NULL,
                entity_ref TEXT,
                branch TEXT,
                summary TEXT NOT NULL,
                markdown TEXT NOT NULL,
                events TEXT NOT NULL,
                shared_by TEXT NOT NULL,
                uploaded_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                source TEXT NOT NULL,
                source_ref TEXT NOT NULL DEFAULT '',
                kind TEXT NOT NULL,
                severity TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL DEFAULT '',
                evidence TEXT NOT NULL DEFAULT '[]',
                fingerprint TEXT NOT NULL,
                join_keys TEXT NOT NULL DEFAULT '{}',
                raw TEXT NOT NULL DEFAULT 'null',
                affected_count INTEGER,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                UNIQUE (org_id, fingerprint)
             );
             CREATE TABLE IF NOT EXISTS inbox_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                fingerprint TEXT NOT NULL,
                title TEXT NOT NULL,
                severity TEXT NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'new',
                dismiss_reason TEXT,
                dismissal_affected_count INTEGER,
                decided_by TEXT,
                decided_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE (org_id, fingerprint)
             );
             CREATE TABLE IF NOT EXISTS item_signals (
                org_id INTEGER NOT NULL,
                item_id INTEGER NOT NULL,
                fingerprint TEXT NOT NULL,
                PRIMARY KEY (org_id, item_id, fingerprint)
             );
             CREATE TABLE IF NOT EXISTS outcomes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                org_id INTEGER NOT NULL,
                item_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                pr_url TEXT,
                note TEXT,
                UNIQUE (org_id, item_id, kind)
             );",
        )?;
        // Additive column for pre-existing databases; `ALTER TABLE` is not
        // idempotent, so tolerate exactly the duplicate-column error.
        if let Err(e) = conn.execute("ALTER TABLE sessions ADD COLUMN verdict TEXT", []) {
            let benign = e.to_string().contains("duplicate column name");
            if !benign {
                return Err(e.into());
            }
        }
        Ok(Store {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Mint a member token; returns (id, secret).
    pub fn mint_token(&self, org: i64, name: &str) -> Result<(i64, String), StoreError> {
        let secret = format!("oadk_{}", uuid::Uuid::new_v4().simple());
        let conn = self.lock();
        conn.execute(
            "INSERT INTO tokens (org_id, name, token) VALUES (?1, ?2, ?3)",
            rusqlite::params![org, name, secret],
        )?;
        Ok((conn.last_insert_rowid(), secret))
    }

    /// Revoke a token by id.
    pub fn revoke_token(&self, org: i64, id: i64) -> Result<(), StoreError> {
        let n = self.lock().execute(
            "UPDATE tokens SET revoked = 1 WHERE id = ?1 AND org_id = ?2 AND revoked = 0",
            rusqlite::params![id, org],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Resolve a bearer secret to (org, member name); revoked/unknown → None.
    pub fn member_for_token(&self, secret: &str) -> Result<Option<(i64, String)>, StoreError> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT org_id, name FROM tokens WHERE token = ?1 AND revoked = 0")?;
        let mut rows = stmt.query([secret])?;
        match rows.next()? {
            Some(row) => Ok(Some((row.get(0)?, row.get(1)?))),
            None => Ok(None),
        }
    }

    /// Tokens for an org (secrets omitted).
    pub fn tokens(&self, org: i64) -> Result<Vec<TokenInfo>, StoreError> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare("SELECT id, name FROM tokens WHERE org_id = ?1 AND revoked = 0")?;
        let rows = stmt
            .query_map([org], |r| {
                Ok(TokenInfo {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Create a workspace.
    pub fn create_workspace(
        &self,
        org: i64,
        title: &str,
        description: &str,
    ) -> Result<Workspace, StoreError> {
        let created_at = chrono::Utc::now().to_rfc3339();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO workspaces (org_id, title, description, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![org, title, description, created_at],
        )?;
        Ok(Workspace {
            id: conn.last_insert_rowid(),
            title: title.to_string(),
            description: description.to_string(),
            created_at,
        })
    }

    /// Workspaces in an org.
    pub fn workspaces(&self, org: i64) -> Result<Vec<Workspace>, StoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, description, created_at FROM workspaces
             WHERE org_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([org], |r| {
                Ok(Workspace {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// One workspace.
    pub fn workspace(&self, org: i64, id: i64) -> Result<Workspace, StoreError> {
        self.workspaces(org)?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or(StoreError::NotFound)
    }

    /// Upload a session record into a workspace.
    #[allow(clippy::too_many_arguments)]
    pub fn upload_session(
        &self,
        org: i64,
        workspace_id: i64,
        title: &str,
        harness: &str,
        entity_ref: Option<&str>,
        branch: Option<&str>,
        summary: &str,
        markdown: &str,
        events: &serde_json::Value,
        shared_by: &str,
    ) -> Result<SharedSession, StoreError> {
        // Workspace must exist in this org.
        self.workspace(org, workspace_id)?;
        let uploaded_at = chrono::Utc::now().to_rfc3339();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO sessions (org_id, workspace_id, title, harness, entity_ref, branch,
                                   summary, markdown, events, shared_by, uploaded_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                org,
                workspace_id,
                title,
                harness,
                entity_ref,
                branch,
                summary,
                markdown,
                events.to_string(),
                shared_by,
                uploaded_at
            ],
        )?;
        Ok(SharedSession {
            id: conn.last_insert_rowid(),
            workspace_id,
            title: title.to_string(),
            harness: harness.to_string(),
            entity_ref: entity_ref.map(str::to_string),
            branch: branch.map(str::to_string),
            summary: summary.to_string(),
            shared_by: shared_by.to_string(),
            uploaded_at,
            verdict: None,
        })
    }

    /// Sessions in a workspace, newest first, optionally entity-filtered.
    pub fn sessions(
        &self,
        org: i64,
        workspace_id: i64,
        entity: Option<&str>,
    ) -> Result<Vec<SharedSession>, StoreError> {
        self.workspace(org, workspace_id)?;
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, title, harness, entity_ref, branch, summary,
                    shared_by, uploaded_at, verdict
             FROM sessions
             WHERE org_id = ?1 AND workspace_id = ?2
               AND (?3 IS NULL OR entity_ref = ?3)
             ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![org, workspace_id, entity], |r| {
                Ok(SharedSession {
                    id: r.get(0)?,
                    workspace_id: r.get(1)?,
                    title: r.get(2)?,
                    harness: r.get(3)?,
                    entity_ref: r.get(4)?,
                    branch: r.get(5)?,
                    summary: r.get(6)?,
                    shared_by: r.get(7)?,
                    uploaded_at: r.get(8)?,
                    verdict: r.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Full detail for one shared session.
    pub fn session(&self, org: i64, id: i64) -> Result<SharedSessionDetail, StoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, title, harness, entity_ref, branch, summary,
                    shared_by, uploaded_at, markdown, events, verdict
             FROM sessions WHERE org_id = ?1 AND id = ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![org, id])?;
        let Some(r) = rows.next()? else {
            return Err(StoreError::NotFound);
        };
        let events_text: String = r.get(10)?;
        Ok(SharedSessionDetail {
            session: SharedSession {
                id: r.get(0)?,
                workspace_id: r.get(1)?,
                title: r.get(2)?,
                harness: r.get(3)?,
                entity_ref: r.get(4)?,
                branch: r.get(5)?,
                summary: r.get(6)?,
                shared_by: r.get(7)?,
                uploaded_at: r.get(8)?,
                verdict: r.get(11)?,
            },
            markdown: r.get(9)?,
            events: serde_json::from_str(&events_text)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        })
    }

    /// Record what reality decided about a shared session.
    pub fn set_session_verdict(&self, org: i64, id: i64, verdict: &str) -> Result<(), StoreError> {
        let n = self.lock().execute(
            "UPDATE sessions SET verdict = ?3 WHERE org_id = ?1 AND id = ?2",
            rusqlite::params![org, id, verdict],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Ingest one normalized signal: dedup on fingerprint (a recurrence
    /// bumps `last_seen` / severity / `affected_count`), keep the inbox
    /// item in sync, and reopen a dismissed item whose impact grew ≥3×
    /// the dismissal-time snapshot (Merge0's escalation rule).
    pub fn ingest_signal(&self, org: i64, sig: &SignalIn) -> Result<IngestOutcome, StoreError> {
        let fp = sig.effective_fingerprint();
        let now = chrono::Utc::now().to_rfc3339();
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let inserted = {
            let n = tx.execute(
                "INSERT INTO signals (org_id, source, source_ref, kind, severity, title, body,
                                      evidence, fingerprint, join_keys, raw, affected_count,
                                      first_seen, last_seen)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)
                 ON CONFLICT (org_id, fingerprint) DO UPDATE SET
                    last_seen = excluded.last_seen,
                    severity = excluded.severity,
                    title = excluded.title,
                    body = excluded.body,
                    evidence = excluded.evidence,
                    affected_count = COALESCE(excluded.affected_count, signals.affected_count)",
                rusqlite::params![
                    org,
                    sig.source,
                    sig.source_ref,
                    sig.kind.as_str(),
                    sig.severity.as_str(),
                    sig.title,
                    sig.body,
                    serde_json::to_string(&sig.evidence).expect("evidence serializes"),
                    fp,
                    serde_json::to_string(&sig.join_keys).expect("join keys serialize"),
                    sig.raw.to_string(),
                    sig.affected_count,
                    now,
                ],
            )?;
            // rusqlite reports 1 for both paths; distinguish via first_seen.
            n == 1
                && tx.query_row(
                    "SELECT first_seen = last_seen FROM signals
                     WHERE org_id = ?1 AND fingerprint = ?2",
                    rusqlite::params![org, fp],
                    |r| r.get::<_, bool>(0),
                )?
        };
        tx.execute(
            "INSERT INTO inbox_items (org_id, fingerprint, title, severity, summary,
                                      created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (org_id, fingerprint) DO UPDATE SET
                title = excluded.title,
                severity = excluded.severity,
                updated_at = excluded.updated_at",
            rusqlite::params![org, fp, sig.title, sig.severity.as_str(), sig.body, now],
        )?;
        let item_id: i64 = tx.query_row(
            "SELECT id FROM inbox_items WHERE org_id = ?1 AND fingerprint = ?2",
            rusqlite::params![org, fp],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO item_signals (org_id, item_id, fingerprint)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![org, item_id, fp],
        )?;

        // Escalation reopen: a dismissal is a decision about the impact
        // known at the time; when the impact grows 3×, the decision is
        // stale and the item returns for a fresh look.
        let escalated = {
            use rusqlite::OptionalExtension;
            let snapshot: Option<i64> = tx
                .query_row(
                    "SELECT dismissal_affected_count FROM inbox_items
                     WHERE org_id = ?1 AND id = ?2 AND status = 'dismissed'",
                    rusqlite::params![org, item_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            let current: Option<i64> = tx.query_row(
                "SELECT SUM(s.affected_count) FROM signals s
                 JOIN item_signals i ON i.org_id = s.org_id AND i.fingerprint = s.fingerprint
                 WHERE i.org_id = ?1 AND i.item_id = ?2",
                rusqlite::params![org, item_id],
                |r| r.get(0),
            )?;
            match (snapshot, current) {
                (Some(snap), Some(cur)) if snap >= 1 && cur >= snap.saturating_mul(3) => {
                    tx.execute(
                        "UPDATE inbox_items SET status = 'new', dismiss_reason = NULL,
                                dismissal_affected_count = NULL, decided_by = NULL,
                                decided_at = NULL, updated_at = ?3,
                                summary = summary || ?4
                         WHERE org_id = ?1 AND id = ?2",
                        rusqlite::params![
                            org,
                            item_id,
                            now,
                            format!(
                                "\n[escalated: affected grew to {cur} (≥3× the {snap} at dismissal)]"
                            )
                        ],
                    )?;
                    true
                }
                _ => false,
            }
        };
        tx.commit()?;
        Ok(IngestOutcome {
            inserted,
            item_id,
            escalated,
        })
    }

    /// Inbox items, newest activity first, optionally status-filtered.
    pub fn inbox(&self, org: i64, status: Option<&str>) -> Result<Vec<InboxItem>, StoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT i.id, i.fingerprint, i.title, i.severity, i.summary, i.status,
                    i.dismiss_reason, i.decided_by, i.decided_at, i.created_at, i.updated_at,
                    SUM(s.affected_count), MAX(s.last_seen)
             FROM inbox_items i
             JOIN item_signals l ON l.org_id = i.org_id AND l.item_id = i.id
             JOIN signals s ON s.org_id = i.org_id AND s.fingerprint = l.fingerprint
             WHERE i.org_id = ?1 AND (?2 IS NULL OR i.status = ?2)
             GROUP BY i.id
             ORDER BY i.updated_at DESC, i.id DESC",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![org, status], row_to_item)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// One inbox item with its signals and fingerprint-anchored outcome
    /// history (the memory that survives recurrences).
    pub fn inbox_item(&self, org: i64, id: i64) -> Result<InboxItemDetail, StoreError> {
        let conn = self.lock();
        let item = {
            let mut stmt = conn.prepare(
                "SELECT i.id, i.fingerprint, i.title, i.severity, i.summary, i.status,
                        i.dismiss_reason, i.decided_by, i.decided_at, i.created_at, i.updated_at,
                        SUM(s.affected_count), MAX(s.last_seen)
                 FROM inbox_items i
                 JOIN item_signals l ON l.org_id = i.org_id AND l.item_id = i.id
                 JOIN signals s ON s.org_id = i.org_id AND s.fingerprint = l.fingerprint
                 WHERE i.org_id = ?1 AND i.id = ?2
                 GROUP BY i.id",
            )?;
            let mut rows = stmt.query(rusqlite::params![org, id])?;
            match rows.next()? {
                Some(r) => row_to_item(r)?,
                None => return Err(StoreError::NotFound),
            }
        };
        let signals = {
            let mut stmt = conn.prepare(
                "SELECT s.id, s.source, s.source_ref, s.kind, s.severity, s.title, s.body,
                        s.evidence, s.fingerprint, s.join_keys, s.affected_count,
                        s.first_seen, s.last_seen
                 FROM signals s
                 JOIN item_signals l ON l.org_id = s.org_id AND l.fingerprint = s.fingerprint
                 WHERE l.org_id = ?1 AND l.item_id = ?2
                 ORDER BY s.last_seen DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![org, id], |r| {
                    let evidence: String = r.get(7)?;
                    let join_keys: String = r.get(9)?;
                    Ok(StoredSignal {
                        id: r.get(0)?,
                        source: r.get(1)?,
                        source_ref: r.get(2)?,
                        kind: r.get(3)?,
                        severity: r.get(4)?,
                        title: r.get(5)?,
                        body: r.get(6)?,
                        evidence: serde_json::from_str(&evidence)
                            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
                        fingerprint: r.get(8)?,
                        join_keys: serde_json::from_str(&join_keys)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        affected_count: r.get(10)?,
                        first_seen: r.get(11)?,
                        last_seen: r.get(12)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        drop(conn);
        let fingerprints: Vec<String> = signals.iter().map(|s| s.fingerprint.clone()).collect();
        let outcomes = self.outcomes_for_fingerprints(org, &fingerprints)?;
        Ok(InboxItemDetail {
            item,
            signals,
            outcomes,
        })
    }

    /// Accept an item (someone is taking the work). `by` is the
    /// authenticated member name — teammates see who took it.
    pub fn accept_item(&self, org: i64, id: i64, by: &str) -> Result<InboxItem, StoreError> {
        self.decide(org, id, "accepted", None, by)
    }

    /// Dismiss an item with a structured reason. The reason lands in
    /// outcome memory (anchored to the fingerprint) and the item snapshots
    /// its impact so a 3× recurrence can escalate it back.
    pub fn dismiss_item(
        &self,
        org: i64,
        id: i64,
        reason: DismissReason,
        by: &str,
    ) -> Result<InboxItem, StoreError> {
        let item = self.decide(org, id, "dismissed", Some(reason.as_str()), by)?;
        self.record_outcome(
            org,
            id,
            OutcomeKind::Dismissed.as_str(),
            None,
            Some(reason.as_str()),
        )?;
        Ok(item)
    }

    fn decide(
        &self,
        org: i64,
        id: i64,
        status: &str,
        reason: Option<&str>,
        by: &str,
    ) -> Result<InboxItem, StoreError> {
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = self.lock();
            let n = conn.execute(
                "UPDATE inbox_items SET status = ?3, dismiss_reason = ?4, decided_by = ?5,
                        decided_at = ?6, updated_at = ?6,
                        dismissal_affected_count = CASE WHEN ?3 = 'dismissed' THEN
                            (SELECT SUM(s.affected_count) FROM signals s
                             JOIN item_signals l ON l.org_id = s.org_id
                                 AND l.fingerprint = s.fingerprint
                             WHERE l.org_id = ?1 AND l.item_id = ?2)
                        ELSE NULL END
                 WHERE org_id = ?1 AND id = ?2 AND status = 'new'",
                rusqlite::params![org, id, status, reason, by, now],
            )?;
            if n == 0 {
                // Missing item vs. already-decided item are different errors.
                let exists: bool = conn.query_row(
                    "SELECT COUNT(*) > 0 FROM inbox_items WHERE org_id = ?1 AND id = ?2",
                    rusqlite::params![org, id],
                    |r| r.get(0),
                )?;
                return Err(if exists {
                    StoreError::Conflict(format!("item {id} was already decided"))
                } else {
                    StoreError::NotFound
                });
            }
        }
        Ok(self.inbox_item(org, id)?.item)
    }

    /// Record an outcome for an item; idempotent per (item, kind).
    /// Returns whether a new row was written.
    pub fn record_outcome(
        &self,
        org: i64,
        item_id: i64,
        kind: &str,
        pr_url: Option<&str>,
        note: Option<&str>,
    ) -> Result<bool, StoreError> {
        let conn = self.lock();
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM inbox_items WHERE org_id = ?1 AND id = ?2",
            rusqlite::params![org, item_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound);
        }
        let now = chrono::Utc::now().to_rfc3339();
        let n = conn.execute(
            "INSERT OR IGNORE INTO outcomes (org_id, item_id, kind, occurred_at, pr_url, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![org, item_id, kind, now, pr_url, note],
        )?;
        Ok(n == 1)
    }

    /// Outcomes anchored to fingerprints, newest first — this is the join
    /// that lets memory survive recurrences and future re-clustering.
    pub fn outcomes_for_fingerprints(
        &self,
        org: i64,
        fingerprints: &[String],
    ) -> Result<Vec<OutcomeRecord>, StoreError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT o.item_id, o.kind, o.occurred_at, o.pr_url, o.note
             FROM outcomes o
             JOIN item_signals l ON l.org_id = o.org_id AND l.item_id = o.item_id
             WHERE o.org_id = ?1 AND l.fingerprint = ?2
             ORDER BY o.occurred_at DESC, o.id DESC",
        )?;
        let mut out: Vec<OutcomeRecord> = Vec::new();
        for fp in fingerprints {
            let rows = stmt
                .query_map(rusqlite::params![org, fp], |r| {
                    Ok(OutcomeRecord {
                        item_id: r.get(0)?,
                        kind: r.get(1)?,
                        occurred_at: r.get(2)?,
                        pr_url: r.get(3)?,
                        note: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for record in rows {
                if !out
                    .iter()
                    .any(|o| o.item_id == record.item_id && o.kind == record.kind)
                {
                    out.push(record);
                }
            }
        }
        out.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));
        Ok(out)
    }
}

fn row_to_item(r: &rusqlite::Row<'_>) -> Result<InboxItem, rusqlite::Error> {
    Ok(InboxItem {
        id: r.get(0)?,
        fingerprint: r.get(1)?,
        title: r.get(2)?,
        severity: r.get(3)?,
        summary: r.get(4)?,
        status: r.get(5)?,
        dismiss_reason: r.get(6)?,
        decided_by: r.get(7)?,
        decided_at: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        affected_count: r.get(11)?,
        last_seen: r.get(12)?,
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
