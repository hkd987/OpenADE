//! Localhost HTTP API (PRD R3's IPC seam).
//!
//! The desktop app — and anything else, e.g. a CLI — attaches to sessions
//! through this API. It binds to loopback only; there is no authentication
//! story yet because nothing is exposed off-host (revisit before any remote
//! mode, PRD P2).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::daemon::{Daemon, DaemonError, FromInboxRequest, LaunchSessionRequest, PickupRequest};

/// Build the API router around a daemon.
pub fn router(daemon: Arc<Daemon>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session).delete(kill_session))
        .route("/sessions/{id}/scrollback", get(get_scrollback))
        .route("/sessions/{id}/input", post(post_input))
        .route("/sessions/{id}/worktree", delete(delete_worktree))
        .route("/sessions/{id}/diff", get(get_diff))
        .route("/sessions/{id}/files", get(get_files))
        .route("/sessions/{id}/file", get(get_file))
        .route("/sessions/{id}/skills", get(get_skills))
        .route("/projects/prs", get(get_project_prs))
        .route("/sessions/{id}/share", post(post_share))
        .route("/sessions/pickup", post(post_pickup))
        .route("/workspace/sessions", get(workspace_sessions))
        .route("/workspace/sessions/{sid}", get(workspace_session))
        .route("/signals", post(post_signals))
        .route("/inbox", get(list_inbox))
        .route("/inbox/{iid}", get(get_inbox_item))
        .route("/inbox/{iid}/accept", post(accept_inbox_item))
        .route("/inbox/{iid}/dismiss", post(dismiss_inbox_item))
        .route("/sessions/from-inbox", post(post_from_inbox))
        .route("/sessions/{id}/inbox-outcome", post(post_inbox_outcome))
        .route("/sessions/{id}/artifact", post(post_artifact))
        .route("/sessions/{id}/handoff", post(post_handoff))
        .route("/projects", get(list_projects))
        .route("/config", get(get_config).put(put_config))
        // The daemon binds loopback-only; the UI (vite dev server, Tauri
        // webview) is a different origin, so CORS must be open for it.
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(daemon)
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<DaemonError> for ApiError {
    fn from(err: DaemonError) -> Self {
        let status = match &err {
            DaemonError::NotFound(_) => StatusCode::NOT_FOUND,
            DaemonError::Worktree(crate::worktree::WorktreeError::Dirty(_)) => StatusCode::CONFLICT,
            DaemonError::MainCheckout => StatusCode::CONFLICT,
            DaemonError::Workspace(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(status, err.to_string())
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

/// Daemon configuration + memory status for the UI (first-run onboarding
/// and settings). The token itself is never echoed back.
fn config_status(daemon: &Daemon) -> serde_json::Value {
    let settings = daemon.settings();
    let gh_bin = catalog_mcp::github::resolve_gh_bin();
    let gh_authenticated = gh_bin
        .as_ref()
        .map(|bin| catalog_mcp::github::gh_auth_warning(bin).is_none());
    serde_json::json!({
        "onboarded": settings.effective_onboarded(),
        "backstage_base_url": settings
            .effective_backstage()
            .map(|c| c.base_url),
        "backstage_token_set": settings
            .effective_backstage()
            .is_some_and(|c| c.token.is_some()),
        "memory_repo": settings.effective_memory_repo(),
        "memory_sources": daemon.memory_sources(),
        "gh_found": gh_bin.is_some(),
        "gh_authenticated": gh_authenticated,
        "server_url": settings.server_url,
        "server_token_set": settings.server_token.as_deref().is_some_and(|t| !t.is_empty())
            || std::env::var(crate::workspace::SERVER_TOKEN_ENV).is_ok_and(|v| !v.is_empty()),
        "server_workspace": settings.server_workspace,
        "workspace_configured": settings.effective_workspace().is_some(),
        "inbox_backend": daemon.inbox_backend().name(),
    })
}

async fn get_config(State(daemon): State<Arc<Daemon>>) -> Json<serde_json::Value> {
    // The gh probe is a subprocess; keep the async runtime clean. The task
    // only fails if it panics, which would be a bug worth crashing on.
    let status = tokio::task::spawn_blocking(move || config_status(&daemon))
        .await
        .expect("config status task panicked");
    Json(status)
}

async fn put_config(
    State(daemon): State<Arc<Daemon>>,
    Json(mut settings): Json<crate::config::Settings>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(repo) = settings.memory_repo.as_ref().filter(|r| !r.is_empty()) {
        if repo.split('/').filter(|part| !part.is_empty()).count() != 2 {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                format!("memory_repo must be owner/name (got {repo:?})"),
            ));
        }
    }
    // The token is never echoed back by GET, so a settings edit that leaves
    // the field untouched (absent) keeps the stored token; an explicit
    // empty string clears it.
    if settings.backstage_token.is_none() {
        settings.backstage_token = daemon.settings().backstage_token;
    }
    if settings.server_token.is_none() {
        settings.server_token = daemon.settings().server_token;
    }
    let result = tokio::task::spawn_blocking(move || {
        daemon
            .apply_settings(settings)
            .map(|()| config_status(&daemon))
    })
    .await
    .expect("config apply task panicked");
    match result {
        Ok(status) => Ok(Json(status)),
        Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

/// Share a session's harness-neutral record to the team workspace.
async fn post_share(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let shared = daemon.share(id).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(shared).unwrap()),
    ))
}

/// Pick a shared workspace session up as a new local session (any harness).
async fn post_pickup(
    State(daemon): State<Arc<Daemon>>,
    Json(req): Json<PickupRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let meta = daemon.pickup(req).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(meta).unwrap()),
    ))
}

