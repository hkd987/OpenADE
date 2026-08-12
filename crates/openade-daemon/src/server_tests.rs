use super::*;
use crate::daemon::CheckoutMode;
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
        checkout: CheckoutMode::default(),
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

async fn create_test_session(app: &Router, repo: &std::path::Path, cmd: &str) -> serde_json::Value {
    let req = LaunchSessionRequest {
        title: "http test".into(),
        harness: Harness::ClaudeCode,
        repo_root: repo.to_path_buf(),
        checkout: CheckoutMode::default(),
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

    // File viewer endpoint serves worktree files and 404s traversal.
    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/sessions/{id}/file?path=README.md"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body_json(res).await["content"]
        .as_str()
        .unwrap()
        .contains("edited over http"));
    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/sessions/{id}/file?path=../../../etc/passwd"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Skills endpoint lists discovered skills.
    std::fs::create_dir_all(worktree.join(".openade/skills")).unwrap();
    std::fs::write(
        worktree.join(".openade/skills/deploy.md"),
        "# Deploy\nShip it safely.\n",
    )
    .unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", &format!("/sessions/{id}/skills"), None))
        .await
        .unwrap();
    let skills = body_json(res).await;
    assert_eq!(skills["skills"][0]["name"], "deploy");
    assert_eq!(skills["skills"][0]["description"], "Ship it safely.");

    // Main-checkout sessions refuse worktree cleanup with a 409.
    let mut main_req = LaunchSessionRequest {
        title: "main mode".into(),
        harness: Harness::ClaudeCode,
        repo_root: repo.to_path_buf(),
        checkout: CheckoutMode::Main,
        entity_ref: None,
        prompt: None,
        mcp_servers: vec![],
        command_override: Some(crate::pty::CommandSpec::new("sh").arg("-c").arg("true")),
    };
    main_req.checkout = CheckoutMode::Main;
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/sessions",
            Some(serde_json::to_value(&main_req).unwrap()),
        ))
        .await
        .unwrap();
    let main_id = body_json(res).await["id"].as_str().unwrap().to_string();
    let res = app
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/sessions/{main_id}/worktree?force=true"),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // PR endpoint degrades cleanly (env untouched here; any answer is a
    // well-formed {prs: [...]} — the gh behaviors are unit-tested).
    let res = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/projects/prs?repo={}", repo.to_string_lossy()),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body_json(res).await["prs"].is_array());
}

