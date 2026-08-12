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

/// Where a session's working directory lives (Xirp's "checkout mode").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckoutMode {
    /// A fresh Git worktree on its own branch (parallel-safe default).
    #[default]
    Worktree,
    /// Run directly in the repository's main checkout — for quick work in
    /// the tree you already have open. No isolation: one such session per
    /// repo is wise, and its checkout is never cleaned up.
    Main,
}

/// Request to launch a new session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LaunchSessionRequest {
    pub title: String,
    pub harness: Harness,
    /// The repository to create a task worktree in.
    pub repo_root: PathBuf,
    /// Checkout mode: isolated worktree (default) or the main checkout.
    #[serde(default)]
    pub checkout: CheckoutMode,
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

/// Request to pick up a shared workspace session locally (any harness).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PickupRequest {
    /// The shared session id on the workspace server.
    pub workspace_session_id: i64,
    pub harness: Harness,
    pub repo_root: PathBuf,
    #[serde(default)]
    pub checkout: CheckoutMode,
    /// Testing/backdoor, as in [`LaunchSessionRequest`].
    #[serde(default)]
    pub command_override: Option<CommandSpec>,
}

/// Request to start a session from an inbox item (triage session).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FromInboxRequest {
    pub item_id: i64,
    pub harness: Harness,
    pub repo_root: PathBuf,
    #[serde(default)]
    pub checkout: CheckoutMode,
    /// Look without deciding: the item stays in the queue for the team.
    #[serde(default)]
    pub investigate: bool,
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
    #[error("session runs in the repository's main checkout; there is no worktree to remove")]
    MainCheckout,
    #[error("{0}")]
    Workspace(String),
}

/// Memory wiring, replaceable at runtime (onboarding saves settings and
/// applies them without a restart).
#[derive(Default)]
struct MemoryState {
    catalog: Option<Arc<dyn CatalogProvider>>,
    memory_repo: Option<MemoryRepo>,
    /// Display names of the active sources (for /config and logs).
    source_names: Vec<String>,
}

/// The daemon: owns every live session.
pub struct Daemon {
    data_dir: PathBuf,
    pty: PtyHost,
    store: TranscriptStore,
    sessions: Mutex<HashMap<Uuid, SessionMeta>>,
    memory: std::sync::RwLock<MemoryState>,
    settings: std::sync::RwLock<crate::config::Settings>,
    /// Embedded inbox store (the local fallback when no workspace server
    /// is configured; same schema as the server by construction).
    inbox_store: Arc<openade_server::store::Store>,
    /// Local session id → shared workspace session id, so outcome
    /// verdicts reach the team copy of a shared session.
    shared_ids: Mutex<HashMap<Uuid, i64>>,
}