#[derive(Deserialize)]
struct WorkspaceListQuery {
    entity: Option<String>,
}

/// Team view data: the workspace's shared sessions, proxied through the
/// daemon (which holds the member token — it never reaches the browser).
async fn workspace_sessions(
    State(daemon): State<Arc<Daemon>>,
    Query(q): Query<WorkspaceListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = daemon
        .workspace_client()
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no workspace configured".into()))?;
    let sessions = client
        .sessions(q.entity.as_deref())
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

/// One shared session's full record (read-only viewer).
async fn workspace_session(
    State(daemon): State<Arc<Daemon>>,
    Path(sid): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = daemon
        .workspace_client()
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no workspace configured".into()))?;
    let detail = client
        .session(sid)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(serde_json::json!({
        "session": detail.session,
        "markdown": detail.markdown,
        "events": detail.events,
    })))
}

/// Signal errors keep the workspace-client error contract: bad input is a
/// 400, everything else (unreachable server, rejected token) a 502-style
/// gateway error the UI can show verbatim.
fn inbox_err(e: String) -> ApiError {
    if e == "not found" {
        ApiError(StatusCode::NOT_FOUND, e)
    } else if e.contains("must be") || e.contains("bad signal") || e.contains("reason must") {
        ApiError(StatusCode::BAD_REQUEST, e)
    } else if e.contains("already decided") {
        ApiError(StatusCode::CONFLICT, e)
    } else {
        ApiError(StatusCode::BAD_GATEWAY, e)
    }
}

/// Ingest signals into the active inbox (team server when configured,
/// the embedded local inbox otherwise) — one surface either way, and the
/// member token never reaches the browser.
async fn post_signals(
    State(daemon): State<Arc<Daemon>>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon
        .inbox_backend()
        .post_signals(body)
        .await
        .map_err(inbox_err)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct InboxListQuery {
    status: Option<String>,
}

async fn list_inbox(
    State(daemon): State<Arc<Daemon>>,
    Query(q): Query<InboxListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon
        .inbox_backend()
        .inbox(q.status)
        .await
        .map_err(inbox_err)?;
    Ok(Json(result))
}

async fn get_inbox_item(
    State(daemon): State<Arc<Daemon>>,
    Path(iid): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon
        .inbox_backend()
        .inbox_item(iid)
        .await
        .map_err(inbox_err)?;
    Ok(Json(result))
}

async fn accept_inbox_item(
    State(daemon): State<Arc<Daemon>>,
    Path(iid): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon
        .inbox_backend()
        .accept(iid)
        .await
        .map_err(inbox_err)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct DismissBody {
    reason: String,
}

async fn dismiss_inbox_item(
    State(daemon): State<Arc<Daemon>>,
    Path(iid): Path<i64>,
    Json(body): Json<DismissBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon
        .inbox_backend()
        .dismiss(iid, body.reason)
        .await
        .map_err(inbox_err)?;
    Ok(Json(result))
}

/// Start a triage session from an inbox item.
async fn post_from_inbox(
    State(daemon): State<Arc<Daemon>>,
    Json(req): Json<FromInboxRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let meta = daemon.start_from_inbox(req).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(meta).unwrap()),
    ))
}

