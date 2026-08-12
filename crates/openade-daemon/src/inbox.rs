//! The signal inbox, server-first with a local fallback.
//!
//! When a workspace server is configured (multiplayer), the inbox is the
//! TEAM's queue on that server — ingestion, triage state, and outcome
//! memory are shared, and every action is attributed to the member token.
//! Without a server, the daemon embeds the very same store
//! (`openade_server::store::Store`) locally, so the inbox works out of the
//! box, single-player, with an identical HTTP surface. Items do not
//! migrate between the two stores when a server is configured later.

use std::sync::Arc;

use openade_server::signal::{DismissReason, SignalIn};
use openade_server::store::{Store, StoreError, DEFAULT_ORG};

use crate::workspace::WorkspaceClient;

/// Where inbox operations go for one request.
pub enum InboxBackend {
    /// The configured workspace server (shared team inbox).
    Remote(WorkspaceClient),
    /// The daemon's embedded store (local inbox, no server needed).
    Embedded(Arc<Store>),
}

/// Local actions are attributed to the OS user so a later server upgrade
/// keeps meaningful history.
fn local_actor() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "local".to_string())
}

fn store_err(e: StoreError) -> String {
    match e {
        StoreError::NotFound => "not found".to_string(),
        other => other.to_string(),
    }
}

impl InboxBackend {
    /// "remote" or "local" — surfaced in `/config` so users can see which
    /// inbox they are looking at.
    pub fn name(&self) -> &'static str {
        match self {
            InboxBackend::Remote(_) => "remote",
            InboxBackend::Embedded(_) => "local",
        }
    }

    /// Ingest one or many normalized signals.
    pub async fn post_signals(&self, body: serde_json::Value) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => client.post_signals(&body).await,
            InboxBackend::Embedded(store) => {
                let signals: Vec<SignalIn> = if body.is_array() {
                    serde_json::from_value(body).map_err(|e| format!("bad signal: {e}"))?
                } else {
                    vec![serde_json::from_value(body).map_err(|e| format!("bad signal: {e}"))?]
                };
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let (mut inserted, mut updated, mut escalated) = (0, 0, 0);
                    for sig in &signals {
                        if sig.source.trim().is_empty() {
                            return Err("source must be non-empty".to_string());
                        }
                        if sig.title.trim().is_empty() {
                            return Err("title must be non-empty".to_string());
                        }
                        let outcome = store.ingest_signal(DEFAULT_ORG, sig).map_err(store_err)?;
                        if outcome.inserted {
                            inserted += 1;
                        } else {
                            updated += 1;
                        }
                        if outcome.escalated {
                            escalated += 1;
                        }
                    }
                    Ok(serde_json::json!({
                        "received": signals.len(),
                        "inserted": inserted,
                        "updated": updated,
                        "escalated": escalated,
                    }))
                })
                .await
                .expect("ingest task panicked")
            }
        }
    }

    /// Inbox items, optionally status-filtered.
    pub async fn inbox(&self, status: Option<String>) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => client.inbox(status.as_deref()).await,
            InboxBackend::Embedded(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let items = store
                        .inbox(DEFAULT_ORG, status.as_deref())
                        .map_err(store_err)?;
                    Ok(serde_json::json!({ "items": items }))
                })
                .await
                .expect("inbox task panicked")
            }
        }
    }

    /// One item with signals + fingerprint-anchored outcome history.
    pub async fn inbox_item(&self, id: i64) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => client.inbox_item(id).await,
            InboxBackend::Embedded(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let detail = store.inbox_item(DEFAULT_ORG, id).map_err(store_err)?;
                    Ok(serde_json::to_value(detail).expect("detail serializes"))
                })
                .await
                .expect("inbox item task panicked")
            }
        }
    }

    /// Accept an item; the actor is the token's member (remote) or the OS
    /// user (local).
    pub async fn accept(&self, id: i64) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => client.accept_item(id).await,
            InboxBackend::Embedded(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let item = store
                        .accept_item(DEFAULT_ORG, id, &local_actor())
                        .map_err(store_err)?;
                    Ok(serde_json::to_value(item).expect("item serializes"))
                })
                .await
                .expect("accept task panicked")
            }
        }
    }

    /// Dismiss an item with a structured reason (recorded in outcome
    /// memory).
    pub async fn dismiss(&self, id: i64, reason: String) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => client.dismiss_item(id, &reason).await,
            InboxBackend::Embedded(store) => {
                let parsed: DismissReason =
                    serde_json::from_value(serde_json::Value::String(reason)).map_err(|_| {
                        "reason must be one of intended_behavior|wont_fix|duplicate|bad_evidence"
                            .to_string()
                    })?;
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let item = store
                        .dismiss_item(DEFAULT_ORG, id, parsed, &local_actor())
                        .map_err(store_err)?;
                    Ok(serde_json::to_value(item).expect("item serializes"))
                })
                .await
                .expect("dismiss task panicked")
            }
        }
    }

    /// Record an outcome for an item (idempotent per kind).
    pub async fn record_outcome(
        &self,
        id: i64,
        kind: String,
        pr_url: Option<String>,
        note: Option<String>,
    ) -> Result<serde_json::Value, String> {
        match self {
            InboxBackend::Remote(client) => {
                client
                    .record_outcome(id, &kind, pr_url.as_deref(), note.as_deref())
                    .await
            }
            InboxBackend::Embedded(store) => {
                let store = store.clone();
                tokio::task::spawn_blocking(move || {
                    let recorded = store
                        .record_outcome(DEFAULT_ORG, id, &kind, pr_url.as_deref(), note.as_deref())
                        .map_err(store_err)?;
                    Ok(serde_json::json!({ "recorded": recorded }))
                })
                .await
                .expect("outcome task panicked")
            }
        }
    }
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;