// The env mutex must stay held across the HTTP awaits — the runtime is
// single-threaded and the env mutations span the whole test.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn config_endpoint_onboards_and_applies_settings_live() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for var in [
        "BACKSTAGE_BASE_URL",
        "BACKSTAGE_TOKEN",
        "OPENADE_MEMORY_REPO",
        "OPENADE_GITHUB_MEMORY",
    ] {
        std::env::remove_var(var);
    }
    let tmp = tempfile::tempdir().unwrap();
    // An authenticated gh: `auth status` succeeds.
    let shim = tmp.path().join("gh");
    std::fs::write(&shim, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    // ETXTBSY hardening (see memory_repo_tests): probe until runnable.
    for _ in 0..100 {
        match Command::new(&shim).arg("--probe").output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            _ => break,
        }
    }
    std::env::set_var("OPENADE_GH_BIN", &shim);

    let data = tmp.path().join("data");
    let daemon = Daemon::open(&data).unwrap();
    daemon.configure(&crate::config::Settings::load(&data));
    let app = router(Arc::new(daemon));

    // First run: not onboarded; gh detected + authenticated; the github
    // source is already active (zero config).
    let res = app
        .clone()
        .oneshot(request("GET", "/config", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let config = body_json(res).await;
    assert_eq!(config["onboarded"], false);
    assert_eq!(config["gh_found"], true);
    assert_eq!(config["gh_authenticated"], true);
    assert_eq!(config["memory_sources"], serde_json::json!(["github"]));
    assert!(config["memory_repo"].is_null());
    assert!(config["backstage_base_url"].is_null());

    // A malformed shared memory repo is rejected up front.
    let res = app
        .clone()
        .oneshot(request(
            "PUT",
            "/config",
            Some(serde_json::json!({ "memory_repo": "not-owner-name" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // Onboarding saves settings; they apply live and persist to disk.
    let res = app
        .clone()
        .oneshot(request(
            "PUT",
            "/config",
            Some(serde_json::json!({
                "backstage_base_url": "http://127.0.0.1:1/api",
                "backstage_token": "token",
                "memory_repo": "acme/team-memory",
                "onboarded": true,
            })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let config = body_json(res).await;
    assert_eq!(config["onboarded"], true);
    assert_eq!(config["backstage_base_url"], "http://127.0.0.1:1/api");
    assert_eq!(config["backstage_token_set"], true);
    assert_eq!(config["memory_repo"], "acme/team-memory");
    assert_eq!(
        config["memory_sources"],
        serde_json::json!(["github", "backstage"])
    );
    let stored = crate::config::Settings::load(&data);
    assert!(stored.onboarded);
    assert_eq!(stored.memory_repo.as_deref(), Some("acme/team-memory"));

    // A later settings edit that omits the token (it is never echoed back)
    // keeps the stored one; an explicit empty string clears it.
    let res = app
        .clone()
        .oneshot(request(
            "PUT",
            "/config",
            Some(serde_json::json!({
                "backstage_base_url": "http://127.0.0.1:1/api",
                "memory_repo": "acme/team-memory",
                "onboarded": true,
            })),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(res).await["backstage_token_set"], true);
    let res = app
        .clone()
        .oneshot(request(
            "PUT",
            "/config",
            Some(serde_json::json!({
                "backstage_base_url": "http://127.0.0.1:1/api",
                "backstage_token": "",
                "onboarded": true,
            })),
        ))
        .await
        .unwrap();
    assert_eq!(body_json(res).await["backstage_token_set"], false);

    // With gh disabled entirely, status says so (found=false, auth
    // unknown) so the UI can show install instructions.
    std::env::remove_var("OPENADE_GH_BIN");
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    let res = app
        .clone()
        .oneshot(request("GET", "/config", None))
        .await
        .unwrap();
    let config = body_json(res).await;
    assert_eq!(config["gh_found"], false);
    assert!(config["gh_authenticated"].is_null());
    std::env::remove_var("OPENADE_GITHUB_MEMORY");

    // Persistence failure surfaces as a 500 (real fault: the data dir is
    // gone and replaced by a plain file).
    std::fs::remove_dir_all(&data).unwrap();
    std::fs::write(&data, "not a dir").unwrap();
    let res = app
        .clone()
        .oneshot(request(
            "PUT",
            "/config",
            Some(serde_json::json!({ "onboarded": true })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// The env mutex must stay held across the HTTP awaits — the runtime is
// single-threaded and the env mutations span the whole test.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn sessions_auto_ground_in_the_repo_origin_remote_over_http() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let add_remote = |dir: &std::path::Path, url: &str| {
        let st = Command::new("git")
            .args(["-C", dir.to_str().unwrap(), "remote", "add", "origin", url])
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    add_remote(&repo, "https://github.com/acme/payments-service.git");
    let daemon = Arc::new(Daemon::open(tmp.path().join("data")).unwrap().with_catalog(
        std::sync::Arc::new(catalog_mcp::testutil::MockProvider::with_payments_graph()),
    ));
    let app = router(daemon);

    // Launch WITHOUT naming an entity: the session grounds itself in the
    // repo's own GitHub origin remote.
    let meta = create_test_session(&app, &repo, "true").await;
    assert_eq!(meta["entity_ref"], "repo:acme/payments-service");
    let wt = std::path::PathBuf::from(meta["worktree_path"].as_str().unwrap());
    let rules = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
    assert!(rules.contains("acme/payments-service"), "{rules}");

    // An origin that doesn't resolve to any memory entity: the inferred ref
    // is NOT adopted — the session launches plain rather than mislabeled.
    let ghost = tmp.path().join("ghost");
    std::fs::create_dir(&ghost).unwrap();
    init_repo(&ghost);
    add_remote(&ghost, "https://github.com/acme/ghost.git");
    let meta = create_test_session(&app, &ghost, "true").await;
    assert!(meta["entity_ref"].is_null(), "{meta}");

    std::env::remove_var("OPENADE_GH_BIN");
}

#[tokio::test]
async fn sessions_can_be_filtered_by_entity() {
    let (_tmp, app, repo) = test_world();
    // One session with an entity, one without.
    let mut with_entity = LaunchSessionRequest {
        title: "entity session".into(),
        harness: Harness::ClaudeCode,
        repo_root: repo.clone(),
        checkout: CheckoutMode::default(),
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
        checkout: CheckoutMode::default(),
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

// Holding the env lock across awaits is fine in tests: the lock is exactly
// what keeps concurrent env-mutating tests from interleaving.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn share_pickup_and_team_views_over_http() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for var in [
        crate::workspace::SERVER_URL_ENV,
        crate::workspace::SERVER_TOKEN_ENV,
        crate::workspace::SERVER_WORKSPACE_ENV,
    ] {
        std::env::remove_var(var);
    }
    let (tmp, app, repo) = test_world();
    let meta = create_test_session(&app, &repo, "true").await;
    let id = meta["id"].as_str().unwrap().to_string();

    // Unconfigured: share names the fix, team views are 404s.
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/sessions/{id}/share"),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    assert!(body_json(res).await["error"]
        .as_str()
        .unwrap()
        .contains("no workspace server configured"));
    for uri in ["/workspace/sessions", "/workspace/sessions/1"] {
        let res = app
            .clone()
            .oneshot(request("GET", uri, None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // Configured against a real workspace server: the whole team story
    // works over the daemon's HTTP surface.
    let (url, token, ws) = crate::workspace::tests::boot_server(&tmp.path().join("srv")).await;
    std::env::set_var(crate::workspace::SERVER_URL_ENV, &url);
    std::env::set_var(crate::workspace::SERVER_TOKEN_ENV, &token);
    std::env::set_var(crate::workspace::SERVER_WORKSPACE_ENV, ws.to_string());

    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/sessions/{id}/share"),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let shared = body_json(res).await;
    assert_eq!(shared["shared_by"], "casey");
    let sid = shared["id"].as_i64().unwrap();

    let res = app
        .clone()
        .oneshot(request("GET", "/workspace/sessions", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body_json(res).await["sessions"].as_array().unwrap().len(),
        1
    );

    let res = app
        .clone()
        .oneshot(request("GET", &format!("/workspace/sessions/{sid}"), None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let detail = body_json(res).await;
    assert_eq!(detail["session"]["title"], "http test");
    assert!(detail["markdown"].as_str().unwrap().contains("http test"));

    let pickup = serde_json::json!({
        "workspace_session_id": sid,
        "harness": "gemini-cli",
        "repo_root": repo,
        "command_override": crate::pty::CommandSpec::new("sh").arg("-c").arg("true"),
    });
    let res = app
        .clone()
        .oneshot(request("POST", "/sessions/pickup", Some(pickup.clone())))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let picked = body_json(res).await;
    assert_eq!(picked["harness"], "gemini-cli");
    assert_eq!(picked["title"], "pickup: http test");

    // Server gone: every proxied route degrades to 502 with the reason.
    std::env::set_var(crate::workspace::SERVER_URL_ENV, "http://127.0.0.1:1");
    let res = app
        .clone()
        .oneshot(request("POST", "/sessions/pickup", Some(pickup)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    for uri in ["/workspace/sessions", "/workspace/sessions/1"] {
        let res = app
            .clone()
            .oneshot(request("GET", uri, None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
        assert!(body_json(res).await["error"]
            .as_str()
            .unwrap()
            .contains("unreachable"));
    }

    for var in [
        crate::workspace::SERVER_URL_ENV,
        crate::workspace::SERVER_TOKEN_ENV,
        crate::workspace::SERVER_WORKSPACE_ENV,
    ] {
        std::env::remove_var(var);
    }
}

/// Holding the env lock across awaits is fine in tests (see above).
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn inbox_routes_run_the_triage_story_over_http() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for var in [
        crate::workspace::SERVER_URL_ENV,
        crate::workspace::SERVER_TOKEN_ENV,
        crate::workspace::SERVER_WORKSPACE_ENV,
    ] {
        std::env::remove_var(var);
    }
    let (_tmp, app, repo) = test_world();

    // No server configured → the embedded local inbox serves the routes.
    let res = app
        .clone()
        .oneshot(request("GET", "/config", None))
        .await
        .unwrap();
    assert_eq!(body_json(res).await["inbox_backend"], "local");

    // Ingest (bad → 400; good → counts), list, detail.
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/signals",
            Some(serde_json::json!({ "source": " ", "kind": "exception",
                "severity": "low", "title": "x" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            "/signals",
            Some(serde_json::json!({
                "source": "sentry", "kind": "exception", "severity": "critical",
                "title": "NPE", "affected_count": 5,
                "evidence": [{ "kind": "issue", "label": "gh issue",
                               "url": "https://gh/i/1" }],
            })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await["inserted"], 1);
    let res = app
        .clone()
        .oneshot(request("GET", "/inbox?status=new", None))
        .await
        .unwrap();
    let items = body_json(res).await;
    let iid = items["items"][0]["id"].as_i64().unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", &format!("/inbox/{iid}"), None))
        .await
        .unwrap();
    assert_eq!(body_json(res).await["signals"][0]["source"], "sentry");
    // Missing item → 404 through the shared error mapping.
    let res = app
        .clone()
        .oneshot(request("GET", "/inbox/999", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Start a triage session from the item over HTTP.
    let from = serde_json::json!({
        "item_id": iid,
        "harness": "gemini-cli",
        "repo_root": repo,
        "command_override": crate::pty::CommandSpec::new("sh").arg("-c").arg("true"),
    });
    let res = app
        .clone()
        .oneshot(request("POST", "/sessions/from-inbox", Some(from)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let meta = body_json(res).await;
    assert_eq!(meta["inbox_item_id"], iid);
    let sid = meta["id"].as_str().unwrap().to_string();

    // Accepting again is a 409 (someone already took it via the launch).
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/inbox/{iid}/accept"),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // Dismiss: bad reason → 400; a fresh item dismisses cleanly.
    app.clone()
        .oneshot(request(
            "POST",
            "/signals",
            Some(serde_json::json!({ "source": "ci", "kind": "regression",
                "severity": "low", "title": "noise" })),
        ))
        .await
        .unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", "/inbox?status=new", None))
        .await
        .unwrap();
    let noise = body_json(res).await["items"][0]["id"].as_i64().unwrap();
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/inbox/{noise}/dismiss"),
            Some(serde_json::json!({ "reason": "meh" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/inbox/{noise}/dismiss"),
            Some(serde_json::json!({ "reason": "duplicate" })),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // A fresh item accepts cleanly over HTTP (attributed locally).
    app.clone()
        .oneshot(request(
            "POST",
            "/signals",
            Some(serde_json::json!({ "source": "ci", "kind": "ticket",
                "severity": "low", "title": "take me" })),
        ))
        .await
        .unwrap();
    let res = app
        .clone()
        .oneshot(request("GET", "/inbox?status=new", None))
        .await
        .unwrap();
    let fresh = body_json(res).await["items"][0]["id"].as_i64().unwrap();
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/inbox/{fresh}/accept"),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body_json(res).await["status"], "accepted");

    // Outcome recording over HTTP: gh disabled → recorded:false with the
    // reason; a non-inbox session → 502-style workspace error.
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/sessions/{sid}/inbox-outcome"),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["recorded"], false);
    assert!(body["note"].as_str().unwrap().contains("gh CLI"));
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    let plain = create_test_session(&app, &repo, "true").await;
    let res = app
        .clone()
        .oneshot(request(
            "POST",
            &format!("/sessions/{}/inbox-outcome", plain["id"].as_str().unwrap()),
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);

    // Remote-down: point the daemon at a dead server — every inbox route
    // degrades to 502 with the reason.
    std::env::set_var(crate::workspace::SERVER_URL_ENV, "http://127.0.0.1:1");
    std::env::set_var(crate::workspace::SERVER_TOKEN_ENV, "oadk_x");
    std::env::set_var(crate::workspace::SERVER_WORKSPACE_ENV, "1");
    let res = app
        .clone()
        .oneshot(request("GET", "/inbox", None))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    for var in [
        crate::workspace::SERVER_URL_ENV,
        crate::workspace::SERVER_TOKEN_ENV,
        crate::workspace::SERVER_WORKSPACE_ENV,
    ] {
        std::env::remove_var(var);
    }
}
