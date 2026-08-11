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

use crate::daemon::{Daemon, DaemonError, LaunchSessionRequest};

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
