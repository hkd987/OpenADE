//! Daemon state assembly: wires the PTY host, worktree manager, adapters,
//! and transcript store into the session lifecycle the API exposes.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use openade_core::rules;
use openade_core::session::{SessionMeta, SessionState};
use openade_core::Harness;
use uuid::Uuid;

use crate::adapters::{adapter_for, LaunchRequest, McpServerSpec};
use crate::pty::{CommandSpec, PtyError, PtyHost};
use crate::transcript::{EventKind, TranscriptError, TranscriptStore};
use crate::worktree::{WorktreeError, WorktreeManager};

/// Request to launch a new session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LaunchSessionRequest {
    pub title: String,
    pub harness: Harness,
    /// The repository to create a task worktree in.
    pub repo_root: PathBuf,
    /// Catalog entity the session is launched from, if any.
    #[serde(default)]
    pub entity_ref: Option<String>,
    /// Initial prompt for the agent.
    #[serde(default)]
    pub prompt: Option<String>,
    /// MCP servers to register for the session (e.g. `catalog`).
    #[serde(default)]
    pub mcp_servers: Vec<McpServerSpec>,
    /// Testing/backdoor: run this command instead of the harness CLI
    /// (the harness CLIs are not installed in CI).
    #[serde(default)]
    pub command_override: Option<CommandSpec>,
}

