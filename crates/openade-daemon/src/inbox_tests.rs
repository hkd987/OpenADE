use super::*;

fn signal_json(title: &str, affected: i64) -> serde_json::Value {
    serde_json::json!({
        "source": "sentry",
        "kind": "exception",
        "severity": "high",
        "title": title,
        "body": format!("{title} stack"),
        "evidence": [{ "kind": "stack_trace", "label": "trace", "url": "https://s.example/1" }],
        "affected_count": affected,
    })
}

fn embedded(tmp: &std::path::Path) -> InboxBackend {
    InboxBackend::Embedded(Arc::new(Store::open(tmp).unwrap()))
}

#[tokio::test]
async fn embedded_backend_round_trips_the_full_triage_story() {
    let tmp = tempfile::tempdir().unwrap();
    let backend = embedded(tmp.path());
    assert_eq!(backend.name(), "local");

    // Single and batch ingestion, with validation errors surfaced.
    let res = backend.post_signals(signal_json("NPE", 5)).await.unwrap();
    assert_eq!(res["inserted"], 1);
    let res = backend
        .post_signals(serde_json::json!([
            signal_json("NPE", 9),
            signal_json("slow", 1)
        ]))
        .await
        .unwrap();
    assert_eq!(res["updated"], 1);
    assert_eq!(res["inserted"], 1);
    let err = backend
        .post_signals(serde_json::json!({ "source": " ", "kind": "exception",
            "severity": "low", "title": "x" }))
        .await
        .unwrap_err();
    assert!(err.contains("source"), "{err}");
    let err = backend
        .post_signals(serde_json::json!({ "source": "s", "kind": "exception",
            "severity": "low", "title": "  " }))
        .await
        .unwrap_err();
    assert!(err.contains("title"), "{err}");
    let err = backend
        .post_signals(serde_json::json!({ "nope": true }))
        .await
        .unwrap_err();
    assert!(err.contains("bad signal"), "{err}");

    // List, filter, detail.
    let items = backend.inbox(None).await.unwrap();
    assert_eq!(items["items"].as_array().unwrap().len(), 2);
    let id = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == "NPE")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let detail = backend.inbox_item(id).await.unwrap();
    assert_eq!(detail["item"]["affected_count"], 9);
    assert_eq!(detail["signals"][0]["evidence"][0]["label"], "trace");

    // Accept is attributed to the OS user locally.
    let taken = backend.accept(id).await.unwrap();
    assert_eq!(taken["status"], "accepted");
    assert_eq!(taken["decided_by"], local_actor());
    let err = backend.accept(id).await.unwrap_err();
    assert!(err.contains("already decided"), "{err}");
    assert_eq!(backend.accept(9999).await.unwrap_err(), "not found");

    // Dismiss validates the taxonomy and records outcome memory.
    let other = backend.inbox(Some("new".into())).await.unwrap()["items"][0]["id"]
        .as_i64()
        .unwrap();
    let err = backend.dismiss(other, "meh".into()).await.unwrap_err();
    assert!(err.contains("intended_behavior"), "{err}");
    let dismissed = backend.dismiss(other, "duplicate".into()).await.unwrap();
    assert_eq!(dismissed["status"], "dismissed");
    let detail = backend.inbox_item(other).await.unwrap();
    assert_eq!(detail["outcomes"][0]["kind"], "dismissed");

    // Outcome recording is idempotent per kind.
    let res = backend
        .record_outcome(id, "merged".into(), Some("https://gh/pr/1".into()), None)
        .await
        .unwrap();
    assert_eq!(res["recorded"], true);
    let res = backend
        .record_outcome(id, "merged".into(), None, None)
        .await
        .unwrap();
    assert_eq!(res["recorded"], false);
    assert_eq!(
        backend
            .record_outcome(777, "merged".into(), None, None)
            .await
            .unwrap_err(),
        "not found"
    );
}

#[test]
fn local_actor_prefers_the_os_user() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved = std::env::var("USER").ok();
    std::env::set_var("USER", "devon");
    assert_eq!(local_actor(), "devon");
    std::env::set_var("USER", "");
    assert_eq!(local_actor(), "local");
    std::env::remove_var("USER");
    assert_eq!(local_actor(), "local");
    match saved {
        Some(u) => std::env::set_var("USER", u),
        None => std::env::remove_var("USER"),
    }
}

/// Holding the env lock across awaits is fine in tests: the lock is what
/// keeps env-mutating tests from interleaving.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn remote_backend_proxies_the_team_inbox() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let (url, token, ws) = crate::workspace::tests::boot_server(tmp.path()).await;
    let backend = InboxBackend::Remote(WorkspaceClient::new(&url, &token, ws));
    assert_eq!(backend.name(), "remote");

    let res = backend
        .post_signals(signal_json("remote NPE", 4))
        .await
        .unwrap();
    assert_eq!(res["inserted"], 1);
    let items = backend.inbox(Some("new".into())).await.unwrap();
    let id = items["items"][0]["id"].as_i64().unwrap();
    let detail = backend.inbox_item(id).await.unwrap();
    assert_eq!(detail["item"]["title"], "remote NPE");

    // The SERVER stamps the actor from the member token — the daemon never
    // sends a name.
    let taken = backend.accept(id).await.unwrap();
    assert_eq!(taken["decided_by"], "casey");

    let res = backend
        .record_outcome(id, "merged".into(), Some("https://gh/pr/2".into()), None)
        .await
        .unwrap();
    assert_eq!(res["recorded"], true);

    // Dismissal + errors travel with the workspace-client error contract.
    backend.post_signals(signal_json("noise", 1)).await.unwrap();
    let other = backend.inbox(Some("new".into())).await.unwrap()["items"][0]["id"]
        .as_i64()
        .unwrap();
    let dismissed = backend.dismiss(other, "bad_evidence".into()).await.unwrap();
    assert_eq!(dismissed["status"], "dismissed");
    let err = backend.inbox_item(9999).await.unwrap_err();
    assert!(err.contains("404"), "{err}");

    let gone = InboxBackend::Remote(WorkspaceClient::new("http://127.0.0.1:1", &token, ws));
    for err in [
        gone.post_signals(signal_json("x", 1)).await.unwrap_err(),
        gone.inbox(None).await.unwrap_err(),
        gone.inbox_item(1).await.unwrap_err(),
        gone.accept(1).await.unwrap_err(),
        gone.dismiss(1, "duplicate".into()).await.unwrap_err(),
        gone.record_outcome(1, "merged".into(), None, None)
            .await
            .unwrap_err(),
    ] {
        assert!(err.contains("unreachable"), "{err}");
    }
}
