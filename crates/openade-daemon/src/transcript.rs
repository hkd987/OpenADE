//! Transcript recording (PRD R6 groundwork).
//!
//! Every session gets a structured JSONL event log (prompts, tool calls,
//! diffs, outcomes) plus a SQLite index for queries like "sessions for this
//! entity". The JSONL file is the source of truth the knowledge-artifact
//! summarization pass (Phase 3) will read; SQLite only indexes.
//!
//! Transcripts stay local unless the user explicitly publishes an artifact
//! (PRD §7.5).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use openade_core::session::{SessionMeta, SessionState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Kind of a transcript event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventKind {
    /// A user prompt sent to the agent.
    Prompt,
    /// A tool call the agent made (name + arguments in the payload).
    ToolCall,
    /// Raw terminal output chunk.
    Output,
    /// A diff produced in the worktree.
    Diff,
    /// Session state transition.
    StateChange,
    /// Catalog context injected at launch (the context bundle).
    Context,
    /// Final outcome (what changed, why — feeds the knowledge artifact).
    Outcome,
}

impl EventKind {
    fn as_str(&self) -> &'static str {
        match self {
            EventKind::Prompt => "prompt",
            EventKind::ToolCall => "tool-call",
            EventKind::Output => "output",
            EventKind::Diff => "diff",
            EventKind::StateChange => "state-change",
            EventKind::Context => "context",
            EventKind::Outcome => "outcome",
        }
    }
}

/// One line in a session's JSONL transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub session_id: Uuid,
    pub seq: i64,
    pub ts: DateTime<Utc>,
    pub kind: EventKind,
    pub payload: serde_json::Value,
}

/// A row from the session index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: Uuid,
    pub title: String,
    pub harness: String,
    pub repo_root: PathBuf,
    pub entity_ref: Option<String>,
    pub state: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub transcript_path: PathBuf,
}

/// Errors from the transcript store.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("unknown session: {0}")]
    UnknownSession(Uuid),
}

/// JSONL transcript files + SQLite index under one data directory.
pub struct TranscriptStore {
    dir: PathBuf,
    db: Mutex<Connection>,
}