/// Errors from the daemon's session lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error(transparent)]
    Pty(#[from] PtyError),
    #[error(transparent)]
    Transcript(#[from] TranscriptError),
    #[error("no such session: {0}")]
    NotFound(Uuid),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// The daemon: owns every live session.
pub struct Daemon {
    data_dir: PathBuf,
    pty: PtyHost,
    store: TranscriptStore,
    sessions: Mutex<HashMap<Uuid, SessionMeta>>,
}

impl Daemon {
    /// Open the daemon with its state under `data_dir`.
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self, DaemonError> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir)?;
        let store = TranscriptStore::open(&data_dir)?;
        Ok(Daemon {
            data_dir,
            pty: PtyHost::new(),
            store,
            sessions: Mutex::new(HashMap::new()),
        })
    }

    /// The transcript store (read access for API queries).
    pub fn store(&self) -> &TranscriptStore {
        &self.store
    }

    fn worktree_manager(&self, repo_root: &std::path::Path) -> WorktreeManager {
        let repo_name = repo_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_string());
        WorktreeManager::new(
            repo_root.to_path_buf(),
            self.data_dir.join("worktrees").join(repo_name),
        )
    }

    /// Launch a session: create the worktree, materialize rules, register
    /// MCP servers, spawn the harness in a PTY, and start the transcript.
    pub fn launch(&self, req: LaunchSessionRequest) -> Result<SessionMeta, DaemonError> {
        let mut meta = SessionMeta::new(&req.title, req.harness, &req.repo_root);
        meta.entity_ref = req.entity_ref.clone();

        // R2: one worktree per task.
        let mgr = self.worktree_manager(&req.repo_root);
        let wt = mgr.create(&req.title)?;
        meta.worktree_path = Some(wt.path.clone());
        meta.branch = Some(wt.branch.clone());

        // R4: same rules for every harness. A project without a canonical
        // rules source is fine — the harness just runs with its own defaults.
        match rules::materialize_rules(&wt.path, &[req.harness], false) {
            Ok(_) | Err(rules::RulesError::MissingCanonical(_)) => {}
            Err(e) => tracing::warn!("rules materialization failed: {e}"),
        }

        // R5: register MCP servers. Project-scoped registrations are written
        // into the worktree; user-scoped ones (e.g. Codex's config.toml) are
        // surfaced to the caller instead of silently editing user config.
        let adapter = adapter_for(req.harness);
        for reg in adapter.mcp_registrations(&wt.path, &req.mcp_servers) {
            if reg.file.is_relative() {
                let target = wt.path.join(&reg.file);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                if !target.exists() {
                    fs::write(&target, &reg.snippet)?;
                }
            } else {
                tracing::info!(
                    "manual MCP registration needed for {}: {} ({})",
                    req.harness,
                    reg.file.display(),
                    reg.note
                );
            }
        }

        // R1: the harness runs in a daemon-owned PTY.
        let spec = req.command_override.clone().unwrap_or_else(|| {
            adapter.launch_command(&LaunchRequest {
                prompt: req.prompt.clone(),
                mcp_servers: req.mcp_servers.clone(),
            })
        });
        self.pty.spawn(meta.id, &spec, Some(wt.path.clone()))?;
        meta.set_state(SessionState::Running);

        // R6 groundwork: everything is on the record from the first moment.
        self.store.begin_session(&meta)?;
        if let Some(prompt) = &req.prompt {
            self.store.record(
                meta.id,
                EventKind::Prompt,
                serde_json::json!({ "text": prompt }),
            )?;
        }
        self.store.record(
            meta.id,
            EventKind::StateChange,
            serde_json::json!({ "state": "running", "command": spec.program }),
        )?;
        self.store.update_state(meta.id, SessionState::Running)?;

        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(meta.id, meta.clone());
        Ok(meta)
    }

    /// Refresh a session's state from its PTY (running → completed/failed
    /// when the process exits) and return the up-to-date metadata.
    pub fn get(&self, id: Uuid) -> Result<SessionMeta, DaemonError> {
        let mut sessions = self.sessions.lock().expect("sessions lock");
        let meta = sessions.get_mut(&id).ok_or(DaemonError::NotFound(id))?;
        if meta.state == SessionState::Running {
            if let Ok(pty) = self.pty.get(id) {
                if pty.has_exited() {
                    let state = match pty.exit_code() {
                        Some(0) | None => SessionState::Completed,
                        Some(_) => SessionState::Failed,
                    };
                    meta.set_state(state);
                    let _ = self.store.end_session(id, state);
                }
            }
        }
        Ok(meta.clone())
    }

    /// All sessions with refreshed states.
    pub fn list(&self) -> Vec<SessionMeta> {
        let ids: Vec<Uuid> = self
            .sessions
            .lock()
            .expect("sessions lock")
            .keys()
            .copied()
            .collect();
        let mut out: Vec<SessionMeta> =
            ids.into_iter().filter_map(|id| self.get(id).ok()).collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out
    }

    /// Current scrollback for a session.
    pub fn scrollback(&self, id: Uuid) -> Result<String, DaemonError> {
        Ok(self.pty.get(id)?.scrollback())
    }

    /// Send input to a session's PTY.
    pub fn write_input(&self, id: Uuid, data: &[u8]) -> Result<(), DaemonError> {
        self.pty.get(id)?.write_input(data)?;
        Ok(())
    }

    /// Kill a session's process (metadata and transcript are kept).
    pub fn kill(&self, id: Uuid) -> Result<SessionMeta, DaemonError> {
        {
            let sessions = self.sessions.lock().expect("sessions lock");
            if !sessions.contains_key(&id) {
                return Err(DaemonError::NotFound(id));
            }
        }
        let _ = self.pty.remove(id);
        let mut sessions = self.sessions.lock().expect("sessions lock");
        let meta = sessions.get_mut(&id).ok_or(DaemonError::NotFound(id))?;
        if !meta.state.is_terminal() {
            meta.set_state(SessionState::Failed);
            let _ = self.store.end_session(id, SessionState::Failed);
        }
        Ok(meta.clone())
    }

    /// Remove a finished session's worktree (dirty-state guarded).
    pub fn cleanup_worktree(&self, id: Uuid, force: bool) -> Result<(), DaemonError> {
        let meta = self.get(id)?;
        let Some(wt) = meta.worktree_path else {
            return Ok(());
        };
        let mgr = self.worktree_manager(&meta.repo_root);
        mgr.remove(&wt, force)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(dir: &std::path::Path) {
        let run = |args: &[&str]| {
            let st = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
            assert!(
                st.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&st.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "init"]);
    }

    fn setup() -> (TempDir, Daemon, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = Daemon::open(tmp.path().join("data")).unwrap();
        (tmp, daemon, repo)
    }

    fn sh(cmd: &str) -> CommandSpec {
        CommandSpec::new("sh").arg("-c").arg(cmd)
    }

    fn launch_req(repo: &std::path::Path, cmd: &str) -> LaunchSessionRequest {
        LaunchSessionRequest {
            title: "test task".into(),
            harness: Harness::ClaudeCode,
            repo_root: repo.to_path_buf(),
            entity_ref: Some("component:default/demo".into()),
            prompt: Some("do the thing".into()),
            mcp_servers: vec![],
            command_override: Some(sh(cmd)),
        }
    }

    fn wait_state(daemon: &Daemon, id: Uuid, state: SessionState) -> SessionMeta {
        for _ in 0..200 {
            let meta = daemon.get(id).unwrap();
            if meta.state == state {
                return meta;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("session never reached {state:?}");
    }

    #[test]
    fn launch_creates_worktree_runs_and_completes() {
        let (_tmp, daemon, repo) = setup();
        let meta = daemon
            .launch(launch_req(&repo, "printf session-ok; exit 0"))
            .unwrap();

        assert_eq!(meta.state, SessionState::Running);
        let wt = meta.worktree_path.clone().unwrap();
        assert!(wt.join("README.md").is_file());
        assert!(meta.branch.as_deref().unwrap().starts_with("openade/"));

        let done = wait_state(&daemon, meta.id, SessionState::Completed);
        assert!(done.state.is_terminal());
        assert!(daemon.scrollback(meta.id).unwrap().contains("session-ok"));

        // Transcript captured prompt + state change and indexed the entity.
        let events = daemon.store().read_transcript(meta.id).unwrap();
        assert!(events.iter().any(|e| e.kind == EventKind::Prompt));
        let by_entity = daemon
            .store()
            .sessions_for_entity("component:default/demo")
            .unwrap();
        assert_eq!(by_entity.len(), 1);
    }

    #[test]
    fn failing_command_marks_session_failed() {
        let (_tmp, daemon, repo) = setup();
        let meta = daemon.launch(launch_req(&repo, "exit 3")).unwrap();
        wait_state(&daemon, meta.id, SessionState::Failed);
    }

    #[test]
    fn parallel_sessions_get_isolated_worktrees() {
        let (_tmp, daemon, repo) = setup();
        let metas: Vec<SessionMeta> = (0..4)
            .map(|_| daemon.launch(launch_req(&repo, "sleep 20")).unwrap())
            .collect();
        let paths: std::collections::HashSet<_> = metas
            .iter()
            .map(|m| m.worktree_path.clone().unwrap())
            .collect();
        assert_eq!(paths.len(), 4);
        assert_eq!(daemon.list().len(), 4);
        for m in &metas {
            daemon.kill(m.id).unwrap();
        }
    }

    #[test]
    fn cleanup_respects_dirty_guard() {
        let (_tmp, daemon, repo) = setup();
        let meta = daemon.launch(launch_req(&repo, "true")).unwrap();
        wait_state(&daemon, meta.id, SessionState::Completed);

        let wt = meta.worktree_path.clone().unwrap();
        std::fs::write(wt.join("dirty.txt"), "uncommitted").unwrap();
        assert!(matches!(
            daemon.cleanup_worktree(meta.id, false),
            Err(DaemonError::Worktree(WorktreeError::Dirty(_)))
        ));
        daemon.cleanup_worktree(meta.id, true).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn rules_are_materialized_into_the_worktree() {
        let (_tmp, daemon, repo) = setup();
        // Give the repo a canonical rules file and commit it so worktrees see it.
        openade_core::rules::init_canonical_rules(&repo, "always be testing\n").unwrap();
        let run = |args: &[&str]| {
            let st = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(st.status.success());
        };
        run(&["add", "."]);
        run(&["commit", "-m", "rules"]);

        let meta = daemon.launch(launch_req(&repo, "true")).unwrap();
        let wt = meta.worktree_path.unwrap();
        let claude_md = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("always be testing"));
    }

    #[test]
    fn mcp_servers_are_registered_project_scoped() {
        let (_tmp, daemon, repo) = setup();
        let mut req = launch_req(&repo, "true");
        req.mcp_servers = vec![McpServerSpec {
            name: "catalog".into(),
            transport: crate::adapters::McpTransport::Stdio {
                command: "catalog-mcp".into(),
                args: vec![],
            },
        }];
        let meta = daemon.launch(req).unwrap();
        let wt = meta.worktree_path.unwrap();
        let mcp_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
        assert!(mcp_json["mcpServers"]["catalog"].is_object());
    }
}