/// Record the session branch's PR fate into outcome memory (idempotent).
async fn post_inbox_outcome(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let result = daemon.record_inbox_outcome(id).await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

/// Read one file from the session's working directory (Files tab viewer).
async fn get_file(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
    Query(q): Query<FileQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let content = daemon.read_workdir_file(id, &q.path)?;
    Ok(Json(
        serde_json::json!({ "path": q.path, "content": content }),
    ))
}

/// Reusable agent skills discovered in the session's working directory.
async fn get_skills(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(serde_json::json!({ "skills": daemon.skills(id)? })))
}

#[derive(Deserialize)]
struct PrQuery {
    repo: std::path::PathBuf,
}

/// Open pull requests for a project, via the user's local gh CLI.
async fn get_project_prs(Query(q): Query<PrQuery>) -> Json<serde_json::Value> {
    // gh is a subprocess; keep the async runtime clean. Panic = bug.
    tokio::task::spawn_blocking(move || Json(Daemon::pull_requests(&q.repo)))
        .await
        .expect("pr list task panicked")
}

#[derive(Deserialize)]
struct ListQuery {
    /// Filter to sessions launched from this catalog entity (includes
    /// historical sessions from the index, newest first). This is the data
    /// source for per-entity session views (e.g. a Backstage plugin).
    entity: Option<String>,
}

async fn list_sessions(
    State(daemon): State<Arc<Daemon>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match q.entity {
        Some(entity_ref) => {
            let records = daemon
                .store()
                .sessions_for_entity(&entity_ref)
                .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(Json(serde_json::json!({ "sessions": records })))
        }
        None => Ok(Json(serde_json::json!({ "sessions": daemon.list() }))),
    }
}

async fn create_session(
    State(daemon): State<Arc<Daemon>>,
    Json(req): Json<LaunchSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // Entity-launched sessions get catalog context (async fetch), degrading
    // to no bundle when the catalog can't resolve it. With no entity named,
    // the session grounds itself in the repo's own GitHub origin remote —
    // memory works without the user thinking about it. The inferred entity
    // is only adopted when it actually resolves to context.
    let mut req = req;
    let bundle = match &req.entity_ref {
        Some(entity_ref) => daemon.build_bundle(entity_ref, Some(&req.repo_root)).await,
        None => match daemon.infer_entity_ref(&req.repo_root) {
            Some(inferred) => {
                let bundle = daemon.build_bundle(&inferred, Some(&req.repo_root)).await;
                if bundle.is_some() {
                    tracing::info!("auto-grounded session in {inferred} (repo origin remote)");
                    req.entity_ref = Some(inferred);
                }
                bundle
            }
            None => None,
        },
    };
    // PTY spawn + git are blocking; keep the async runtime clean.
    let meta = tokio::task::spawn_blocking(move || daemon.launch(req, bundle))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok((StatusCode::CREATED, Json(meta)))
}

async fn get_session(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(daemon.get(id)?))
}

async fn get_scrollback(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let text = daemon.scrollback(id)?;
    Ok(Json(serde_json::json!({ "scrollback": text })))
}

async fn get_diff(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let diff = tokio::task::spawn_blocking(move || daemon.diff(id))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(serde_json::json!({ "diff": diff })))
}

async fn get_files(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let files = tokio::task::spawn_blocking(move || daemon.files(id))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(serde_json::json!({ "files": files })))
}

async fn list_projects(State(daemon): State<Arc<Daemon>>) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(serde_json::json!({ "projects": daemon.projects()? })))
}

async fn post_artifact(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let info = tokio::task::spawn_blocking(move || daemon.publish_artifact(id))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok((StatusCode::CREATED, Json(info)))
}

async fn post_handoff(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
    Json(req): Json<crate::daemon::HandoffRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let meta = tokio::task::spawn_blocking(move || daemon.handoff(id, req))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok((StatusCode::CREATED, Json(meta)))
}

#[derive(Deserialize)]
struct InputBody {
    data: String,
}

async fn post_input(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
    Json(body): Json<InputBody>,
) -> Result<impl IntoResponse, ApiError> {
    daemon.write_input(id, body.data.as_bytes())?;
    Ok(StatusCode::NO_CONTENT)
}

async fn kill_session(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(daemon.kill(id)?))
}

#[derive(Deserialize)]
struct CleanupQuery {
    #[serde(default)]
    force: bool,
}

async fn delete_worktree(
    State(daemon): State<Arc<Daemon>>,
    Path(id): Path<Uuid>,
    Query(q): Query<CleanupQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let force = q.force;
    tokio::task::spawn_blocking(move || daemon.cleanup_worktree(id, force))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
