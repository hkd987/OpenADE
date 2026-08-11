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

async fn list_sessions(State(daemon): State<Arc<Daemon>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "sessions": daemon.list() }))
}

async fn create_session(
    State(daemon): State<Arc<Daemon>>,
    Json(req): Json<LaunchSessionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    // PTY spawn + git are blocking; keep the async runtime clean.
    let meta = tokio::task::spawn_blocking(move || daemon.launch(req))
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
}
