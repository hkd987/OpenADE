use super::*;
use std::sync::Arc;

/// Boot a real openade-server on an ephemeral port; returns (base_url,
/// member token, workspace id).
pub(crate) async fn boot_server(tmp: &std::path::Path) -> (String, String, i64) {
    let store = openade_server::store::Store::open(tmp).unwrap();
    let (_, token) = store
        .mint_token(openade_server::store::DEFAULT_ORG, "casey")
        .unwrap();
    let ws = store
        .create_workspace(openade_server::store::DEFAULT_ORG, "Team", "")
        .unwrap();
    let app = openade_server::server::router(Arc::new(openade_server::server::AppState {
        store,
        admin_token: "admin".into(),
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), token, ws.id)
}

fn record(title: &str, entity: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "title": title,
        "harness": "claude-code",
        "entity_ref": entity,
        "branch": "openade/x",
        "summary": format!("{title} summary"),
        "markdown": format!("# Session: {title}"),
        "events": [{ "kind": "prompt", "payload": { "text": "do it" } }],
    })
}

#[tokio::test]
async fn client_round_trips_against_a_real_server() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, token, ws) = boot_server(tmp.path()).await;
    let client = WorkspaceClient::new(format!("{url}/"), &token, ws);

    let uploaded = client
        .upload(&record("add retries", Some("repo:a/p")))
        .await
        .unwrap();
    assert_eq!(uploaded.shared_by, "casey");
    client.upload(&record("other", None)).await.unwrap();

    let all = client.sessions(None).await.unwrap();
    assert_eq!(all.len(), 2);
    let filtered = client.sessions(Some("repo:a/p")).await.unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].title, "add retries");

    let detail = client.session(uploaded.id).await.unwrap();
    assert_eq!(detail.markdown, "# Session: add retries");
    assert_eq!(detail.events[0]["kind"], "prompt");

    // Error contract: bad token names the fix; missing session surfaces
    // the server's status; unreachable server is a clean message.
    let bad = WorkspaceClient::new(&url, "junk", ws);
    let err = bad.sessions(None).await.unwrap_err();
    assert!(err.contains("member"), "{err}");
    let err = client.session(9999).await.unwrap_err();
    assert!(err.contains("404"), "{err}");
    let gone = WorkspaceClient::new("http://127.0.0.1:1", &token, ws);
    let err = gone.upload(&record("x", None)).await.unwrap_err();
    assert!(err.contains("unreachable"), "{err}");
    let err = gone.session(1).await.unwrap_err();
    assert!(err.contains("unreachable"), "{err}");
}
