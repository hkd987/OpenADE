use super::*;
use http_body_util::BodyExt;
use tower::util::ServiceExt;

fn app() -> (tempfile::TempDir, Router) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    let app = router(Arc::new(AppState {
        store,
        admin_token: "admin-secret".into(),
    }));
    (tmp, app)
}

fn request(
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> axum::http::Request<axum::body::Body> {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    match body {
        Some(v) => builder.body(axum::body::Body::from(v.to_string())).unwrap(),
        None => builder.body(axum::body::Body::empty()).unwrap(),
    }
}

async fn json(res: Response) -> serde_json::Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn full_workspace_flow_over_http() {
    let (_tmp, app) = app();

    // Health is open; everything else requires a token.
    let res = app
        .clone()
        .oneshot(request("GET", "/health", None, None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = app
        .clone()
        .oneshot(request("GET", "/workspaces", None, None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = app
        .clone()
        .oneshot(request("GET", "/whoami", Some("junk"), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Admin mints a member token; members cannot mint.
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/tokens",
            Some("admin-secret"),
            Some(serde_json::json!({ "name": "casey" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let minted = json(res).await;
    let member = minted["token"].as_str().unwrap().to_string();
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/tokens",
            Some(&member),
            Some(serde_json::json!({ "name": "sneaky" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // whoami introspects the member; admins list tokens.
    let res = app
        .clone()
        .oneshot(request("GET", "/whoami", Some(&member), None))
        .await
        .unwrap();
    assert_eq!(json(res).await["member"], "casey");
    let res = app
        .clone()
        .oneshot(request("GET", "/tokens", Some("admin-secret"), None))
        .await
        .unwrap();
    assert_eq!(json(res).await["tokens"][0]["name"], "casey");

    // Workspace lifecycle + upload + browse + entity filter + detail.
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/workspaces",
            Some(&member),
            Some(serde_json::json!({ "title": "Payments", "description": "d" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let ws = json(res).await;
    let ws_id = ws["id"].as_i64().unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", "/workspaces", Some(&member), None))
        .await
        .unwrap();
    assert_eq!(json(res).await["workspaces"][0]["title"], "Payments");
    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/workspaces/{ws_id}"),
            Some(&member),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["title"], "Payments");

    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/workspaces/{ws_id}/sessions"),
            Some(&member),
            Some(serde_json::json!({
                "title": "add retries",
                "harness": "claude-code",
                "entity_ref": "repo:acme/payments",
                "branch": "openade/add-retries",
                "summary": "Added retries.",
                "markdown": "# Session\nretries",
                "events": [{ "kind": "prompt", "payload": { "text": "fix" } }],
            })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let uploaded = json(res).await;
    assert_eq!(uploaded["shared_by"], "casey");
    let sid = uploaded["id"].as_i64().unwrap();

    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/workspaces/{ws_id}/sessions?entity=repo:acme/payments"),
            Some(&member),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(json(res).await["sessions"][0]["title"], "add retries");

    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/workspaces/{ws_id}/sessions/{sid}"),
            Some(&member),
            None,
        ))
        .await
        .unwrap();
    let detail = json(res).await;
    assert_eq!(detail["markdown"], "# Session\nretries");
    assert_eq!(detail["events"][0]["kind"], "prompt");

    // Missing things 404; uploads into missing workspaces 404.
    let res = app
        .clone()
        .oneshot(request("GET", "/workspaces/99", Some(&member), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/workspaces/99/sessions",
            Some(&member),
            Some(serde_json::json!({
                "title": "x", "harness": "codex-cli", "summary": "s", "markdown": "m"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Revoke the member: their token stops working (admin-only op works,
    // revoking twice 404s).
    let token_id = minted["id"].as_i64().unwrap();
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/tokens/{token_id}/revoke"),
            Some("admin-secret"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = app
        .clone()
        .oneshot(request("GET", "/whoami", Some(&member), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/tokens/{token_id}/revoke"),
            Some("admin-secret"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // The admin can browse too (acts as a default-org member).
    let res = app
        .clone()
        .oneshot(request("GET", "/whoami", Some("admin-secret"), None))
        .await
        .unwrap();
    assert_eq!(json(res).await["member"], "admin");
}

#[tokio::test]
async fn empty_admin_token_never_grants_admin() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path()).unwrap();
    let app = router(Arc::new(AppState {
        store,
        admin_token: String::new(),
    }));
    // An empty bearer must not match the empty configured admin token.
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/tokens",
            Some(""),
            Some(serde_json::json!({ "name": "x" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn database_faults_become_500s_not_panics() {
    let (tmp, app) = app();
    // Break the schema out from under the running server.
    rusqlite::Connection::open(tmp.path().join("server.db"))
        .unwrap()
        .execute("DROP TABLE workspaces", [])
        .unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", "/workspaces", Some("admin-secret"), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(json(res).await["error"]
        .as_str()
        .unwrap()
        .contains("workspaces"));
}