impl TranscriptStore {
    /// Open (or create) the store under `dir`.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, TranscriptError> {
        let dir = dir.into();
        fs::create_dir_all(dir.join("transcripts"))?;
        let db = Connection::open(dir.join("index.db"))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                 id              TEXT PRIMARY KEY,
                 title           TEXT NOT NULL,
                 harness         TEXT NOT NULL,
                 repo_root       TEXT NOT NULL DEFAULT '',
                 entity_ref      TEXT,
                 state           TEXT NOT NULL,
                 started_at      TEXT NOT NULL,
                 ended_at        TEXT,
                 transcript_path TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS events (
                 session_id TEXT NOT NULL REFERENCES sessions(id),
                 seq        INTEGER NOT NULL,
                 ts         TEXT NOT NULL,
                 kind       TEXT NOT NULL,
                 PRIMARY KEY (session_id, seq)
             );
             CREATE INDEX IF NOT EXISTS idx_sessions_entity ON sessions(entity_ref);",
        )?;
        // Pre-alpha migration: add repo_root to indexes created before it
        // existed. ALTER TABLE ADD COLUMN is a no-op error when present.
        let has_repo_root = db.prepare("SELECT repo_root FROM sessions LIMIT 1").is_ok();
        if !has_repo_root {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN repo_root TEXT NOT NULL DEFAULT ''")?;
        }
        Ok(TranscriptStore {
            dir,
            db: Mutex::new(db),
        })
    }

    fn transcript_path(&self, id: Uuid) -> PathBuf {
        self.dir.join("transcripts").join(format!("{id}.jsonl"))
    }

    /// Register a session and create its (empty) transcript file.
    pub fn begin_session(&self, meta: &SessionMeta) -> Result<(), TranscriptError> {
        let path = self.transcript_path(meta.id);
        File::create(&path)?;
        let db = self.db.lock().expect("db lock");
        db.execute(
            "INSERT INTO sessions (id, title, harness, repo_root, entity_ref, state, started_at, transcript_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                meta.id.to_string(),
                meta.title,
                meta.harness.id(),
                meta.repo_root.to_string_lossy(),
                meta.entity_ref,
                state_str(meta.state),
                meta.created_at.to_rfc3339(),
                path.to_string_lossy(),
            ],
        )?;
        Ok(())
    }

    /// Append an event to the session's JSONL transcript and index it.
    pub fn record(
        &self,
        session_id: Uuid,
        kind: EventKind,
        payload: serde_json::Value,
    ) -> Result<SessionEvent, TranscriptError> {
        let db = self.db.lock().expect("db lock");
        let known: bool = db
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE id = ?1",
                params![session_id.to_string()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)?;
        if !known {
            return Err(TranscriptError::UnknownSession(session_id));
        }
        let seq: i64 = db.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE session_id = ?1",
            params![session_id.to_string()],
            |r| r.get(0),
        )?;
        let event = SessionEvent {
            session_id,
            seq,
            ts: Utc::now(),
            kind,
            payload,
        };

        let mut file = OpenOptions::new()
            .append(true)
            .open(self.transcript_path(session_id))?;
        let mut line = serde_json::to_string(&event)?;
        line.push('\n');
        file.write_all(line.as_bytes())?;

        db.execute(
            "INSERT INTO events (session_id, seq, ts, kind) VALUES (?1, ?2, ?3, ?4)",
            params![
                session_id.to_string(),
                seq,
                event.ts.to_rfc3339(),
                kind.as_str()
            ],
        )?;
        Ok(event)
    }

    /// Mark a session's final state.
    pub fn end_session(
        &self,
        session_id: Uuid,
        state: SessionState,
    ) -> Result<(), TranscriptError> {
        let db = self.db.lock().expect("db lock");
        let changed = db.execute(
            "UPDATE sessions SET state = ?2, ended_at = ?3 WHERE id = ?1",
            params![
                session_id.to_string(),
                state_str(state),
                Utc::now().to_rfc3339()
            ],
        )?;
        if changed == 0 {
            return Err(TranscriptError::UnknownSession(session_id));
        }
        Ok(())
    }

    /// Update the indexed state without ending the session.
    pub fn update_state(
        &self,
        session_id: Uuid,
        state: SessionState,
    ) -> Result<(), TranscriptError> {
        let db = self.db.lock().expect("db lock");
        let changed = db.execute(
            "UPDATE sessions SET state = ?2 WHERE id = ?1",
            params![session_id.to_string(), state_str(state)],
        )?;
        if changed == 0 {
            return Err(TranscriptError::UnknownSession(session_id));
        }
        Ok(())
    }

    /// All indexed sessions, newest first.
    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, TranscriptError> {
        self.query_sessions("SELECT * FROM sessions ORDER BY started_at DESC", &[])
    }

    /// Sessions launched from a given catalog entity, newest first (feeds
    /// "prior sessions on this entity" in context bundles).
    pub fn sessions_for_entity(
        &self,
        entity_ref: &str,
    ) -> Result<Vec<SessionRecord>, TranscriptError> {
        self.query_sessions(
            "SELECT * FROM sessions WHERE entity_ref = ?1 ORDER BY started_at DESC",
            &[entity_ref],
        )
    }

    /// Distinct repositories sessions have been launched in, most recently
    /// used first (feeds the project list in the UI).
    pub fn projects(&self) -> Result<Vec<PathBuf>, TranscriptError> {
        let db = self.db.lock().expect("db lock");
        let mut stmt = db.prepare(
            "SELECT repo_root, MAX(started_at) AS last_used FROM sessions
             WHERE repo_root != '' GROUP BY repo_root ORDER BY last_used DESC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(PathBuf::from(row?));
        }
        Ok(out)
    }

    /// Read a session's full transcript back from JSONL.
    pub fn read_transcript(&self, session_id: Uuid) -> Result<Vec<SessionEvent>, TranscriptError> {
        let path = self.transcript_path(session_id);
        if !path.exists() {
            return Err(TranscriptError::UnknownSession(session_id));
        }
        let content = fs::read_to_string(path)?;
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(TranscriptError::from))
            .collect()
    }

    fn query_sessions(
        &self,
        sql: &str,
        args: &[&str],
    ) -> Result<Vec<SessionRecord>, TranscriptError> {
        let db = self.db.lock().expect("db lock");
        let mut stmt = db.prepare(sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), row_to_record)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

fn state_str(state: SessionState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "idle".to_string())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let id: String = row.get("id")?;
    let started: String = row.get("started_at")?;
    let ended: Option<String> = row.get("ended_at")?;
    let path: String = row.get("transcript_path")?;
    let repo_root: String = row.get("repo_root")?;
    Ok(SessionRecord {
        id: id.parse().unwrap_or_default(),
        title: row.get("title")?,
        harness: row.get("harness")?,
        repo_root: PathBuf::from(repo_root),
        entity_ref: row.get("entity_ref")?,
        state: row.get("state")?,
        started_at: parse_ts(&started),
        ended_at: ended.as_deref().map(parse_ts),
        transcript_path: PathBuf::from(path),
    })
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_default()
}

