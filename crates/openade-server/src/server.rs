//! HTTP API: bearer-token-authenticated workspace + session endpoints.
//!
//! Two token classes: the admin token (from `OPENADE_SERVER_ADMIN_TOKEN`)
//! manages member tokens; member tokens do everything else. All routes
//! except `/health` require a token.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::signal::{DismissReason, SignalIn};
use crate::store::{Store, StoreError, DEFAULT_ORG};

/// Shared application state.
pub struct AppState {
    pub store: Store,
    pub admin_token: String,
}

/// Who a request is authenticated as.
enum Caller {
    Admin,
    Member { org: i64, name: String },
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound => ApiError(StatusCode::NOT_FOUND, "not found".into()),
            StoreError::Conflict(msg) => ApiError(StatusCode::CONFLICT, msg),
            StoreError::Db(e) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}

fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Caller, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            ApiError(
                StatusCode::UNAUTHORIZED,
                "missing bearer token (member token from your server admin)".into(),
            )
        })?;
    if !state.admin_token.is_empty() && token == state.admin_token {
        return Ok(Caller::Admin);
    }
    match state.store.member_for_token(token)? {
        Some((org, name)) => Ok(Caller::Member { org, name }),
        None => Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid or revoked token".into(),
        )),
    }
}

/// A member (not the admin); admins are for token management only.
fn member(state: &AppState, headers: &HeaderMap) -> Result<(i64, String), ApiError> {
    match authenticate(state, headers)? {
        Caller::Member { org, name } => Ok((org, name)),
        Caller::Admin => Ok((DEFAULT_ORG, "admin".to_string())),
    }
}

fn admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    match authenticate(state, headers)? {
        Caller::Admin => Ok(()),
        Caller::Member { .. } => Err(ApiError(
            StatusCode::FORBIDDEN,
            "admin token required".into(),
        )),
    }
}

/// Build the API router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/whoami", get(whoami))
        .route("/tokens", get(list_tokens).post(mint_token))
        .route("/tokens/{id}/revoke", post(revoke_token))
        .route("/workspaces", get(list_workspaces).post(create_workspace))
        .route("/workspaces/{id}", get(get_workspace))
        .route(
            "/workspaces/{id}/sessions",
            get(list_sessions).post(upload_session),
        )
        .route("/workspaces/{id}/sessions/{sid}", get(get_session))
        .route(
            "/workspaces/{id}/sessions/{sid}/verdict",
            post(set_session_verdict),
        )
        .route("/signals", post(ingest_signals))
        .route("/inbox", get(list_inbox))
        .route("/inbox/{id}", get(get_inbox_item))
        .route("/inbox/{id}/accept", post(accept_inbox_item))
        .route("/inbox/{id}/dismiss", post(dismiss_inbox_item))
        .route("/inbox/{id}/outcomes", post(record_inbox_outcome))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}

/// One signal or a batch — both are valid `POST /signals` bodies.
#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(SignalIn),
    Many(Vec<SignalIn>),
}

/// Ingest normalized signals (the generic webhook). Dedup is by
/// fingerprint; recurrences update rather than duplicate, and may
/// escalate a dismissed item back into the queue.
async fn ingest_signals(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<OneOrMany>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    let signals = match body {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    };
    let (mut inserted, mut updated, mut escalated) = (0, 0, 0);
    for sig in &signals {
        if sig.source.trim().is_empty() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "source must be non-empty".into(),
            ));
        }
        if sig.title.trim().is_empty() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "title must be non-empty".into(),
            ));
        }
        let outcome = state.store.ingest_signal(org, sig)?;
        if outcome.inserted {
            inserted += 1;
        } else {
            updated += 1;
        }
        if outcome.escalated {
            escalated += 1;
        }
    }
    Ok(Json(serde_json::json!({
        "received": signals.len(),
        "inserted": inserted,
        "updated": updated,
        "escalated": escalated,
    })))
}

#[derive(Deserialize)]
struct InboxQuery {
    status: Option<String>,
}

async fn list_inbox(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<InboxQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    let items = state.store.inbox(org, q.status.as_deref())?;
    Ok(Json(serde_json::json!({ "items": items })))
}

async fn get_inbox_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    let detail = state.store.inbox_item(org, id)?;
    Ok(Json(
        serde_json::to_value(detail).expect("detail serializes"),
    ))
}

