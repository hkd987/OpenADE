//! Client for the multiplayer workspace server (`openade-server`).
//!
//! Everything is optional and degrade-friendly: when no server is
//! configured the daemon behaves exactly as before. Sessions cross the
//! wire in OpenADE's harness-neutral representation only — the pickup
//! side re-renders them for whatever harness the next person uses.

use serde::{Deserialize, Serialize};

/// Environment overrides (always win over stored settings).
pub const SERVER_URL_ENV: &str = "OPENADE_SERVER_URL";
pub const SERVER_TOKEN_ENV: &str = "OPENADE_SERVER_TOKEN";
pub const SERVER_WORKSPACE_ENV: &str = "OPENADE_SERVER_WORKSPACE";

/// A configured connection to a workspace server.
#[derive(Debug, Clone)]
pub struct WorkspaceClient {
    pub base_url: String,
    token: String,
    pub workspace_id: i64,
}

/// A shared session as listed by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSession {
    pub id: i64,
    pub title: String,
    pub harness: String,
    #[serde(default)]
    pub entity_ref: Option<String>,
    pub summary: String,
    pub shared_by: String,
    pub uploaded_at: String,
    /// What reality decided about this session's work, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
}

/// Full detail of a shared session (harness-neutral record).
#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceSessionDetail {
    #[serde(flatten)]
    pub session: WorkspaceSession,
    pub markdown: String,
    #[serde(default)]
    pub events: serde_json::Value,
}

impl WorkspaceClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>, workspace_id: i64) -> Self {
        WorkspaceClient {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            workspace_id,
        }
    }

    fn client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn check(res: reqwest::Response) -> Result<reqwest::Response, String> {
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let detail = res
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(format!(
                "workspace server rejected the token ({detail}) — check your member \
                 token in Settings"
            ));
        }
        Err(format!("workspace server returned {status}: {detail}"))
    }

    /// Upload a session record to the configured workspace.
    pub async fn upload(&self, record: &serde_json::Value) -> Result<WorkspaceSession, String> {
        let res = self
            .client()
            .post(self.url(&format!("/workspaces/{}/sessions", self.workspace_id)))
            .bearer_auth(&self.token)
            .json(record)
            .send()
            .await
            .map_err(|e| format!("workspace server unreachable: {e}"))?;
        Self::check(res)
            .await?
            .json()
            .await
            .map_err(|e| format!("bad response from workspace server: {e}"))
    }

    /// Shared sessions, newest first, optionally entity-filtered.
    pub async fn sessions(&self, entity: Option<&str>) -> Result<Vec<WorkspaceSession>, String> {
        let mut req = self
            .client()
            .get(self.url(&format!("/workspaces/{}/sessions", self.workspace_id)))
            .bearer_auth(&self.token);
        if let Some(entity) = entity {
            req = req.query(&[("entity", entity)]);
        }
        let res = req
            .send()
            .await
            .map_err(|e| format!("workspace server unreachable: {e}"))?;
        let body: serde_json::Value = Self::check(res)
            .await?
            .json()
            .await
            .map_err(|e| format!("bad response from workspace server: {e}"))?;
        serde_json::from_value(body["sessions"].clone())
            .map_err(|e| format!("bad response from workspace server: {e}"))
    }

    /// Full record of one shared session.
    pub async fn session(&self, id: i64) -> Result<WorkspaceSessionDetail, String> {
        let res = self
            .client()
            .get(self.url(&format!("/workspaces/{}/sessions/{id}", self.workspace_id)))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| format!("workspace server unreachable: {e}"))?;
        Self::check(res)
            .await?
            .json()
            .await
            .map_err(|e| format!("bad response from workspace server: {e}"))
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, String> {
        let res = self
            .client()
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| format!("workspace server unreachable: {e}"))?;
        Self::check(res)
            .await?
            .json()
            .await
            .map_err(|e| format!("bad response from workspace server: {e}"))
    }

    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let res = self
            .client()
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("workspace server unreachable: {e}"))?;
        Self::check(res)
            .await?
            .json()
            .await
            .map_err(|e| format!("bad response from workspace server: {e}"))
    }

    /// Push normalized signals into the team inbox.
    pub async fn post_signals(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.post_json("/signals", body).await
    }

    /// The team inbox, optionally status-filtered.
    pub async fn inbox(&self, status: Option<&str>) -> Result<serde_json::Value, String> {
        let query = status.map(|s| format!("?status={s}")).unwrap_or_default();
        self.get_json(&format!("/inbox{query}")).await
    }

    /// One inbox item with signals + outcome history.
    pub async fn inbox_item(&self, id: i64) -> Result<serde_json::Value, String> {
        self.get_json(&format!("/inbox/{id}")).await
    }

    /// Take an item (the server stamps who, from this client's token).
    pub async fn accept_item(&self, id: i64) -> Result<serde_json::Value, String> {
        self.post_json(&format!("/inbox/{id}/accept"), &serde_json::json!({}))
            .await
    }

    /// Dismiss an item with a structured reason.
    pub async fn dismiss_item(&self, id: i64, reason: &str) -> Result<serde_json::Value, String> {
        self.post_json(
            &format!("/inbox/{id}/dismiss"),
            &serde_json::json!({ "reason": reason }),
        )
        .await
    }

    /// Record what reality decided about an item's work (idempotent).
    pub async fn record_outcome(
        &self,
        id: i64,
        kind: &str,
        pr_url: Option<&str>,
        note: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        self.post_json(
            &format!("/inbox/{id}/outcomes"),
            &serde_json::json!({ "kind": kind, "pr_url": pr_url, "note": note }),
        )
        .await
    }

    /// Record a verdict on a shared session.
    pub async fn set_verdict(&self, session_id: i64, verdict: &str) -> Result<(), String> {
        self.post_json(
            &format!(
                "/workspaces/{}/sessions/{session_id}/verdict",
                self.workspace_id
            ),
            &serde_json::json!({ "verdict": verdict }),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
pub(crate) mod tests;
