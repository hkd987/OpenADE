//! Daemon state assembly: wires the PTY host, worktree manager, adapters,
//! and transcript store into the session lifecycle the API exposes.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use catalog_mcp::bundle::build_context_bundle;
use catalog_mcp::provider::{CatalogProvider, EntityRef};
use openade_core::context::{ContextBundle, PriorSessionSummary};
use openade_core::rules;
use openade_core::session::{SessionMeta, SessionState};
use openade_core::Harness;
use uuid::Uuid;

use crate::adapters::{adapter_for, LaunchRequest, McpServerSpec};
use crate::artifact::ArtifactInfo;
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
    catalog: Option<Arc<dyn CatalogProvider>>,
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
            catalog: None,
        })
    }

    /// Attach a catalog provider; sessions launched from an entity get a
    /// context bundle injected (PRD R5/G2).
    pub fn with_catalog(mut self, provider: Arc<dyn CatalogProvider>) -> Self {
        self.catalog = Some(provider);
        self
    }

    /// Whether a catalog backend is configured.
    pub fn has_catalog(&self) -> bool {
        self.catalog.is_some()
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

    /// Build the context bundle for an entity (PRD §7.3): catalog data plus
    /// summaries of prior OpenADE sessions on the same entity. Returns `None`
    /// when no catalog is configured or the entity cannot be resolved —
    /// sessions launch without context rather than failing (degradation is
    /// recorded in logs).
    pub async fn build_bundle(&self, entity_ref: &str) -> Option<ContextBundle> {
        let catalog = self.catalog.as_ref()?;
        let parsed: EntityRef = match entity_ref.parse() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("invalid entity ref {entity_ref:?}: {e}");
                return None;
            }
        };
        let prior = self.prior_session_summaries(entity_ref);
        match build_context_bundle(catalog.as_ref(), &parsed, prior).await {
            Ok(bundle) => Some(bundle),
            Err(e) => {
                tracing::warn!("context bundle for {entity_ref} unavailable: {e}");
                None
            }
        }
    }

    /// Summaries of the most recent sessions on an entity (G3: every session
    /// makes the next one smarter). Uses the recorded outcome when one
    /// exists, the session title otherwise.
    fn prior_session_summaries(&self, entity_ref: &str) -> Vec<PriorSessionSummary> {
        const MAX_PRIOR: usize = 3;
        let records = match self.store.sessions_for_entity(entity_ref) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        records
            .into_iter()
            .take(MAX_PRIOR)
            .map(|rec| {
                let summary = self
                    .store
                    .read_transcript(rec.id)
                    .ok()
                    .and_then(|events| {
                        events.into_iter().rev().find_map(|e| {
                            (e.kind == EventKind::Outcome)
                                .then(|| e.payload.get("summary")?.as_str().map(str::to_string))
                                .flatten()
                        })
                    })
                    .unwrap_or_else(|| format!("{} ({})", rec.title, rec.state));
                PriorSessionSummary {
                    session_id: rec.id.to_string(),
                    harness: Some(rec.harness),
                    completed_at: rec.ended_at,
                    summary,
                }
            })
            .collect()
    }

    /// Launch a session: create the worktree, materialize rules, inject the
    /// context bundle, register MCP servers, spawn the harness in a PTY, and
    /// start the transcript. Build `bundle` with [`Daemon::build_bundle`]
    /// when launching from a catalog entity.
    pub fn launch(
        &self,
        req: LaunchSessionRequest,
        bundle: Option<ContextBundle>,
    ) -> Result<SessionMeta, DaemonError> {
        let mut meta = SessionMeta::new(&req.title, req.harness, &req.repo_root);
        meta.entity_ref = req.entity_ref.clone();

        // R2: one worktree per task.
        let mgr = self.worktree_manager(&req.repo_root);
        let wt = mgr.create(&req.title)?;
        meta.worktree_path = Some(wt.path.clone());
        meta.branch = Some(wt.branch.clone());
        meta.base_commit = Some(wt.base_commit.clone());

        // R4: same rules for every harness. A project without a canonical
        // rules source is fine — the harness just runs with its own defaults.
        match rules::materialize_rules(&wt.path, &[req.harness], false) {
            Ok(_) | Err(rules::RulesError::MissingCanonical(_)) => {}
            Err(e) => tracing::warn!("rules materialization failed: {e}"),
        }

        // R5/G2: inject the context bundle where the harness will read it —
        // appended to its rules file — plus machine-readable copies under
        // .openade/ for tools and handoff.
        if let Some(bundle) = &bundle {
            let dot_openade = wt.path.join(".openade");
            fs::create_dir_all(&dot_openade)?;
            fs::write(
                dot_openade.join("context.json"),
                serde_json::to_string_pretty(bundle).unwrap_or_default(),
            )?;
            let markdown = bundle.to_markdown();
            fs::write(dot_openade.join("context.md"), &markdown)?;

            let rules_file = wt.path.join(req.harness.rules_filename());
            let mut content = fs::read_to_string(&rules_file).unwrap_or_default();
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&markdown);
            fs::write(&rules_file, content)?;
        }

        // A configured catalog means entity-launched sessions get the MCP
        // server registered automatically (deeper retrieval on demand).
        let mut mcp_servers = req.mcp_servers.clone();
        if self.catalog.is_some()
            && req.entity_ref.is_some()
            && !mcp_servers.iter().any(|s| s.name == "catalog")
        {
            mcp_servers.push(McpServerSpec {
                name: "catalog".into(),
                transport: crate::adapters::McpTransport::Stdio {
                    command: "catalog-mcp".into(),
                    args: vec![],
                },
            });
        }

        // R5: register MCP servers. Project-scoped registrations are written
        // into the worktree; user-scoped ones (e.g. Codex's config.toml) are
        // surfaced to the caller instead of silently editing user config.
        let adapter = adapter_for(req.harness);
        for reg in adapter.mcp_registrations(&wt.path, &mcp_servers) {
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
                mcp_servers: mcp_servers.clone(),
            })
        });
        self.pty.spawn(meta.id, &spec, Some(wt.path.clone()))?;
        meta.set_state(SessionState::Running);

        // R6 groundwork: everything is on the record from the first moment.
        self.store.begin_session(&meta)?;
        if let Some(bundle) = &bundle {
            self.store.record(
                meta.id,
                EventKind::Context,
                serde_json::to_value(bundle).unwrap_or_default(),
            )?;
        }
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

    /// The session's task diff: worktree state (committed + uncommitted)
    /// against the commit the task branch forked from (R3 diff view).
    pub fn diff(&self, id: Uuid) -> Result<String, DaemonError> {
        let meta = self.get(id)?;
        let (Some(wt), Some(base)) = (meta.worktree_path.as_ref(), meta.base_commit.as_ref())
        else {
            return Ok(String::new());
        };
        let mgr = self.worktree_manager(&meta.repo_root);
        Ok(mgr.diff(wt, base)?)
    }

    /// Files in the session's worktree (R3 file browser).
    pub fn files(&self, id: Uuid) -> Result<Vec<String>, DaemonError> {
        let meta = self.get(id)?;
        let Some(wt) = meta.worktree_path.as_ref() else {
            return Ok(Vec::new());
        };
        let mgr = self.worktree_manager(&meta.repo_root);
        Ok(mgr.files(wt)?)
    }

    /// Repositories sessions have been launched in (R3 project list).
    pub fn projects(&self) -> Result<Vec<PathBuf>, DaemonError> {
        Ok(self.store.projects()?)
    }

    /// Produce the session's knowledge artifact (R6): summarize the
    /// transcript + diff into markdown and commit it to the repository on an
    /// `openade/knowledge-*` review branch. The summary is also recorded as
    /// the session outcome, feeding future context bundles on the entity.
    pub fn publish_artifact(&self, id: Uuid) -> Result<ArtifactInfo, DaemonError> {
        use crate::artifact::{artifact_slug, Summarizer, TemplateSummarizer};

        let meta = self.get(id)?;
        let events = self.store.read_transcript(id)?;
        let diff = self.diff(id)?;

        let summarizer = TemplateSummarizer;
        let summary = summarizer.summary_line(&meta, &events, &diff);
        let markdown = summarizer.summarize(&meta, &events, &diff);

        let slug = artifact_slug(&meta);
        let branch = format!("{}{slug}", crate::artifact::KNOWLEDGE_BRANCH_PREFIX);
        let file = PathBuf::from(crate::artifact::ARTIFACT_DIR).join(format!("{slug}.md"));

        let mgr = self.worktree_manager(&meta.repo_root);
        mgr.commit_file_on_branch(
            &branch,
            &file,
            &markdown,
            &format!("docs: session knowledge — {}", meta.title),
        )?;

        self.store.record(
            id,
            EventKind::Outcome,
            serde_json::json!({
                "summary": summary,
                "artifact_branch": branch,
                "artifact_file": file,
            }),
        )?;

        Ok(ArtifactInfo {
            branch,
            file,
            summary,
            markdown,
        })
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
            .launch(launch_req(&repo, "printf session-ok; exit 0"), None)
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
        let meta = daemon.launch(launch_req(&repo, "exit 3"), None).unwrap();
        wait_state(&daemon, meta.id, SessionState::Failed);
    }

    #[test]
    fn parallel_sessions_get_isolated_worktrees() {
        let (_tmp, daemon, repo) = setup();
        let metas: Vec<SessionMeta> = (0..4)
            .map(|_| daemon.launch(launch_req(&repo, "sleep 20"), None).unwrap())
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
        let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
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
    fn diff_files_and_projects_are_exposed() {
        let (_tmp, daemon, repo) = setup();
        let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
        wait_state(&daemon, meta.id, SessionState::Completed);

        let wt = meta.worktree_path.clone().unwrap();
        std::fs::write(wt.join("README.md"), "hi\nagent was here\n").unwrap();

        let diff = daemon.diff(meta.id).unwrap();
        assert!(diff.contains("+agent was here"), "{diff}");

        let files = daemon.files(meta.id).unwrap();
        assert!(files.contains(&"README.md".to_string()));

        assert_eq!(daemon.projects().unwrap(), vec![repo]);
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

        let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
        let wt = meta.worktree_path.unwrap();
        let claude_md = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("always be testing"));
    }

    #[test]
    fn publish_artifact_commits_to_a_review_branch_without_touching_checkout() {
        let (_tmp, daemon, repo) = setup();
        let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
        wait_state(&daemon, meta.id, SessionState::Completed);

        // Session did some work in its worktree.
        let wt = meta.worktree_path.clone().unwrap();
        std::fs::write(wt.join("README.md"), "hi\nnew feature\n").unwrap();

        let head_before = Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;

        let info = daemon.publish_artifact(meta.id).unwrap();
        assert!(info.branch.starts_with("openade/knowledge-"));
        assert!(info.summary.contains("README.md"), "{}", info.summary);
        assert!(info.markdown.contains("# Session: test task"));

        // The artifact is committed on the review branch...
        let show = Command::new("git")
            .args([
                "-C",
                repo.to_str().unwrap(),
                "show",
                &format!("{}:{}", info.branch, info.file.display()),
            ])
            .output()
            .unwrap();
        assert!(show.status.success());
        let committed = String::from_utf8_lossy(&show.stdout);
        assert!(committed.contains("README.md"));

        // ...while the user's checkout is untouched (HEAD and status).
        let head_after = Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(head_before, head_after);
        let status = Command::new("git")
            .args(["-C", repo.to_str().unwrap(), "status", "--porcelain"])
            .output()
            .unwrap();
        assert!(status.stdout.is_empty());

        // The outcome is on the record for future context bundles.
        let events = daemon.store().read_transcript(meta.id).unwrap();
        let outcome = events
            .iter()
            .rev()
            .find(|e| e.kind == EventKind::Outcome)
            .unwrap();
        assert_eq!(outcome.payload["artifact_branch"], info.branch);
    }

    fn daemon_with_catalog(tmp: &TempDir) -> Daemon {
        Daemon::open(tmp.path().join("data"))
            .unwrap()
            .with_catalog(Arc::new(
                catalog_mcp::testutil::MockProvider::with_payments_graph(),
            ))
    }

    #[tokio::test]
    async fn entity_launch_injects_context_bundle() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = daemon_with_catalog(&tmp);
        assert!(daemon.has_catalog());

        let mut req = launch_req(&repo, "true");
        req.entity_ref = Some("component:default/payments-api".into());
        let bundle = daemon.build_bundle("component:default/payments-api").await;
        assert!(bundle.is_some());

        let meta = daemon.launch(req, bundle).unwrap();
        let wt = meta.worktree_path.clone().unwrap();

        // Injected where the harness reads instructions...
        let rules = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
        assert!(rules.contains("Payments API"), "{rules}");
        assert!(rules.contains("Payments Team"));

        // ...and machine-readable under .openade/.
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(wt.join(".openade/context.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            json["entity"]["entity_ref"],
            "component:default/payments-api"
        );
        assert!(wt.join(".openade/context.md").is_file());

        // The catalog MCP server was auto-registered project-scoped.
        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
        assert!(mcp["mcpServers"]["catalog"].is_object());

        // And the transcript recorded the injected context.
        let events = daemon.store().read_transcript(meta.id).unwrap();
        assert!(events.iter().any(|e| e.kind == EventKind::Context));
    }

    #[tokio::test]
    async fn prior_session_outcomes_feed_the_next_bundle() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = daemon_with_catalog(&tmp);

        // First session on the entity, with a recorded outcome.
        let mut req = launch_req(&repo, "true");
        req.entity_ref = Some("component:default/payments-api".into());
        let first = daemon.launch(req.clone(), None).unwrap();
        daemon
            .store()
            .record(
                first.id,
                EventKind::Outcome,
                serde_json::json!({ "summary": "Added idempotency keys to POST /charges." }),
            )
            .unwrap();

        // The next bundle for the same entity carries that knowledge.
        let bundle = daemon
            .build_bundle("component:default/payments-api")
            .await
            .unwrap();
        assert_eq!(bundle.prior_sessions.len(), 1);
        assert!(bundle.prior_sessions[0]
            .summary
            .contains("idempotency keys"));
        assert!(bundle.to_markdown().contains("idempotency keys"));
    }

    #[tokio::test]
    async fn unresolvable_entities_degrade_to_no_bundle() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = daemon_with_catalog(&tmp);

        assert!(daemon
            .build_bundle("component:default/ghost")
            .await
            .is_none());
        assert!(daemon.build_bundle("not-a-ref").await.is_none());

        // No catalog configured at all → also None, and launches still work.
        let plain = Daemon::open(tmp.path().join("data2")).unwrap();
        assert!(plain
            .build_bundle("component:default/payments-api")
            .await
            .is_none());
        let mut req = launch_req(&repo, "true");
        req.entity_ref = Some("component:default/ghost".into());
        let meta = daemon.launch(req, None).unwrap();
        assert!(meta.worktree_path.is_some());
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
        let meta = daemon.launch(req, None).unwrap();
        let wt = meta.worktree_path.unwrap();
        let mcp_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
        assert!(mcp_json["mcpServers"]["catalog"].is_object());
    }
}
