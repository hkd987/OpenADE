//! Daemon state assembly: wires the PTY host, worktree manager, adapters,
//! and transcript store into the session lifecycle the API exposes.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use catalog_mcp::bundle::build_context_bundle;
use catalog_mcp::provider::{CatalogProvider, EntityRef};
use openade_core::context::{ContextBundle, DocLink, PriorSessionSummary};
use openade_core::rules;
use openade_core::session::{SessionMeta, SessionState};
use openade_core::Harness;
use uuid::Uuid;

use crate::adapters::{adapter_for, LaunchRequest, McpServerSpec};
use crate::artifact::ArtifactInfo;
use crate::memory_repo::MemoryRepo;
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

/// Request to hand a session's task to a different harness (P1/G4:
/// switch harness mid-task without losing working state).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HandoffRequest {
    /// The harness taking over.
    pub harness: Harness,
    /// Extra instruction appended to the handoff prompt.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Testing/backdoor, as in [`LaunchSessionRequest`].
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
    #[error("cannot hand off session {0}: {1}")]
    Handoff(Uuid, String),
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
    memory_repo: Option<MemoryRepo>,
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
            memory_repo: None,
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

    /// Attach a shared team memory repository: published knowledge is also
    /// committed straight to its default branch, and its index feeds
    /// entity context bundles.
    pub fn with_memory_repo(mut self, memory_repo: MemoryRepo) -> Self {
        self.memory_repo = Some(memory_repo);
        self
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
            Ok(mut bundle) => {
                self.add_shared_memory(&mut bundle, entity_ref).await;
                Some(bundle)
            }
            Err(e) => {
                tracing::warn!("context bundle for {entity_ref} unavailable: {e}");
                None
            }
        }
    }

    /// Fold the shared team memory repo into a bundle: recent index entries
    /// mentioning this entity become prior-session context (what teammates
    /// already learned), and the repo itself is linked as documentation.
    /// Read failures degrade silently — shared memory never blocks a launch.
    async fn add_shared_memory(&self, bundle: &mut ContextBundle, entity_ref: &str) {
        const MAX_SHARED: usize = 3;
        let Some(memory_repo) = self.memory_repo.clone() else {
            return;
        };
        bundle.docs.push(DocLink {
            title: format!("Shared team memory ({})", memory_repo.repo()),
            url: memory_repo.html_url(),
        });
        let reader = memory_repo.clone();
        let index = tokio::task::spawn_blocking(move || reader.read_file("index.md"))
            .await
            .ok()
            .flatten();
        let Some(index) = index else { return };
        let marker = format!("`{entity_ref}`");
        for line in index
            .lines()
            .filter(|l| l.starts_with("- [") && l.contains(&marker))
            .take(MAX_SHARED)
        {
            bundle.prior_sessions.push(PriorSessionSummary {
                session_id: "shared-memory".to_string(),
                harness: None,
                completed_at: None,
                summary: line.trim_start_matches("- ").to_string(),
            });
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
            match reg.scope {
                crate::adapters::RegistrationScope::Project => {
                    // reg.file is worktree-relative, so joining onto the
                    // worktree root always yields a parent.
                    let target = wt.path.join(&reg.file);
                    let parent = target.parent().expect("registration path has a parent");
                    fs::create_dir_all(parent)?;
                    if !target.exists() {
                        fs::write(&target, &reg.snippet)?;
                    }
                }
                crate::adapters::RegistrationScope::User => {
                    let file = reg.file.display().to_string();
                    tracing::info!(
                        "manual MCP registration needed for {}: {file} ({})",
                        req.harness,
                        reg.note
                    );
                }
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
        // Registering the session must succeed; individual event writes are
        // best-effort (a transcript hiccup must not kill a live session).
        self.store.begin_session(&meta)?;
        if let Some(bundle) = &bundle {
            self.record_event(
                meta.id,
                EventKind::Context,
                serde_json::to_value(bundle).unwrap_or_default(),
            );
        }
        if let Some(prompt) = &req.prompt {
            self.record_event(
                meta.id,
                EventKind::Prompt,
                serde_json::json!({ "text": prompt }),
            );
        }
        self.record_event(
            meta.id,
            EventKind::StateChange,
            serde_json::json!({ "state": "running", "command": spec.program }),
        );
        self.index_state(meta.id, SessionState::Running);

        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(meta.id, meta.clone());
        Ok(meta)
    }

    /// Test-only: register a session in the in-memory registry without going
    /// through `launch` (used to exercise guards for states that normal
    /// operation cannot produce, e.g. a session with no worktree).
    #[cfg(test)]
    pub(crate) fn insert_test_session(&self, meta: SessionMeta) {
        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(meta.id, meta);
    }

    /// Best-effort transcript write: failures are logged, never fatal to the
    /// session lifecycle.
    fn record_event(&self, id: Uuid, kind: EventKind, payload: serde_json::Value) {
        if let Err(e) = self.store.record(id, kind, payload) {
            tracing::warn!("transcript write failed for {id}: {e}");
        }
    }

    /// Best-effort state index update, same policy as [`Self::record_event`].
    fn index_state(&self, id: Uuid, state: SessionState) {
        if let Err(e) = self.store.update_state(id, state) {
            tracing::warn!("state index update failed for {id}: {e}");
        }
    }

    /// Refresh a session's state from its PTY (running → completed/failed on
    /// exit, running ↔ needs-input from prompt detection) and return the
    /// up-to-date metadata.
    pub fn get(&self, id: Uuid) -> Result<SessionMeta, DaemonError> {
        /// Silence required before prompt detection may flip a session to
        /// `needs-input` — agents redraw constantly while working.
        const NEEDS_INPUT_QUIESCENCE: std::time::Duration = std::time::Duration::from_millis(1000);

        let mut sessions = self.sessions.lock().expect("sessions lock");
        let meta = sessions.get_mut(&id).ok_or(DaemonError::NotFound(id))?;
        if matches!(meta.state, SessionState::Running | SessionState::NeedsInput) {
            if let Ok(pty) = self.pty.get(id) {
                if pty.has_exited() {
                    let state = match pty.exit_code() {
                        Some(0) | None => SessionState::Completed,
                        Some(_) => SessionState::Failed,
                    };
                    meta.set_state(state);
                    let _ = self.store.end_session(id, state);
                } else {
                    // R1: surface waiting-on-input in the grid.
                    let awaiting = pty.idle_for() >= NEEDS_INPUT_QUIESCENCE
                        && crate::pty::looks_like_awaiting_input(&pty.tail(512));
                    let state = if awaiting {
                        SessionState::NeedsInput
                    } else {
                        SessionState::Running
                    };
                    if meta.state != state {
                        meta.set_state(state);
                        self.index_state(id, state);
                    }
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
        out.sort_by_key(|m| std::cmp::Reverse(m.created_at));
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

    /// Hand a task to a different harness (P1/G4). The worktree, branch, and
    /// entity carry over; the new harness starts from a written handoff
    /// summary (`.openade/handoff.md`) of everything done so far. The old
    /// session is ended and both transcripts record the handoff.
    pub fn handoff(&self, id: Uuid, req: HandoffRequest) -> Result<SessionMeta, DaemonError> {
        use crate::artifact::{Summarizer, TemplateSummarizer};

        let old = self.get(id)?;
        let Some(wt_path) = old.worktree_path.clone() else {
            return Err(DaemonError::Handoff(id, "session has no worktree".into()));
        };
        if !wt_path.is_dir() {
            return Err(DaemonError::Handoff(id, "worktree no longer exists".into()));
        }

        // Working-state summary before we touch anything.
        let events = self.store.read_transcript(id)?;
        let diff = self.diff(id)?;
        let summarizer = TemplateSummarizer;
        let summary_line = summarizer.summary_line(&old, &events, &diff);
        let handoff_doc = summarizer.summarize(&old, &events, &diff);

        // End the old session (best effort if the PTY already exited).
        let _ = self.pty.remove(id);
        {
            let mut sessions = self.sessions.lock().expect("sessions lock");
            let old_meta = sessions.get_mut(&id).ok_or(DaemonError::NotFound(id))?;
            if !old_meta.state.is_terminal() {
                old_meta.set_state(SessionState::Completed);
                let _ = self.store.end_session(id, SessionState::Completed);
            }
        }

        // Carry state over on disk: handoff doc, rules for the new harness,
        // and the entity context bundle if the session had one.
        let dot_openade = wt_path.join(".openade");
        fs::create_dir_all(&dot_openade)?;
        fs::write(dot_openade.join("handoff.md"), &handoff_doc)?;
        match rules::materialize_rules(&wt_path, &[req.harness], false) {
            Ok(_) | Err(rules::RulesError::MissingCanonical(_)) => {}
            Err(e) => tracing::warn!("rules materialization failed: {e}"),
        }
        let context_md = dot_openade.join("context.md");
        if context_md.is_file() {
            let bundle_md = fs::read_to_string(&context_md)?;
            let rules_file = wt_path.join(req.harness.rules_filename());
            let mut content = fs::read_to_string(&rules_file).unwrap_or_default();
            if !content.contains("# System context:") {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push('\n');
                content.push_str(&bundle_md);
                fs::write(&rules_file, content)?;
            }
        }

        // New session, same task.
        let mut meta = SessionMeta::new(&old.title, req.harness, &old.repo_root);
        meta.worktree_path = Some(wt_path.clone());
        meta.branch = old.branch.clone();
        meta.base_commit = old.base_commit.clone();
        meta.entity_ref = old.entity_ref.clone();

        let mut prompt = format!(
            "You are taking over the task \"{}\" from {}. Read .openade/handoff.md \
             for what has been done so far. Current status: {}",
            old.title,
            old.harness.display_name(),
            summary_line
        );
        if let Some(extra) = &req.prompt {
            prompt.push(' ');
            prompt.push_str(extra);
        }

        let adapter = adapter_for(req.harness);
        let spec = req.command_override.clone().unwrap_or_else(|| {
            adapter.launch_command(&LaunchRequest {
                prompt: Some(prompt.clone()),
                mcp_servers: vec![],
            })
        });
        self.pty.spawn(meta.id, &spec, Some(wt_path))?;
        meta.set_state(SessionState::Running);

        self.store.begin_session(&meta)?;
        self.record_event(
            meta.id,
            EventKind::StateChange,
            serde_json::json!({
                "state": "running",
                "handoff_from": id,
                "previous_harness": old.harness.id(),
            }),
        );
        self.record_event(
            meta.id,
            EventKind::Prompt,
            serde_json::json!({ "text": prompt }),
        );
        self.index_state(meta.id, SessionState::Running);
        self.record_event(
            id,
            EventKind::Outcome,
            serde_json::json!({
                "summary": format!("Handed off to {} (session {})", req.harness.display_name(), meta.id),
                "handoff_to": meta.id,
            }),
        );

        self.sessions
            .lock()
            .expect("sessions lock")
            .insert(meta.id, meta.clone());
        Ok(meta)
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

        // Living documentation: the artifact plus an updated index page that
        // aggregates every published session (Xirp's "documentation that
        // compounds" — ours is Git-reviewed).
        let mgr = self.worktree_manager(&meta.repo_root);
        let index_rel = PathBuf::from(crate::artifact::INDEX_FILE);
        let existing_index = mgr.read_file_at("HEAD", &index_rel).ok();
        let entry = crate::artifact::IndexEntry {
            file_name: format!("{slug}.md"),
            title: meta.title.clone(),
            summary: summary.clone(),
            date: meta.created_at.format("%Y-%m-%d").to_string(),
            harness: meta.harness.id().to_string(),
            entity: meta.entity_ref.clone(),
        };
        let index = crate::artifact::upsert_index(existing_index.as_deref(), &entry);
        mgr.commit_files_on_branch(
            &branch,
            &[
                (file.as_path(), markdown.as_str()),
                (index_rel.as_path(), index.as_str()),
            ],
            &format!("docs: session knowledge — {}", meta.title),
        )?;

        // Shared team memory: also push the document + index straight to the
        // configured memory repo's default branch (everyone has write access
        // — no review gate there by design). Failure degrades to local-only.
        let mut shared_repo = None;
        let mut shared_path = None;
        if let Some(memory_repo) = &self.memory_repo {
            let shared_file = format!("sessions/{slug}.md");
            let shared_entry = crate::artifact::IndexEntry {
                file_name: shared_file.clone(),
                ..entry.clone()
            };
            let shared_index = crate::artifact::upsert_index(
                memory_repo.read_file("index.md").as_deref(),
                &shared_entry,
            );
            let message = format!("memory: {} — {}", meta.title, summary);
            match memory_repo.publish(&shared_file, &markdown, &shared_index, &message) {
                Ok(()) => {
                    shared_repo = Some(memory_repo.repo().to_string());
                    shared_path = Some(shared_file);
                }
                Err(e) => {
                    let repo = memory_repo.repo();
                    tracing::warn!("shared memory push to {repo} failed (kept locally): {e}");
                }
            }
        }

        // The artifact is already committed; a transcript hiccup is not fatal.
        self.record_event(
            id,
            EventKind::Outcome,
            serde_json::json!({
                "summary": summary,
                "artifact_branch": branch,
                "artifact_file": file,
                "shared_repo": shared_repo,
                "shared_path": shared_path,
            }),
        );

        Ok(ArtifactInfo {
            branch,
            file,
            summary,
            markdown,
            shared_repo,
            shared_path,
        })
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
