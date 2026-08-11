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
    // to no bundle when the catalog can't resolve it.
    let bundle = match &req.entity_ref {
        Some(entity_ref) => daemon.build_bundle(entity_ref).await,
        None => None,
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
mod tests {
    use super::*;
    use http_body_util::BodyExt;
    use openade_core::Harness;
    use std::process::Command;
    use tower::util::ServiceExt;

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
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "T"]);
        std::fs::write(dir.join("README.md"), "hi\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "init"]);
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn request(
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> axum::http::Request<axum::body::Body> {
        let builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        match body {
            Some(v) => builder.body(axum::body::Body::from(v.to_string())).unwrap(),
            None => builder.body(axum::body::Body::empty()).unwrap(),
        }
    }

    #[tokio::test]
    async fn health_and_session_lifecycle_over_http() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = Arc::new(Daemon::open(tmp.path().join("data")).unwrap());
        let app = router(daemon);

        // Health.
        let res = app
            .clone()
            .oneshot(request("GET", "/health", None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["status"], "ok");

        // Empty list.
        let res = app
            .clone()
            .oneshot(request("GET", "/sessions", None))
            .await
            .unwrap();
        assert_eq!(
            body_json(res).await["sessions"].as_array().unwrap().len(),
            0
        );

        // Create a session (command override: the harness CLIs are not
        // installed in CI).
        let req = LaunchSessionRequest {
            title: "http test".into(),
            harness: Harness::ClaudeCode,
            repo_root: repo.clone(),
            entity_ref: None,
            prompt: None,
            mcp_servers: vec![],
            command_override: Some(
                crate::pty::CommandSpec::new("sh")
                    .arg("-c")
                    .arg("printf over-http; sleep 5"),
            ),
        };
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                "/sessions",
                Some(serde_json::to_value(&req).unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let meta = body_json(res).await;
        let id = meta["id"].as_str().unwrap().to_string();
        assert_eq!(meta["state"], "running");

        // Scrollback appears.
        let mut saw_output = false;
        for _ in 0..100 {
            let res = app
                .clone()
                .oneshot(request("GET", &format!("/sessions/{id}/scrollback"), None))
                .await
                .unwrap();
            if body_json(res).await["scrollback"]
                .as_str()
                .unwrap()
                .contains("over-http")
            {
                saw_output = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(saw_output);

        // Kill it; state becomes terminal.
        let res = app
            .clone()
            .oneshot(request("DELETE", &format!("/sessions/{id}"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["state"], "failed");

        // Unknown session is a 404.
        let res = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/sessions/{}", Uuid::new_v4()),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    fn test_world() -> (tempfile::TempDir, Router, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        init_repo(&repo);
        let daemon = Arc::new(Daemon::open(tmp.path().join("data")).unwrap());
        let app = router(daemon);
        (tmp, app, repo)
    }

    async fn create_test_session(
        app: &Router,
        repo: &std::path::Path,
        cmd: &str,
    ) -> serde_json::Value {
        let req = LaunchSessionRequest {
            title: "http test".into(),
            harness: Harness::ClaudeCode,
            repo_root: repo.to_path_buf(),
            entity_ref: None,
            prompt: Some("do the thing".into()),
            mcp_servers: vec![],
            command_override: Some(crate::pty::CommandSpec::new("sh").arg("-c").arg(cmd)),
        };
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                "/sessions",
                Some(serde_json::to_value(&req).unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        body_json(res).await
    }

    #[tokio::test]
    async fn input_diff_files_and_projects_over_http() {
        let (_tmp, app, repo) = test_world();
        let meta = create_test_session(&app, &repo, "read x; exit 0").await;
        let id = meta["id"].as_str().unwrap();
        let worktree = std::path::PathBuf::from(meta["worktree_path"].as_str().unwrap());

        // Input endpoint (204).
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                &format!("/sessions/{id}/input"),
                Some(serde_json::json!({ "data": "y\n" })),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // Diff reflects a worktree edit.
        std::fs::write(worktree.join("README.md"), "hi\nedited over http\n").unwrap();
        let res = app
            .clone()
            .oneshot(request("GET", &format!("/sessions/{id}/diff"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(body_json(res).await["diff"]
            .as_str()
            .unwrap()
            .contains("+edited over http"));

        // File listing.
        let res = app
            .clone()
            .oneshot(request("GET", &format!("/sessions/{id}/files"), None))
            .await
            .unwrap();
        let files = body_json(res).await;
        assert!(files["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "README.md"));

        // Project list knows the repo.
        let res = app
            .clone()
            .oneshot(request("GET", "/projects", None))
            .await
            .unwrap();
        let projects = body_json(res).await;
        assert_eq!(projects["projects"][0], repo.to_string_lossy().as_ref());
    }

    #[tokio::test]
    async fn sessions_can_be_filtered_by_entity() {
        let (_tmp, app, repo) = test_world();
        // One session with an entity, one without.
        let mut with_entity = LaunchSessionRequest {
            title: "entity session".into(),
            harness: Harness::ClaudeCode,
            repo_root: repo.clone(),
            entity_ref: Some("component:default/payments-api".into()),
            prompt: None,
            mcp_servers: vec![],
            command_override: Some(crate::pty::CommandSpec::new("sh").arg("-c").arg("true")),
        };
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                "/sessions",
                Some(serde_json::to_value(&with_entity).unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        with_entity.entity_ref = None;
        with_entity.title = "plain session".into();
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                "/sessions",
                Some(serde_json::to_value(&with_entity).unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);

        let res = app
            .clone()
            .oneshot(request(
                "GET",
                "/sessions?entity=component:default/payments-api",
                None,
            ))
            .await
            .unwrap();
        let body = body_json(res).await;
        let sessions = body["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["title"], "entity session");

        // Unfiltered list still returns both.
        let res = app
            .clone()
            .oneshot(request("GET", "/sessions", None))
            .await
            .unwrap();
        assert_eq!(
            body_json(res).await["sessions"].as_array().unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn artifact_and_handoff_over_http() {
        let (_tmp, app, repo) = test_world();
        let meta = create_test_session(&app, &repo, "sleep 20").await;
        let id = meta["id"].as_str().unwrap();

        // Artifact → 201 with a review branch.
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                &format!("/sessions/{id}/artifact"),
                Some(serde_json::json!({})),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let artifact = body_json(res).await;
        assert!(artifact["branch"]
            .as_str()
            .unwrap()
            .starts_with("openade/knowledge-"));

        // Handoff → 201 with a new session on another harness.
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                &format!("/sessions/{id}/handoff"),
                Some(serde_json::json!({
                    "harness": "gemini-cli",
                    "command_override": {"program": "sh", "args": ["-c", "true"]},
                })),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let new_meta = body_json(res).await;
        assert_eq!(new_meta["harness"], "gemini-cli");
        assert_eq!(new_meta["worktree_path"], meta["worktree_path"]);
    }

    #[tokio::test]
    async fn worktree_cleanup_respects_dirty_guard_over_http() {
        let (_tmp, app, repo) = test_world();
        let meta = create_test_session(&app, &repo, "true").await;
        let id = meta["id"].as_str().unwrap();
        let worktree = std::path::PathBuf::from(meta["worktree_path"].as_str().unwrap());

        // Make it dirty: cleanup without force → 409.
        std::fs::write(worktree.join("wip.txt"), "uncommitted").unwrap();
        let res = app
            .clone()
            .oneshot(request("DELETE", &format!("/sessions/{id}/worktree"), None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);

        // Forced cleanup → 204 and gone.
        let res = app
            .clone()
            .oneshot(request(
                "DELETE",
                &format!("/sessions/{id}/worktree?force=true"),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert!(!worktree.exists());
    }

    #[tokio::test]
    async fn launch_errors_are_surfaced_as_api_errors() {
        let (_tmp, app, _repo) = test_world();
        let req = LaunchSessionRequest {
            title: "bad".into(),
            harness: Harness::ClaudeCode,
            repo_root: "/definitely/not/a/repo".into(),
            entity_ref: None,
            prompt: None,
            mcp_servers: vec![],
            command_override: None,
        };
        let res = app
            .clone()
            .oneshot(request(
                "POST",
                "/sessions",
                Some(serde_json::to_value(&req).unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(res).await;
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("not a git repository"));
    }
}
