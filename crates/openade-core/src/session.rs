//! Session model: the unit of work OpenADE orchestrates.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::harness::Harness;

/// Lifecycle state of an agent session, surfaced in the session grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    /// Created but the harness process is not running.
    Idle,
    /// The harness process is running and working.
    Running,
    /// The harness is blocked waiting for user input.
    NeedsInput,
    /// The session finished successfully.
    Completed,
    /// The session finished with an error (or the process died).
    Failed,
}

impl SessionState {
    /// Whether the session has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, SessionState::Completed | SessionState::Failed)
    }
}

/// Metadata describing one agent session.
///
/// A session is bound to exactly one Git worktree so parallel sessions on the
/// same repository never collide (PRD R2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// Stable session identifier.
    pub id: Uuid,
    /// Short human-readable task title.
    pub title: String,
    /// Which harness runs this session.
    pub harness: Harness,
    /// The repository the task belongs to (the main checkout).
    pub repo_root: PathBuf,
    /// The isolated worktree this session operates in, once created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    /// The task branch checked out in the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit the task branch forked from (base for diff views).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_commit: Option<String>,
    /// Catalog entity this session was launched from, if any
    /// (e.g. `component:default/payments-api`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_ref: Option<String>,
    /// Current lifecycle state.
    pub state: SessionState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SessionMeta {
    /// Create metadata for a new idle session.
    pub fn new(title: impl Into<String>, harness: Harness, repo_root: impl Into<PathBuf>) -> Self {
        let now = Utc::now();
        SessionMeta {
            id: Uuid::new_v4(),
            title: title.into(),
            harness,
            repo_root: repo_root.into(),
            worktree_path: None,
            branch: None,
            base_commit: None,
            entity_ref: None,
            state: SessionState::Idle,
            created_at: now,
            updated_at: now,
        }
    }

    /// Transition to a new state, bumping `updated_at`.
    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