impl Daemon {
    /// Open the daemon with its state under `data_dir`.
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self, DaemonError> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir)?;
        let store = TranscriptStore::open(&data_dir)?;
        let inbox_store = openade_server::store::Store::open(&data_dir)
            .map_err(|e| DaemonError::Workspace(format!("local inbox store: {e}")))?;
        Ok(Daemon {
            data_dir,
            pty: PtyHost::new(),
            store,
            sessions: Mutex::new(HashMap::new()),
            memory: std::sync::RwLock::new(MemoryState::default()),
            settings: std::sync::RwLock::new(crate::config::Settings::default()),
            inbox_store: Arc::new(inbox_store),
            shared_ids: Mutex::new(HashMap::new()),
        })
    }

    /// The inbox for this request: the team's (workspace server) when
    /// multiplayer is configured, the embedded local one otherwise.
    pub fn inbox_backend(&self) -> crate::inbox::InboxBackend {
        match self.workspace_client() {
            Some(client) => crate::inbox::InboxBackend::Remote(client),
            None => crate::inbox::InboxBackend::Embedded(self.inbox_store.clone()),
        }
    }

    fn memory_read(&self) -> std::sync::RwLockReadGuard<'_, MemoryState> {
        self.memory.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Attach a catalog provider; sessions launched from an entity get a
    /// context bundle injected (PRD R5/G2).
    pub fn with_catalog(self, provider: Arc<dyn CatalogProvider>) -> Self {
        {
            let mut memory = self.memory.write().unwrap_or_else(|e| e.into_inner());
            memory.catalog = Some(provider);
            memory.source_names = vec!["catalog".to_string()];
        }
        self
    }

    /// Whether a catalog backend is configured.
    pub fn has_catalog(&self) -> bool {
        self.memory_read().catalog.is_some()
    }

    /// The active catalog provider, if any.
    fn catalog(&self) -> Option<Arc<dyn CatalogProvider>> {
        self.memory_read().catalog.clone()
    }

    /// The daemon-wide shared memory repo, if any.
    fn daemon_memory_repo(&self) -> Option<MemoryRepo> {
        self.memory_read().memory_repo.clone()
    }

    /// Names of the active memory sources (for /config and logs).
    pub fn memory_sources(&self) -> Vec<String> {
        self.memory_read().source_names.clone()
    }

    /// The stored (persisted) settings.
    pub fn settings(&self) -> crate::config::Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Attach a shared team memory repository: published knowledge is also
    /// committed straight to its default branch, and its index feeds
    /// entity context bundles.
    pub fn with_memory_repo(self, memory_repo: MemoryRepo) -> Self {
        self.memory
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .memory_repo = Some(memory_repo);
        self
    }

    /// Wire memory from `settings` merged with the environment (env wins),
    /// replacing whatever was active. Called at startup with the persisted
    /// settings and again whenever `PUT /config` saves new ones — the
    /// onboarding flow applies live, no restart.
    pub fn configure(&self, settings: &crate::config::Settings) {
        let backstage = settings.effective_backstage().map(|config| {
            tracing::info!("memory source: Backstage at {}", config.base_url);
            Arc::new(catalog_mcp::backstage::BackstageProvider::new(config))
                as Arc<dyn CatalogProvider>
        });
        let github = match catalog_mcp::github::GithubProvider::from_env() {
            Ok(provider) => {
                tracing::info!("memory source: GitHub via the local gh CLI");
                // A logged-out gh fails every call later — say so once, with
                // the fix.
                let auth_warning = catalog_mcp::github::resolve_gh_bin()
                    .and_then(|gh| catalog_mcp::github::gh_auth_warning(&gh));
                if let Some(warning) = auth_warning {
                    tracing::warn!("{warning}");
                }
                Some(Arc::new(provider) as Arc<dyn CatalogProvider>)
            }
            Err(reason) => {
                tracing::debug!("github memory source not configured: {reason}");
                None
            }
        };
        let router = catalog_mcp::MemoryRouter::new(backstage, github);
        let memory_repo = settings
            .effective_memory_repo()
            .and_then(|name| MemoryRepo::from_name(&name));
        {
            let mut memory = self.memory.write().unwrap_or_else(|e| e.into_inner());
            memory.source_names = match &router {
                Some(router) => router
                    .source_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                None => Vec::new(),
            };
            if memory.source_names.is_empty() {
                let hint = catalog_mcp::github::GH_SETUP_HINT;
                tracing::info!(
                    "memory sources: none — set BACKSTAGE_BASE_URL for Backstage, and/or {hint} \
                     for GitHub memory"
                );
            } else {
                tracing::info!("memory sources: {}", memory.source_names.join(", "));
            }
            if let Some(repo) = &memory_repo {
                tracing::info!("shared memory repo: {}", repo.repo());
            }
            memory.catalog = router.map(|r| Arc::new(r) as Arc<dyn CatalogProvider>);
            memory.memory_repo = memory_repo;
        }
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    }

    /// Persist new settings and apply them immediately.
    pub fn apply_settings(&self, settings: crate::config::Settings) -> Result<(), String> {
        settings.save(&self.data_dir)?;
        self.configure(&settings);
        Ok(())
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
    /// recorded in logs). Pass the launching repository so its committed
    /// `.openade/memory-repo` shared memory is folded in.
    pub async fn build_bundle(
        &self,
        entity_ref: &str,
        repo_root: Option<&std::path::Path>,
    ) -> Option<ContextBundle> {
        let catalog = self.catalog()?;
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
                self.add_shared_memory(&mut bundle, entity_ref, repo_root)
                    .await;
                self.add_workspace_memory(&mut bundle, entity_ref).await;
                Some(bundle)
            }
            Err(e) => {
                tracing::warn!("context bundle for {entity_ref} unavailable: {e}");
                None
            }
        }
    }

    /// The memory entity a session in this repository grounds in when the
    /// user didn't name one: the repo's own GitHub `origin` remote, as a
    /// `repo:owner/name` ref. `None` when the GitHub memory source isn't
    /// usable, there is no catalog, or the remote isn't a GitHub URL —
    /// callers fall back to launching without context. This is what makes
    /// memory zero-config: sessions ground themselves.
    pub fn infer_entity_ref(&self, repo_root: &std::path::Path) -> Option<String> {
        self.catalog()?;
        // Same enablement rules as the GitHub memory source itself
        // (OPENADE_GITHUB_MEMORY=0 disables; a gh binary must resolve).
        catalog_mcp::github::GithubProvider::from_env().ok()?;
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        crate::memory_repo::github_entity_from_remote(&url)
    }

    /// The shared memory repo for sessions in `repo_root`: the repository's
    /// committed `.openade/memory-repo` file wins (team-level, zero setup
    /// for each member), falling back to the daemon-wide
    /// `OPENADE_MEMORY_REPO` configuration.
    fn memory_repo_for(&self, repo_root: Option<&std::path::Path>) -> Option<MemoryRepo> {
        repo_root
            .and_then(MemoryRepo::for_repo)
            .or_else(|| self.daemon_memory_repo())
    }

    /// Fold the shared team memory repo into a bundle: recent index entries
    /// mentioning this entity become prior-session context (what teammates
    /// already learned), and the repo itself is linked as documentation.
    /// Read failures degrade silently — shared memory never blocks a launch.
    async fn add_shared_memory(
        &self,
        bundle: &mut ContextBundle,
        entity_ref: &str,
        repo_root: Option<&std::path::Path>,
    ) {
        const MAX_SHARED: usize = 3;
        let Some(memory_repo) = self.memory_repo_for(repo_root) else {
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
                verdict: None,
            });
        }
    }

    /// Fold the multiplayer workspace into a bundle: teammates' shared
    /// sessions on this entity become prior-session context, and the
    /// workspace is linked. Degrades silently — the server being down
    /// never blocks a launch.
    async fn add_workspace_memory(&self, bundle: &mut ContextBundle, entity_ref: &str) {
        const MAX_SHARED: usize = 3;
        let Some(client) = self.workspace_client() else {
            return;
        };
        bundle.docs.push(DocLink {
            title: format!("Team workspace #{}", client.workspace_id),
            url: format!("{}/workspaces/{}", client.base_url, client.workspace_id),
        });
        match client.sessions(Some(entity_ref)).await {
            Ok(sessions) => {
                for s in sessions.into_iter().take(MAX_SHARED) {
                    // The upload time gives the age/STALE annotation; the
                    // verdict says whether the shared work actually shipped.
                    let uploaded = chrono::DateTime::parse_from_rfc3339(&s.uploaded_at)
                        .ok()
                        .map(|t| t.with_timezone(&chrono::Utc));
                    bundle.prior_sessions.push(PriorSessionSummary {
                        session_id: format!("workspace-{}", s.id),
                        harness: Some(s.harness),
                        completed_at: uploaded,
                        summary: format!("{} — {} (shared by {})", s.title, s.summary, s.shared_by),
                        verdict: s.verdict.clone(),
                    });
                }
            }
            Err(e) => tracing::warn!("workspace read-back unavailable: {e}"),
        }
    }

    /// The configured multiplayer workspace connection, if any.
    pub fn workspace_client(&self) -> Option<crate::workspace::WorkspaceClient> {
        self.settings().effective_workspace()
    }

    /// Share a session to the team workspace (Xirp-style manual upload):
    /// the harness-neutral record — meta, summary, artifact markdown, and
    /// transcript events — goes to the server; nothing is uploaded unless
    /// this is called.
    pub async fn share(&self, id: Uuid) -> Result<crate::workspace::WorkspaceSession, DaemonError> {
        use crate::artifact::{Summarizer, TemplateSummarizer};
        let client = self.workspace_client().ok_or_else(|| {
            DaemonError::Workspace(
                "no workspace server configured — set the server URL, member token, \
                 and workspace in Settings"
                    .into(),
            )
        })?;
        let meta = self.get(id)?;
        let events = self.store.read_transcript(id)?;
        let diff = self.diff(id)?;
        let summarizer = TemplateSummarizer;
        let record = serde_json::json!({
            "title": meta.title,
            "harness": meta.harness.id(),
            "entity_ref": meta.entity_ref,
            "branch": meta.branch,
            "summary": summarizer.summary_line(&meta, &events, &diff),
            "markdown": summarizer.summarize(&meta, &events, &diff),
            "events": events,
        });
        let shared = client
            .upload(&record)
            .await
            .map_err(DaemonError::Workspace)?;
        // Remember the team copy so an outcome verdict can reach it later.
        self.shared_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, shared.id);
        Ok(shared)
    }

    /// Start a session from an inbox item (triage session): the item's
    /// evidence and the outcome history of prior attempts land in
    /// `.openade/inbox-item.md`, and the agent is prompted to investigate
    /// and fix. Accepts the item — teammates see who took it — unless
    /// `investigate` (a look without a decision).
    pub async fn start_from_inbox(
        &self,
        req: FromInboxRequest,
    ) -> Result<SessionMeta, DaemonError> {
        let backend = self.inbox_backend();
        let detail = backend
            .inbox_item(req.item_id)
            .await
            .map_err(DaemonError::Workspace)?;
        let item = &detail["item"];
        let title = item["title"].as_str().unwrap_or("inbox item").to_string();
        let severity = item["severity"].as_str().unwrap_or("unknown");

        let mut doc = format!(
            "# Triaging: {title}\n\nSeverity **{severity}**, affected {}, first seen {}, last seen {}.\n",
            item["affected_count"]
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "unknown".into()),
            detail["signals"][0]["first_seen"].as_str().unwrap_or("?"),
            item["last_seen"].as_str().unwrap_or("?"),
        );
        if let Some(signals) = detail["signals"].as_array() {
            for sig in signals {
                doc.push_str(&format!(
                    "\n## Signal from {} ({})\n\n{}\n",
                    sig["source"].as_str().unwrap_or("?"),
                    sig["kind"].as_str().unwrap_or("?"),
                    sig["body"].as_str().unwrap_or(""),
                ));
                if let Some(evidence) = sig["evidence"].as_array() {
                    if !evidence.is_empty() {
                        doc.push_str("\nEvidence:\n");
                        for e in evidence {
                            doc.push_str(&format!(
                                "- [{}]({})\n",
                                e["label"].as_str().unwrap_or("link"),
                                e["url"].as_str().unwrap_or(""),
                            ));
                        }
                    }
                }
                let keys = &sig["join_keys"];
                if keys.as_object().is_some_and(|o| !o.is_empty()) {
                    doc.push_str(&format!("\nJoin keys: `{keys}`\n"));
                }
            }
        }
        let outcomes = detail["outcomes"].as_array().cloned().unwrap_or_default();
        if !outcomes.is_empty() {
            doc.push_str("\n## Prior outcomes on this fingerprint\n\n");
            let now = chrono::Utc::now();
            for o in outcomes
                .iter()
                .take(openade_core::context::PRIOR_ATTEMPTS_CAP)
            {
                let age = o["occurred_at"]
                    .as_str()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| {
                        openade_core::context::age_annotation(t.with_timezone(&chrono::Utc), now)
                    })
                    .unwrap_or_default();
                let pr = o["pr_url"]
                    .as_str()
                    .map(|u| format!(" [attempt PR: {u}]"))
                    .unwrap_or_default();
                let note = o["note"]
                    .as_str()
                    .map(|n| format!(" ({n})"))
                    .unwrap_or_default();
                doc.push_str(&format!(
                    "- \"{}\" on {} {age}{note}{pr}\n",
                    o["kind"].as_str().unwrap_or("?"),
                    o["occurred_at"]
                        .as_str()
                        .map(|t| t.split('T').next().unwrap_or(t))
                        .unwrap_or("?"),
                ));
            }
            doc.push_str(
                "\nSTALE entries are old context — they inform, they do not veto a retry.\n",
            );
        }

        let prompt = format!(
            "You are triaging the signal {title:?} (severity {severity}). Read \
             .openade/inbox-item.md for the evidence and the outcome history of \
             prior attempts, investigate the root cause, and fix it with tests."
        );
        let launch = LaunchSessionRequest {
            title: format!("triage: {title}"),
            harness: req.harness,
            repo_root: req.repo_root,
            checkout: req.checkout,
            entity_ref: None,
            prompt: Some(prompt),
            mcp_servers: Vec::new(),
            command_override: req.command_override,
        };
        let mut meta = self.launch(launch, None)?;
        let wt = meta
            .worktree_path
            .clone()
            .expect("launched session has a workdir");
        let dot = wt.join(".openade");
        fs::create_dir_all(&dot)?;
        fs::write(dot.join("inbox-item.md"), &doc)?;

        if !req.investigate {
            backend
                .accept(req.item_id)
                .await
                .map_err(DaemonError::Workspace)?;
        }
        meta.inbox_item_id = Some(req.item_id);
        self.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(meta.id, meta.clone());
        Ok(meta)
    }

    /// Record what reality decided about an inbox-launched session: the
    /// task branch's PR fate via the user's own gh CLI. Idempotent — safe
    /// to call on every terminal-state edge. Also stamps the verdict on
    /// the shared team copy when the session was shared.
    pub async fn record_inbox_outcome(&self, id: Uuid) -> Result<serde_json::Value, DaemonError> {
        let meta = self.get(id)?;
        let Some(item_id) = meta.inbox_item_id else {
            return Err(DaemonError::Workspace(
                "session was not started from an inbox item".into(),
            ));
        };
        let Some(branch) = meta.branch.clone() else {
            return Ok(serde_json::json!({ "recorded": false, "note": "session has no branch" }));
        };
        let repo_root = meta.repo_root.clone();
        let fate = tokio::task::spawn_blocking(move || Self::branch_pr_fate(&repo_root, &branch))
            .await
            .expect("pr fate task panicked");
        let (kind, pr_url) = match fate {
            Ok(Some(pair)) => pair,
            Ok(None) => {
                return Ok(serde_json::json!({
                    "recorded": false,
                    "note": "no decided PR for the session branch yet",
                }));
            }
            Err(note) => return Ok(serde_json::json!({ "recorded": false, "note": note })),
        };
        let result = self
            .inbox_backend()
            .record_outcome(item_id, kind.clone(), Some(pr_url.clone()), None)
            .await
            .map_err(DaemonError::Workspace)?;
        self.store.record(
            id,
            EventKind::Outcome,
            serde_json::json!({ "kind": kind, "pr_url": pr_url, "inbox_item_id": item_id }),
        )?;
        // Verdicts travel to the shared team copy too.
        let shared_id = self
            .shared_ids
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .copied();
        if let Some(sid) = shared_id {
            if let Some(client) = self.workspace_client() {
                if let Err(e) = client.set_verdict(sid, &kind).await {
                    tracing::warn!("verdict did not reach the shared session: {e}");
                }
            }
        }
        Ok(serde_json::json!({
            "recorded": result["recorded"],
            "kind": kind,
            "pr_url": pr_url,
        }))
    }

    /// The fate of the branch's PR via the user's gh CLI:
    /// `Ok(Some((kind, url)))` when decided, `Ok(None)` when still open or
    /// absent, `Err(note)` when gh is unavailable/unusable.
    fn branch_pr_fate(
        repo_root: &std::path::Path,
        branch: &str,
    ) -> Result<Option<(String, String)>, String> {
        let Some(gh) = catalog_mcp::github::resolve_gh_bin() else {
            return Err("gh CLI not available".to_string());
        };
        let output = std::process::Command::new(gh)
            .current_dir(repo_root)
            .args(["pr", "view", branch, "--json", "state,url"])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|e| format!("bad gh output: {e}"))?;
        let url = parsed["url"].as_str().unwrap_or("").to_string();
        match parsed["state"].as_str() {
            Some("MERGED") => Ok(Some(("merged".to_string(), url))),
            Some("CLOSED") => Ok(Some(("closed".to_string(), url))),
            _ => Ok(None),
        }
    }

    /// Pick up a shared workspace session as a new local session — in any
    /// harness. The server record is harness-neutral; this renders it for
    /// the chosen harness: context via its rules file (launch path),
    /// history via `.openade/pickup.md`, and the takeover instruction via
    /// the adapter's own prompt convention.
    pub async fn pickup(&self, req: PickupRequest) -> Result<SessionMeta, DaemonError> {
        let client = self
            .workspace_client()
            .ok_or_else(|| DaemonError::Workspace("no workspace server configured".into()))?;
        let shared = client
            .session(req.workspace_session_id)
            .await
            .map_err(DaemonError::Workspace)?;

        let mut doc = format!(
            "# Picking up: {}

Shared by **{}** (originally run on {}), uploaded {}.

## Summary

{}

## Full session record

{}
",
            shared.session.title,
            shared.session.shared_by,
            shared.session.harness,
            shared.session.uploaded_at,
            shared.session.summary,
            shared.markdown,
        );
        if let Some(prompts) = shared.events.as_array() {
            let asked: Vec<&str> = prompts
                .iter()
                .filter(|e| e["kind"] == "prompt")
                .filter_map(|e| e["payload"]["text"].as_str())
                .collect();
            if !asked.is_empty() {
                doc.push_str(
                    "
## Original prompts

",
                );
                for p in asked {
                    doc.push_str(&format!(
                        "> {p}
"
                    ));
                }
            }
        }

        let prompt = format!(
            "You are picking up the shared team session {:?} where it left off. Read \
             .openade/pickup.md for the full context and continue the work.",
            shared.session.title
        );
        let launch = LaunchSessionRequest {
            title: format!("pickup: {}", shared.session.title),
            harness: req.harness,
            repo_root: req.repo_root,
            checkout: req.checkout,
            entity_ref: shared.session.entity_ref.clone(),
            prompt: Some(prompt),
            mcp_servers: Vec::new(),
            command_override: req.command_override,
        };
        let meta = self.launch(launch, None)?;
        // Launched sessions always have a working directory (worktree or
        // the repo itself in main-checkout mode).
        let wt = meta
            .worktree_path
            .clone()
            .expect("launched session has a workdir");
        let dot = wt.join(".openade");
        fs::create_dir_all(&dot)?;
        fs::write(dot.join("pickup.md"), &doc)?;
        Ok(meta)
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
                    verdict: None,
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

        // R2: one worktree per task — unless the user explicitly asked to
        // run in the main checkout (Xirp's second checkout mode).
        let mgr = self.worktree_manager(&req.repo_root);
        let workdir = match req.checkout {
            CheckoutMode::Worktree => {
                let wt = mgr.create(&req.title)?;
                meta.branch = Some(wt.branch.clone());
                meta.base_commit = Some(wt.base_commit.clone());
                wt.path
            }
            CheckoutMode::Main => {
                let (branch, head) = mgr.current_branch_and_head()?;
                meta.branch = Some(branch);
                meta.base_commit = Some(head);
                req.repo_root.clone()
            }
        };
        meta.worktree_path = Some(workdir.clone());
        let wt_path = workdir;

        // R4: same rules for every harness. A project without a canonical
        // rules source is fine — the harness just runs with its own defaults.
        match rules::materialize_rules(&wt_path, &[req.harness], false) {
            Ok(_) | Err(rules::RulesError::MissingCanonical(_)) => {}
            Err(e) => tracing::warn!("rules materialization failed: {e}"),
        }

        // R5/G2: inject the context bundle where the harness will read it —
        // appended to its rules file — plus machine-readable copies under
        // .openade/ for tools and handoff.
        if let Some(bundle) = &bundle {
            let dot_openade = wt_path.join(".openade");
            fs::create_dir_all(&dot_openade)?;
            fs::write(
                dot_openade.join("context.json"),
                serde_json::to_string_pretty(bundle).unwrap_or_default(),
            )?;
            let markdown = bundle.to_markdown_budgeted();
            fs::write(dot_openade.join("context.md"), &markdown)?;

            let rules_file = wt_path.join(req.harness.rules_filename());
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
        if self.has_catalog()
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
        for reg in adapter.mcp_registrations(&wt_path, &mcp_servers) {
            match reg.scope {
                crate::adapters::RegistrationScope::Project => {
                    // reg.file is worktree-relative, so joining onto the
                    // worktree root always yields a parent.
                    let target = wt_path.join(&reg.file);
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
        self.pty.spawn(meta.id, &spec, Some(wt_path.clone()))?;
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

    /// Remove a finished session's worktree (dirty-state guarded). A
    /// main-checkout session has nothing to clean up — and its directory
    /// is the user's repository, which must never be removed.
    pub fn cleanup_worktree(&self, id: Uuid, force: bool) -> Result<(), DaemonError> {
        let meta = self.get(id)?;
        let Some(wt) = meta.worktree_path else {
            return Ok(());
        };
        if wt == meta.repo_root {
            return Err(DaemonError::MainCheckout);
        }
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

    /// Read one file from the session's working directory (the Files tab
    /// viewer). The path is worktree-relative; escapes are rejected.
    pub fn read_workdir_file(&self, id: Uuid, rel: &str) -> Result<String, DaemonError> {
        let meta = self.get(id)?;
        let wt = meta.worktree_path.ok_or(DaemonError::NotFound(id))?;
        let target = wt.join(rel.trim_start_matches('/'));
        let canonical = target
            .canonicalize()
            .map_err(|_| DaemonError::NotFound(id))?;
        let root = wt.canonicalize().map_err(|_| DaemonError::NotFound(id))?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err(DaemonError::NotFound(id));
        }
        Ok(fs::read_to_string(canonical)?)
    }

    /// Reusable agent skills discovered in the session's working directory
    /// (Xirp's Skills tab): `.claude/skills/<name>/SKILL.md` and
    /// `.openade/skills/*.md`. The description is the first non-heading,
    /// non-frontmatter line of the file.
    pub fn skills(&self, id: Uuid) -> Result<Vec<serde_json::Value>, DaemonError> {
        let meta = self.get(id)?;
        let wt = meta.worktree_path.ok_or(DaemonError::NotFound(id))?;
        let mut out = Vec::new();
        let mut push = |name: String, rel: PathBuf| {
            let description = fs::read_to_string(wt.join(&rel))
                .ok()
                .and_then(|content| {
                    // Skip a leading YAML frontmatter block, then take the
                    // first non-empty, non-heading line.
                    let body = match content.strip_prefix("---") {
                        Some(rest) => rest
                            .split_once("\n---")
                            .map(|(_, after)| after.to_string())
                            .unwrap_or(content.clone()),
                        None => content.clone(),
                    };
                    body.lines()
                        .map(str::trim)
                        .find(|l| !l.is_empty() && !l.starts_with('#'))
                        .map(str::to_string)
                })
                .unwrap_or_default();
            out.push(serde_json::json!({
                "name": name,
                "path": rel,
                "description": description,
            }));
        };
        if let Ok(entries) = fs::read_dir(wt.join(".claude/skills")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let rel = PathBuf::from(".claude/skills").join(&name).join("SKILL.md");
                if wt.join(&rel).is_file() {
                    push(name, rel);
                }
            }
        }
        if let Ok(entries) = fs::read_dir(wt.join(".openade/skills")) {
            for entry in entries.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if let Some(name) = file.strip_suffix(".md") {
                    push(
                        name.to_string(),
                        PathBuf::from(".openade/skills").join(&file),
                    );
                }
            }
        }
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(out)
    }

    /// Open pull requests for a repository, through the user's local gh CLI
    /// (Xirp's PR monitoring). Degrades to an empty list with a reason when
    /// gh is unavailable or the repo has no GitHub remote.
    pub fn pull_requests(repo_root: &std::path::Path) -> serde_json::Value {
        let Some(gh) = catalog_mcp::github::resolve_gh_bin() else {
            return serde_json::json!({ "prs": [], "note": "gh CLI not available" });
        };
        let output = std::process::Command::new(gh)
            .current_dir(repo_root)
            .args([
                "pr",
                "list",
                "--json",
                "number,title,url,headRefName,isDraft",
            ])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let prs: serde_json::Value =
                    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| serde_json::json!([]));
                serde_json::json!({ "prs": prs })
            }
            Ok(out) => serde_json::json!({
                "prs": [],
                "note": String::from_utf8_lossy(&out.stderr).trim(),
            }),
            Err(e) => serde_json::json!({ "prs": [], "note": e.to_string() }),
        }
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
        if let Some(memory_repo) = &self.memory_repo_for(Some(&meta.repo_root)) {
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