/// Accept an item: the actor is the authenticated member (never
/// client-supplied), so every teammate's inbox shows who took the work.
async fn accept_inbox_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, name) = member(&state, &headers)?;
    let item = state.store.accept_item(org, id, &name)?;
    Ok(Json(serde_json::to_value(item).expect("item serializes")))
}

#[derive(Deserialize)]
struct DismissBody {
    reason: String,
}

async fn dismiss_inbox_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<DismissBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, name) = member(&state, &headers)?;
    let reason: DismissReason = serde_json::from_value(serde_json::Value::String(body.reason))
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "reason must be one of intended_behavior|wont_fix|duplicate|bad_evidence".into(),
            )
        })?;
    let item = state.store.dismiss_item(org, id, reason, &name)?;
    Ok(Json(serde_json::to_value(item).expect("item serializes")))
}

#[derive(Deserialize)]
struct OutcomeBody {
    kind: String,
    #[serde(default)]
    pr_url: Option<String>,
    #[serde(default)]
    note: Option<String>,
}

async fn record_inbox_outcome(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<OutcomeBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let (org, _) = member(&state, &headers)?;
    let inserted = state.store.record_outcome(
        org,
        id,
        &body.kind,
        body.pr_url.as_deref(),
        body.note.as_deref(),
    )?;
    Ok((
        if inserted {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(serde_json::json!({ "recorded": inserted })),
    ))
}

#[derive(Deserialize)]
struct VerdictBody {
    verdict: String,
}

/// Record what reality decided about a shared session (merged / closed /
/// reverted) — every member's context bundles pick it up.
async fn set_session_verdict(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_ws, sid)): Path<(i64, i64)>,
    Json(body): Json<VerdictBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    state.store.set_session_verdict(org, sid, &body.verdict)?;
    Ok(Json(serde_json::json!({ "verdict": body.verdict })))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn whoami(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, name) = member(&state, &headers)?;
    Ok(Json(serde_json::json!({ "org": org, "member": name })))
}

#[derive(Deserialize)]
struct MintRequest {
    name: String,
}

async fn mint_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<MintRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    admin(&state, &headers)?;
    let (id, secret) = state.store.mint_token(DEFAULT_ORG, &req.name)?;
    Ok(Json(
        serde_json::json!({ "id": id, "name": req.name, "token": secret }),
    ))
}

async fn list_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    admin(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "tokens": state.store.tokens(DEFAULT_ORG)? }),
    ))
}

async fn revoke_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    admin(&state, &headers)?;
    state.store.revoke_token(DEFAULT_ORG, id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct WorkspaceRequest {
    title: String,
    #[serde(default)]
    description: String,
}

async fn create_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<WorkspaceRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let (org, _) = member(&state, &headers)?;
    let ws = state
        .store
        .create_workspace(org, &req.title, &req.description)?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(ws).unwrap())))
}

async fn list_workspaces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    Ok(Json(
        serde_json::json!({ "workspaces": state.store.workspaces(org)? }),
    ))
}

async fn get_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    Ok(Json(
        serde_json::to_value(state.store.workspace(org, id)?).unwrap(),
    ))
}

#[derive(Deserialize)]
struct UploadRequest {
    title: String,
    harness: String,
    #[serde(default)]
    entity_ref: Option<String>,
    #[serde(default)]
    branch: Option<String>,
    summary: String,
    markdown: String,
    #[serde(default)]
    events: serde_json::Value,
}

async fn upload_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(req): Json<UploadRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let (org, name) = member(&state, &headers)?;
    let session = state.store.upload_session(
        org,
        id,
        &req.title,
        &req.harness,
        req.entity_ref.as_deref(),
        req.branch.as_deref(),
        &req.summary,
        &req.markdown,
        &req.events,
        &name,
    )?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(session).unwrap()),
    ))
}

#[derive(Deserialize)]
struct SessionsQuery {
    entity: Option<String>,
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<SessionsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    let sessions = state.store.sessions(org, id, q.entity.as_deref())?;
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_id, sid)): Path<(i64, i64)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (org, _) = member(&state, &headers)?;
    Ok(Json(
        serde_json::to_value(state.store.session(org, sid)?).unwrap(),
    ))
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
