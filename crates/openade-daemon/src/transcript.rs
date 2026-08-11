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

    /// Test-only fault injection: run arbitrary SQL against the index (e.g.
    /// dropping a table) to exercise error paths that healthy storage can
    /// never produce.
    #[cfg(test)]
    pub(crate) fn raw_sql(&self, sql: &str) {
        self.db
            .lock()
            .expect("db lock")
            .execute_batch(sql)
            .expect("test sql");
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
#[path = "transcript_tests.rs"]
mod tests;