/// Default data directory for the daemon (`~/.openade` or a fallback).
pub fn default_data_dir() -> PathBuf {
    std::env::var_os("OPENADE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".openade")))
        .unwrap_or_else(|| PathBuf::from(".openade-data"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openade_core::Harness;
    use tempfile::TempDir;

    fn store() -> (TempDir, TranscriptStore) {
        let tmp = TempDir::new().unwrap();
        let store = TranscriptStore::open(tmp.path()).unwrap();
        (tmp, store)
    }

    fn meta(entity: Option<&str>) -> SessionMeta {
        let mut m = SessionMeta::new("add retries", Harness::ClaudeCode, "/tmp/repo");
        m.entity_ref = entity.map(str::to_string);
        m
    }

    #[test]
    fn records_events_in_order_and_reads_them_back() {
        let (_tmp, store) = store();
        let m = meta(None);
        store.begin_session(&m).unwrap();

        store
            .record(
                m.id,
                EventKind::Prompt,
                serde_json::json!({"text": "add retries"}),
            )
            .unwrap();
        store
            .record(
                m.id,
                EventKind::ToolCall,
                serde_json::json!({"tool": "bash", "cmd": "ls"}),
            )
            .unwrap();
        store
            .record(
                m.id,
                EventKind::Outcome,
                serde_json::json!({"summary": "added retries"}),
            )
            .unwrap();

        let events = store.read_transcript(m.id).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(events[0].kind, EventKind::Prompt);
        assert_eq!(events[2].payload["summary"], "added retries");
    }

    #[test]
    fn unknown_session_is_rejected() {
        let (_tmp, store) = store();
        let err = store.record(Uuid::new_v4(), EventKind::Prompt, serde_json::json!({}));
        assert!(matches!(err, Err(TranscriptError::UnknownSession(_))));
        let err = store.end_session(Uuid::new_v4(), SessionState::Completed);
        assert!(matches!(err, Err(TranscriptError::UnknownSession(_))));
    }

    #[test]
    fn indexes_sessions_by_entity() {
        let (_tmp, store) = store();
        let a = meta(Some("component:default/payments-api"));
        let b = meta(Some("component:default/ledger"));
        let c = meta(Some("component:default/payments-api"));
        for m in [&a, &b, &c] {
            store.begin_session(m).unwrap();
        }
        store.end_session(a.id, SessionState::Completed).unwrap();

        let all = store.list_sessions().unwrap();
        assert_eq!(all.len(), 3);

        let payments = store
            .sessions_for_entity("component:default/payments-api")
            .unwrap();
        assert_eq!(payments.len(), 2);
        assert!(payments
            .iter()
            .any(|r| r.id == a.id && r.state == "completed"));
        assert!(payments
            .iter()
            .all(|r| r.entity_ref.as_deref() == Some("component:default/payments-api")));
    }

    #[test]
    fn projects_lists_distinct_repos_most_recent_first() {
        let (_tmp, store) = store();
        let older = SessionMeta::new("first", Harness::ClaudeCode, "/repos/alpha");
        store.begin_session(&older).unwrap();
        let mut mid = SessionMeta::new("second", Harness::CodexCli, "/repos/beta");
        mid.created_at = older.created_at + chrono::Duration::seconds(1);
        store.begin_session(&mid).unwrap();
        let mut newer = SessionMeta::new("third", Harness::GeminiCli, "/repos/alpha");
        newer.created_at = older.created_at + chrono::Duration::seconds(2);
        store.begin_session(&newer).unwrap();

        let projects = store.projects().unwrap();
        assert_eq!(
            projects,
            vec![PathBuf::from("/repos/alpha"), PathBuf::from("/repos/beta")]
        );
        let all = store.list_sessions().unwrap();
        assert_eq!(all[0].repo_root, PathBuf::from("/repos/alpha"));
    }

    #[test]
    fn migrates_indexes_created_before_repo_root_existed() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
        let db = Connection::open(tmp.path().join("index.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE sessions (
                 id TEXT PRIMARY KEY, title TEXT NOT NULL, harness TEXT NOT NULL,
                 entity_ref TEXT, state TEXT NOT NULL, started_at TEXT NOT NULL,
                 ended_at TEXT, transcript_path TEXT NOT NULL
             );
             CREATE TABLE events (
                 session_id TEXT NOT NULL, seq INTEGER NOT NULL, ts TEXT NOT NULL,
                 kind TEXT NOT NULL, PRIMARY KEY (session_id, seq)
             );
             INSERT INTO sessions VALUES ('00000000-0000-0000-0000-000000000001',
                 'old session', 'claude-code', NULL, 'completed',
                 '2026-01-01T00:00:00+00:00', NULL, '/old.jsonl');",
        )
        .unwrap();
        drop(db);

        let store = TranscriptStore::open(tmp.path()).unwrap();
        let all = store.list_sessions().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].repo_root, PathBuf::new());

        // New sessions insert fine after migration.
        let m = meta(None);
        store.begin_session(&m).unwrap();
        assert_eq!(store.list_sessions().unwrap().len(), 2);
        assert_eq!(store.projects().unwrap(), vec![PathBuf::from("/tmp/repo")]);
    }

    #[test]
    fn reading_an_unknown_transcript_fails() {
        let (_tmp, store) = store();
        assert!(matches!(
            store.read_transcript(Uuid::new_v4()),
            Err(TranscriptError::UnknownSession(_))
        ));
        assert!(matches!(
            store.update_state(Uuid::new_v4(), SessionState::Running),
            Err(TranscriptError::UnknownSession(_))
        ));
    }

    #[test]
    fn state_updates_are_indexed() {
        let (_tmp, store) = store();
        let m = meta(None);
        store.begin_session(&m).unwrap();
        store.update_state(m.id, SessionState::Running).unwrap();
        let all = store.list_sessions().unwrap();
        assert_eq!(all[0].state, "running");
        assert!(all[0].ended_at.is_none());
    }
}
