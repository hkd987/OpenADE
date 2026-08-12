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

/// Errors from the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("not found")]
    NotFound,
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
             );",
        )?;
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
                    shared_by, uploaded_at
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
                    shared_by, uploaded_at, markdown, events
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
            },
            markdown: r.get(9)?,
            events: serde_json::from_str(&events_text)
                .unwrap_or_else(|_| serde_json::Value::Array(Vec::new())),
        })
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
