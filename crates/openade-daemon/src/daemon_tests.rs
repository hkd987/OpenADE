use super::*;
use std::process::Command;
use tempfile::TempDir;

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
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(dir.join("README.md"), "hi\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "init"]);
}

fn setup() -> (TempDir, Daemon, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = Daemon::open(tmp.path().join("data")).unwrap();
    (tmp, daemon, repo)
}

fn sh(cmd: &str) -> CommandSpec {
    CommandSpec::new("sh").arg("-c").arg(cmd)
}

fn launch_req(repo: &std::path::Path, cmd: &str) -> LaunchSessionRequest {
    LaunchSessionRequest {
        title: "test task".into(),
        harness: Harness::ClaudeCode,
        repo_root: repo.to_path_buf(),
        checkout: CheckoutMode::default(),
        entity_ref: Some("component:default/demo".into()),
        prompt: Some("do the thing".into()),
        mcp_servers: vec![],
        command_override: Some(sh(cmd)),
    }
}

fn wait_state(daemon: &Daemon, id: Uuid, state: SessionState) -> SessionMeta {
    for _ in 0..200 {
        let meta = daemon.get(id).unwrap();
        if meta.state == state {
            return meta;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("session never reached {state:?}");
}

#[test]
fn launch_creates_worktree_runs_and_completes() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon
        .launch(launch_req(&repo, "printf session-ok; exit 0"), None)
        .unwrap();

    assert_eq!(meta.state, SessionState::Running);
    let wt = meta.worktree_path.clone().unwrap();
    assert!(wt.join("README.md").is_file());
    assert!(meta.branch.as_deref().unwrap().starts_with("openade/"));

    let done = wait_state(&daemon, meta.id, SessionState::Completed);
    assert!(done.state.is_terminal());
    assert!(daemon.scrollback(meta.id).unwrap().contains("session-ok"));

    // Transcript captured prompt + state change and indexed the entity.
    let events = daemon.store().read_transcript(meta.id).unwrap();
    assert!(events.iter().any(|e| e.kind == EventKind::Prompt));
    let by_entity = daemon
        .store()
        .sessions_for_entity("component:default/demo")
        .unwrap();
    assert_eq!(by_entity.len(), 1);
}

#[test]
fn failing_command_marks_session_failed() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "exit 3"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Failed);
}

#[test]
fn parallel_sessions_get_isolated_worktrees() {
    let (_tmp, daemon, repo) = setup();
    let metas: Vec<SessionMeta> = (0..4)
        .map(|_| daemon.launch(launch_req(&repo, "sleep 20"), None).unwrap())
        .collect();
    let paths: std::collections::HashSet<_> = metas
        .iter()
        .map(|m| m.worktree_path.clone().unwrap())
        .collect();
    assert_eq!(paths.len(), 4);
    assert_eq!(daemon.list().len(), 4);
    for m in &metas {
        daemon.kill(m.id).unwrap();
    }
}

#[test]
fn main_checkout_sessions_run_in_the_repo_without_a_worktree() {
    let (tmp, daemon, repo) = setup();
    let mut req = launch_req(&repo, "true");
    req.checkout = CheckoutMode::Main;
    let meta = daemon.launch(req, None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    // The working directory IS the repository, on its current branch.
    assert_eq!(meta.worktree_path.as_deref(), Some(repo.as_path()));
    assert_eq!(meta.branch.as_deref(), Some("main"));
    assert!(meta.base_commit.is_some());
    // No worktree directory was created.
    assert!(!tmp.path().join("data/worktrees/repo").exists());

    // Diff and files work against the main checkout.
    std::fs::write(repo.join("README.md"), "hi\nmain checkout edit\n").unwrap();
    assert!(daemon
        .diff(meta.id)
        .unwrap()
        .contains("+main checkout edit"));
    assert!(daemon
        .files(meta.id)
        .unwrap()
        .contains(&"README.md".to_string()));

    // Cleanup must refuse: the "worktree" is the user's repository.
    let err = daemon.cleanup_worktree(meta.id, true).unwrap_err();
    assert!(matches!(err, DaemonError::MainCheckout));
    assert!(repo.exists());
    std::fs::write(repo.join("README.md"), "hi\n").unwrap();
}

#[test]
fn workdir_files_are_readable_but_never_outside_the_worktree() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    let content = daemon.read_workdir_file(meta.id, "README.md").unwrap();
    assert_eq!(content, "hi\n");
    // Leading slash is tolerated (treated as worktree-relative).
    assert!(daemon.read_workdir_file(meta.id, "/README.md").is_ok());

    // Missing file and path traversal are both NotFound.
    assert!(daemon.read_workdir_file(meta.id, "nope.md").is_err());
    assert!(daemon
        .read_workdir_file(meta.id, "../../../etc/passwd")
        .is_err());
    // A canonicalizable escape and a directory are rejected on the same
    // guard: outside the worktree, or not a regular file.
    let wt = meta.worktree_path.clone().unwrap();
    std::fs::write(wt.parent().unwrap().join("escape.txt"), "secret").unwrap();
    assert!(daemon.read_workdir_file(meta.id, "../escape.txt").is_err());
    assert!(daemon.read_workdir_file(meta.id, ".openade").is_err());
}

#[test]
fn skills_are_discovered_from_both_conventions() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    let wt = meta.worktree_path.clone().unwrap();

    // No skill folders → empty, not an error.
    assert!(daemon.skills(meta.id).unwrap().is_empty());

    std::fs::create_dir_all(wt.join(".claude/skills/release")).unwrap();
    std::fs::write(
        wt.join(".claude/skills/release/SKILL.md"),
        "---\nname: release\n---\n# Release\n\nCut and publish a release.\n",
    )
    .unwrap();
    std::fs::create_dir_all(wt.join(".openade/skills")).unwrap();
    std::fs::write(
        wt.join(".openade/skills/migrate.md"),
        "# Migrate\nRun the schema migration playbook.\n",
    )
    .unwrap();
    // A skill dir without SKILL.md is skipped.
    std::fs::create_dir_all(wt.join(".claude/skills/empty")).unwrap();

    let skills = daemon.skills(meta.id).unwrap();
    assert_eq!(skills.len(), 2);
    assert_eq!(skills[0]["name"], "migrate");
    assert_eq!(
        skills[0]["description"],
        "Run the schema migration playbook."
    );
    assert_eq!(skills[1]["name"], "release");
    assert_eq!(skills[1]["description"], "Cut and publish a release.");
}

#[test]
fn pull_requests_report_via_gh_and_degrade_without_it() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();

    // A gh shim that serves `pr list`.
    let shim = tmp.path().join("gh");
    std::fs::write(
        &shim,
        "#!/bin/sh\ncase \"$*\" in\n  \"pr list\"*) printf '[{\"number\":7,\"title\":\"Add retries\",\"url\":\"https://github.com/acme/x/pull/7\",\"headRefName\":\"retries\",\"isDraft\":false}]' ;;\n  *) echo 'gh: Not Found (HTTP 404)' >&2; exit 1 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::env::set_var("OPENADE_GH_BIN", &shim);
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    let result = Daemon::pull_requests(tmp.path());
    assert_eq!(result["prs"][0]["number"], 7);
    assert_eq!(result["prs"][0]["title"], "Add retries");

    // gh failure → empty list with the reason.
    let failing = tmp.path().join("gh-fail");
    std::fs::write(&failing, "#!/bin/sh\necho 'no remote' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::env::set_var("OPENADE_GH_BIN", &failing);
    let result = Daemon::pull_requests(tmp.path());
    assert_eq!(result["prs"].as_array().unwrap().len(), 0);
    assert_eq!(result["note"], "no remote");

    // Missing binary → spawn error note; disabled gh → note.
    std::env::set_var("OPENADE_GH_BIN", tmp.path().join("nope"));
    let result = Daemon::pull_requests(tmp.path());
    assert!(result["note"].as_str().unwrap().contains("No such file"));
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    let result = Daemon::pull_requests(tmp.path());
    assert_eq!(result["note"], "gh CLI not available");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    std::env::remove_var("OPENADE_GH_BIN");
}

#[test]
fn cleanup_respects_dirty_guard() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    let wt = meta.worktree_path.clone().unwrap();
    std::fs::write(wt.join("dirty.txt"), "uncommitted").unwrap();
    assert!(matches!(
        daemon.cleanup_worktree(meta.id, false),
        Err(DaemonError::Worktree(WorktreeError::Dirty(_)))
    ));
    daemon.cleanup_worktree(meta.id, true).unwrap();
    assert!(!wt.exists());
}

#[test]
fn diff_files_and_projects_are_exposed() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    let wt = meta.worktree_path.clone().unwrap();
    std::fs::write(wt.join("README.md"), "hi\nagent was here\n").unwrap();

    let diff = daemon.diff(meta.id).unwrap();
    assert!(diff.contains("+agent was here"), "{diff}");

    let files = daemon.files(meta.id).unwrap();
    assert!(files.contains(&"README.md".to_string()));

    assert_eq!(daemon.projects().unwrap(), vec![repo]);
}

#[test]
fn rules_are_materialized_into_the_worktree() {
    let (_tmp, daemon, repo) = setup();
    // Give the repo a canonical rules file and commit it so worktrees see it.
    openade_core::rules::init_canonical_rules(&repo, "always be testing\n").unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run(&["add", "."]);
    run(&["commit", "-m", "rules"]);

    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    let wt = meta.worktree_path.unwrap();
    let claude_md = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("always be testing"));
}

#[tokio::test]
async fn handoff_carries_worktree_and_context_to_the_new_harness() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);

    // Entity-launched Claude session that did some work.
    let mut req = launch_req(&repo, "sleep 30");
    req.entity_ref = Some("component:default/payments-api".into());
    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await;
    let old = daemon.launch(req, bundle).unwrap();
    let wt = old.worktree_path.clone().unwrap();
    std::fs::write(wt.join("README.md"), "hi\nwork in progress\n").unwrap();

    // Hand off to Gemini; the override prints the handoff doc so we can
    // verify the new session actually sees it.
    let new = daemon
        .handoff(
            old.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: Some("Finish the remaining edge cases.".into()),
                command_override: Some(sh("cat .openade/handoff.md")),
            },
        )
        .unwrap();

    // Same task, same working state, new harness.
    assert_eq!(new.harness, Harness::GeminiCli);
    assert_eq!(new.worktree_path.as_ref(), Some(&wt));
    assert_eq!(new.branch, old.branch);
    assert_eq!(new.entity_ref, old.entity_ref);
    assert_ne!(new.id, old.id);

    // Old session ended; registry shows both.
    assert!(daemon.get(old.id).unwrap().state.is_terminal());
    assert_eq!(daemon.list().len(), 2);

    // Handoff doc captures the work; entity context re-attached for the
    // new harness's rules file.
    let handoff = std::fs::read_to_string(wt.join(".openade/handoff.md")).unwrap();
    assert!(handoff.contains("README.md"), "{handoff}");
    let gemini_rules = std::fs::read_to_string(wt.join("GEMINI.md")).unwrap();
    assert!(gemini_rules.contains("Payments API"));

    // The new session's PTY really read the handoff doc.
    wait_state(&daemon, new.id, SessionState::Completed);
    assert!(daemon.scrollback(new.id).unwrap().contains("# Session:"));

    // Both transcripts record the handoff.
    let new_events = daemon.store().read_transcript(new.id).unwrap();
    assert!(new_events
        .iter()
        .any(|e| e.payload.get("handoff_from").is_some()));
    let old_events = daemon.store().read_transcript(old.id).unwrap();
    assert!(old_events
        .iter()
        .any(|e| e.payload.get("handoff_to").is_some()));
}

#[test]
fn handoff_without_worktree_is_rejected() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    // Simulate a lost worktree.
    let wt = meta.worktree_path.clone().unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    daemon.cleanup_worktree(meta.id, true).unwrap();
    assert!(!wt.exists());
    let err = daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::CodexCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap_err();
    assert!(matches!(err, DaemonError::Handoff(_, _)));
}

#[test]
fn sessions_waiting_on_a_prompt_surface_needs_input() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon
        .launch(
            launch_req(&repo, "printf 'Continue? (y/n) '; read x; exit 0"),
            None,
        )
        .unwrap();

    // After the prompt prints and output quiesces, state flips.
    wait_state(&daemon, meta.id, SessionState::NeedsInput);

    // Answering resumes and completes the session.
    daemon.write_input(meta.id, b"y\n").unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
}

#[test]
fn publish_artifact_commits_to_a_review_branch_without_touching_checkout() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    // Session did some work in its worktree.
    let wt = meta.worktree_path.clone().unwrap();
    std::fs::write(wt.join("README.md"), "hi\nnew feature\n").unwrap();

    let head_before = Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;

    let info = daemon.publish_artifact(meta.id).unwrap();
    assert!(info.branch.starts_with("openade/knowledge-"));
    assert!(info.summary.contains("README.md"), "{}", info.summary);
    assert!(info.markdown.contains("# Session: test task"));

    // The artifact is committed on the review branch...
    let show = Command::new("git")
        .args([
            "-C",
            repo.to_str().unwrap(),
            "show",
            &format!("{}:{}", info.branch, info.file.display()),
        ])
        .output()
        .unwrap();
    assert!(show.status.success());
    let committed = String::from_utf8_lossy(&show.stdout);
    assert!(committed.contains("README.md"));

    // The living-docs index is committed alongside, referencing the artifact.
    let show_index = Command::new("git")
        .args([
            "-C",
            repo.to_str().unwrap(),
            "show",
            &format!("{}:docs/openade/sessions/index.md", info.branch),
        ])
        .output()
        .unwrap();
    assert!(show_index.status.success());
    let index = String::from_utf8_lossy(&show_index.stdout);
    assert!(index.contains("# Session knowledge index"));
    assert!(index.contains("test task"), "{index}");

    // ...while the user's checkout is untouched (HEAD and status).
    let head_after = Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(head_before, head_after);
    let status = Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());

    // The outcome is on the record for future context bundles.
    let events = daemon.store().read_transcript(meta.id).unwrap();
    let outcome = events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Outcome)
        .unwrap();
    assert_eq!(outcome.payload["artifact_branch"], info.branch);
}

#[test]
fn publish_artifact_also_pushes_to_the_shared_memory_repo() {
    let (tmp, daemon, repo) = setup();
    let shared = crate::memory_repo::tests::repo_with_shim(tmp.path());
    let daemon = daemon.with_memory_repo(shared);

    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    std::fs::write(
        meta.worktree_path.clone().unwrap().join("README.md"),
        "hi\nnew feature\n",
    )
    .unwrap();

    let info = daemon.publish_artifact(meta.id).unwrap();
    assert_eq!(info.shared_repo.as_deref(), Some("acme/team-memory"));
    let shared_path = info.shared_path.clone().unwrap();
    assert!(shared_path.starts_with("sessions/"), "{shared_path}");

    // The document and the index landed on the shared repo's default branch
    // (the shim's state dir), with the entity ref threaded into the index.
    let doc = crate::memory_repo::tests::state_file(tmp.path(), &shared_path);
    assert!(doc.contains("# Session: test task"), "{doc}");
    let index = crate::memory_repo::tests::state_file(tmp.path(), "index.md");
    assert!(index.contains(&format!("]({shared_path})")), "{index}");
    assert!(index.contains("`component:default/demo`"), "{index}");

    // The outcome records where the shared copy went.
    let events = daemon.store().read_transcript(meta.id).unwrap();
    let outcome = events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Outcome)
        .unwrap();
    assert_eq!(outcome.payload["shared_repo"], "acme/team-memory");
}

#[test]
fn shared_memory_push_failure_degrades_to_local_only() {
    let (tmp, daemon, repo) = setup();
    // A memory repo whose gh binary doesn't exist: pushes fail, publication
    // still succeeds locally.
    let broken =
        crate::memory_repo::MemoryRepo::new(tmp.path().join("no-such-gh"), "acme/team-memory");
    let daemon = daemon.with_memory_repo(broken);

    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    let info = daemon.publish_artifact(meta.id).unwrap();
    assert!(info.branch.starts_with("openade/knowledge-"));
    assert!(info.shared_repo.is_none());
    assert!(info.shared_path.is_none());
}

#[test]
fn sessions_auto_ground_in_the_repo_origin_remote() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");

    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };

    // No origin remote → nothing to infer.
    assert!(daemon.infer_entity_ref(&repo).is_none());

    // GitHub origin → the repo grounds itself.
    git(&[
        "remote",
        "add",
        "origin",
        "git@github.com:acme/payments-service.git",
    ]);
    assert_eq!(
        daemon.infer_entity_ref(&repo).as_deref(),
        Some("repo:acme/payments-service")
    );

    // Explicitly disabled GitHub memory → no inference.
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    assert!(daemon.infer_entity_ref(&repo).is_none());
    std::env::remove_var("OPENADE_GITHUB_MEMORY");

    // Non-GitHub origin → no repo: memory to offer.
    git(&[
        "remote",
        "set-url",
        "origin",
        "https://gitlab.com/acme/x.git",
    ]);
    assert!(daemon.infer_entity_ref(&repo).is_none());

    // Not a git repository → degrade quietly.
    let plain_dir = tmp.path().join("not-a-repo");
    std::fs::create_dir(&plain_dir).unwrap();
    assert!(daemon.infer_entity_ref(&plain_dir).is_none());

    // No catalog → no context to ground in.
    let plain = Daemon::open(tmp.path().join("data2")).unwrap();
    assert!(plain.infer_entity_ref(&repo).is_none());

    std::env::remove_var("OPENADE_GH_BIN");
}

#[test]
fn configure_wires_memory_from_settings_and_env() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for var in [
        "BACKSTAGE_BASE_URL",
        "BACKSTAGE_TOKEN",
        "OPENADE_MEMORY_REPO",
        "OPENADE_GH_BIN",
    ] {
        std::env::remove_var(var);
    }
    // Kill switch: everything gh-backed is off for the first sections.
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");

    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let daemon = Daemon::open(&data).unwrap();

    // Nothing configured anywhere → no memory, still a working daemon.
    daemon.configure(&crate::config::Settings::default());
    assert!(!daemon.has_catalog());
    assert!(daemon.memory_sources().is_empty());

    // Stored settings (as onboarding saves them): Backstage becomes a
    // source; the shared memory repo needs a gh CLI, which is disabled, so
    // it is dropped with a warning.
    let settings = crate::config::Settings {
        backstage_base_url: Some("http://127.0.0.1:9/api".into()),
        backstage_token: Some("token".into()),
        memory_repo: Some("acme/team-memory".into()),
        onboarded: true,
        server_url: None,
        server_token: None,
        server_workspace: None,
    };
    daemon.configure(&settings);
    assert!(daemon.has_catalog());
    assert_eq!(daemon.memory_sources(), vec!["backstage"]);
    assert_eq!(daemon.settings(), settings);

    // With gh back (explicit binary) the github source joins (specific
    // kinds first).
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    daemon.configure(&settings);
    assert_eq!(daemon.memory_sources(), vec!["github", "backstage"]);

    // apply_settings persists and re-wires in one step.
    daemon.apply_settings(settings.clone()).unwrap();
    assert!(data.join("config.json").exists());
    assert_eq!(crate::config::Settings::load(&data), settings);

    std::env::remove_var("OPENADE_GH_BIN");
}

#[test]
fn committed_memory_repo_file_configures_shared_memory_per_repo() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (tmp, daemon, repo) = setup();
    // The daemon has NO daemon-wide memory repo; the repository commits its
    // own via .openade/memory-repo, resolved through the gh on
    // OPENADE_GH_BIN (the stateful contents shim).
    let _shared = crate::memory_repo::tests::repo_with_shim(tmp.path());
    std::env::set_var("OPENADE_GH_BIN", tmp.path().join("gh"));
    std::fs::create_dir_all(repo.join(".openade")).unwrap();
    std::fs::write(repo.join(".openade/memory-repo"), "acme/team-memory\n").unwrap();

    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    let info = daemon.publish_artifact(meta.id).unwrap();
    assert_eq!(info.shared_repo.as_deref(), Some("acme/team-memory"));
    let doc = crate::memory_repo::tests::state_file(tmp.path(), &info.shared_path.unwrap());
    assert!(doc.contains("# Session: test task"), "{doc}");

    std::env::remove_var("OPENADE_GH_BIN");
}

#[tokio::test]
async fn shared_memory_index_entries_enrich_the_bundle() {
    let tmp = TempDir::new().unwrap();
    let shared = crate::memory_repo::tests::repo_with_shim(tmp.path());
    shared
        .put_file(
            "index.md",
            "# Session knowledge index\n\n\
             - [Newest](sessions/d.md) — most recent *(2026-08-11, claude-code, `component:default/payments-api`)*\n\
             - [Fix retries](sessions/a.md) — hardened retry loop *(2026-08-10, claude-code, `component:default/payments-api`)*\n\
             - [Other entity](sessions/b.md) — unrelated *(2026-08-09, codex-cli, `repo:acme/ledger`)*\n\
             - [No entity](sessions/c.md) — untagged *(2026-08-08, gemini-cli)*\n\
             - [Oldest](sessions/e.md) — old lesson *(2026-08-01, codex-cli, `component:default/payments-api`)*\n",
            "memory: seed",
        )
        .unwrap();
    let daemon = daemon_with_catalog(&tmp).with_memory_repo(shared);

    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await
        .unwrap();

    // The shared repo is linked as documentation...
    assert!(bundle
        .docs
        .iter()
        .any(|d| d.url == "https://github.com/acme/team-memory"
            && d.title.contains("acme/team-memory")));

    // ...and entity-matching index lines (capped at 3) become prior-session
    // context, tagged as team memory.
    let shared_priors: Vec<&PriorSessionSummary> = bundle
        .prior_sessions
        .iter()
        .filter(|p| p.session_id == "shared-memory")
        .collect();
    assert_eq!(shared_priors.len(), 3, "{:?}", bundle.prior_sessions);
    assert!(shared_priors[0].summary.contains("[Newest]"));
    assert!(shared_priors[2].summary.contains("[Oldest]"));
    assert!(!bundle
        .prior_sessions
        .iter()
        .any(|p| p.summary.contains("Other entity") || p.summary.contains("No entity")));
}

#[tokio::test]
async fn unreadable_shared_index_still_links_the_memory_repo() {
    let tmp = TempDir::new().unwrap();
    // Shim exists but has no index.md yet — reads return None.
    let shared = crate::memory_repo::tests::repo_with_shim(tmp.path());
    let daemon = daemon_with_catalog(&tmp).with_memory_repo(shared);

    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await
        .unwrap();
    assert!(bundle
        .docs
        .iter()
        .any(|d| d.url == "https://github.com/acme/team-memory"));
    assert!(!bundle
        .prior_sessions
        .iter()
        .any(|p| p.session_id == "shared-memory"));
}

fn daemon_with_catalog(tmp: &TempDir) -> Daemon {
    Daemon::open(tmp.path().join("data"))
        .unwrap()
        .with_catalog(Arc::new(
            catalog_mcp::testutil::MockProvider::with_payments_graph(),
        ))
}

#[tokio::test]
async fn entity_launch_injects_context_bundle() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);
    assert!(daemon.has_catalog());

    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("component:default/payments-api".into());
    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await;
    assert!(bundle.is_some());

    let meta = daemon.launch(req, bundle).unwrap();
    let wt = meta.worktree_path.clone().unwrap();

    // Injected where the harness reads instructions...
    let rules = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
    assert!(rules.contains("Payments API"), "{rules}");
    assert!(rules.contains("Payments Team"));

    // ...and machine-readable under .openade/.
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(wt.join(".openade/context.json")).unwrap())
            .unwrap();
    assert_eq!(
        json["entity"]["entity_ref"],
        "component:default/payments-api"
    );
    assert!(wt.join(".openade/context.md").is_file());

    // The catalog MCP server was auto-registered project-scoped.
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
    assert!(mcp["mcpServers"]["catalog"].is_object());

    // And the transcript recorded the injected context.
    let events = daemon.store().read_transcript(meta.id).unwrap();
    assert!(events.iter().any(|e| e.kind == EventKind::Context));
}

#[tokio::test]
async fn repo_entity_launch_injects_github_memory_context() {
    // A `repo:owner/name` ref (the GitHub memory source) flows through the
    // same bundle pipeline as catalog entities.
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);

    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("repo:acme/payments-service".into());
    let bundle = daemon
        .build_bundle("repo:acme/payments-service", None)
        .await;
    assert!(bundle.is_some());

    let meta = daemon.launch(req, bundle).unwrap();
    let wt = meta.worktree_path.clone().unwrap();

    let rules = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
    assert!(rules.contains("acme/payments-service"), "{rules}");
    assert!(rules.contains("Payments service repository on GitHub."));
    // CODEOWNERS-derived team ownership carried into the bundle (the group
    // entity is unresolvable in the graph, so the bare ref is kept).
    assert!(rules.contains("group:acme/payments-team"), "{rules}");

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(wt.join(".openade/context.json")).unwrap())
            .unwrap();
    assert_eq!(json["entity"]["entity_ref"], "repo:acme/payments-service");
    assert_eq!(json["entity"]["kind"], "repo");
}

#[tokio::test]
async fn prior_session_outcomes_feed_the_next_bundle() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);

    // First session on the entity, with a recorded outcome.
    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("component:default/payments-api".into());
    let first = daemon.launch(req.clone(), None).unwrap();
    daemon
        .store()
        .record(
            first.id,
            EventKind::Outcome,
            serde_json::json!({ "summary": "Added idempotency keys to POST /charges." }),
        )
        .unwrap();

    // The next bundle for the same entity carries that knowledge.
    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await
        .unwrap();
    assert_eq!(bundle.prior_sessions.len(), 1);
    assert!(bundle.prior_sessions[0]
        .summary
        .contains("idempotency keys"));
    assert!(bundle.to_markdown().contains("idempotency keys"));
}

#[tokio::test]
async fn unresolvable_entities_degrade_to_no_bundle() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let daemon = daemon_with_catalog(&tmp);

    assert!(daemon
        .build_bundle("component:default/ghost", None)
        .await
        .is_none());
    assert!(daemon.build_bundle("not-a-ref", None).await.is_none());

    // No catalog configured at all → also None, and launches still work.
    let plain = Daemon::open(tmp.path().join("data2")).unwrap();
    assert!(plain
        .build_bundle("component:default/payments-api", None)
        .await
        .is_none());
    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("component:default/ghost".into());
    let meta = daemon.launch(req, None).unwrap();
    assert!(meta.worktree_path.is_some());
}

#[test]
fn user_scoped_registrations_never_touch_the_worktree() {
    let _guard = trace_guard();
    let (_tmp, daemon, repo) = setup();
    let mut req = launch_req(&repo, "true");
    req.harness = Harness::CodexCli;
    req.mcp_servers = vec![McpServerSpec {
        name: "catalog".into(),
        transport: crate::adapters::McpTransport::Stdio {
            command: "catalog-mcp".into(),
            args: vec![],
        },
    }];
    let meta = daemon.launch(req, None).unwrap();
    let wt = meta.worktree_path.unwrap();
    // Codex registration is user-scoped (~/.codex/config.toml): nothing
    // like a literal "~" directory may appear in the worktree.
    assert!(!wt.join("~").exists());
    assert!(!wt.join(".mcp.json").exists());
}

#[test]
fn mcp_servers_are_registered_project_scoped() {
    let (_tmp, daemon, repo) = setup();
    let mut req = launch_req(&repo, "true");
    req.mcp_servers = vec![McpServerSpec {
        name: "catalog".into(),
        transport: crate::adapters::McpTransport::Stdio {
            command: "catalog-mcp".into(),
            args: vec![],
        },
    }];
    let meta = daemon.launch(req, None).unwrap();
    let wt = meta.worktree_path.unwrap();
    let mcp_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
    assert!(mcp_json["mcpServers"]["catalog"].is_object());
}

fn trace_guard() -> tracing::subscriber::DefaultGuard {
    tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(std::io::sink)
            .finish(),
    )
}

#[test]
fn guards_for_sessions_without_worktrees() {
    let (_tmp, daemon, repo) = setup();
    let meta = SessionMeta::new("no worktree", Harness::ClaudeCode, &repo);
    let id = meta.id;
    daemon.insert_test_session(meta);

    // Every worktree-dependent operation degrades or errors cleanly.
    assert_eq!(daemon.diff(id).unwrap(), "");
    assert!(daemon.files(id).unwrap().is_empty());
    daemon.cleanup_worktree(id, false).unwrap();
    let err = daemon
        .handoff(
            id,
            HandoffRequest {
                harness: Harness::CodexCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap_err();
    assert!(matches!(err, DaemonError::Handoff(_, _)));

    // Unknown ids are NotFound.
    assert!(matches!(
        daemon.kill(Uuid::new_v4()),
        Err(DaemonError::NotFound(_))
    ));
}

#[test]
fn transcript_write_failures_never_kill_the_session_lifecycle() {
    let _guard = trace_guard();
    let (_tmp, daemon, repo) = setup();
    // Fault injection: event indexing is broken for the whole lifecycle.
    daemon.store().raw_sql("DROP TABLE events");

    let meta = daemon.launch(launch_req(&repo, "sleep 20"), None).unwrap();
    assert_eq!(meta.state, SessionState::Running);

    let new = daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap();
    wait_state(&daemon, new.id, SessionState::Completed);

    // Artifact publication still works; the commit is the deliverable.
    let info = daemon.publish_artifact(new.id).unwrap();
    assert!(info.branch.starts_with("openade/knowledge-"));
}

#[tokio::test]
async fn prior_summary_index_failure_degrades_to_no_history() {
    let _guard = trace_guard();
    let tmp = TempDir::new().unwrap();
    let daemon = daemon_with_catalog(&tmp);
    daemon.store().raw_sql("DROP TABLE sessions");
    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await
        .unwrap();
    assert!(bundle.prior_sessions.is_empty());
}

#[test]
fn unreadable_rules_targets_warn_but_do_not_block() {
    let _guard = trace_guard();
    let (_tmp, daemon, repo) = setup();
    openade_core::rules::init_canonical_rules(&repo, "rule\n").unwrap();
    // A CLAUDE.md *directory* makes rules materialization fail with an Io
    // error (not MissingCanonical) — the launch must still proceed.
    std::fs::create_dir(repo.join("CLAUDE.md")).unwrap();
    std::fs::write(repo.join("CLAUDE.md/inner.txt"), "x").unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run(&["add", "."]);
    run(&["commit", "-m", "weird rules target"]);

    let meta = daemon.launch(launch_req(&repo, "sleep 20"), None).unwrap();
    assert_eq!(meta.state, SessionState::Running);

    // Same on the handoff path (GEMINI.md as a directory in the worktree).
    let wt = meta.worktree_path.clone().unwrap();
    std::fs::create_dir(wt.join("GEMINI.md")).unwrap();
    std::fs::write(wt.join("GEMINI.md/inner.txt"), "x").unwrap();
    let new = daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap();
    assert_eq!(new.harness, Harness::GeminiCli);
}

#[tokio::test]
async fn existing_project_files_are_respected_at_launch() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    // Hand-written CLAUDE.md without a trailing newline + a committed
    // .mcp.json: neither may be clobbered.
    std::fs::write(repo.join("CLAUDE.md"), "my hand-written rules").unwrap();
    std::fs::write(repo.join(".mcp.json"), "{\"mcpServers\":{}}").unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run(&["add", "."]);
    run(&["commit", "-m", "project files"]);

    let daemon = daemon_with_catalog(&tmp);
    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("component:default/payments-api".into());
    let bundle = daemon
        .build_bundle("component:default/payments-api", None)
        .await;
    let meta = daemon.launch(req, bundle).unwrap();
    let wt = meta.worktree_path.unwrap();

    // Bundle appended after a separating newline; original text intact.
    let rules = std::fs::read_to_string(wt.join("CLAUDE.md")).unwrap();
    assert!(rules.starts_with("my hand-written rules\n"));
    assert!(rules.contains("# System context: Payments API"));
    // Pre-existing MCP config untouched.
    assert_eq!(
        std::fs::read_to_string(wt.join(".mcp.json")).unwrap(),
        "{\"mcpServers\":{}}"
    );
}

#[test]
fn adapter_commands_are_used_when_no_override_is_given() {
    let (tmp, daemon, repo) = setup();
    // Shims for the harness CLIs on PATH (prepended; git etc. still resolve).
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    for name in ["claude", "gemini"] {
        let shim = bin.join(name);
        std::fs::write(
            &shim,
            format!("#!/bin/sh\necho {name}-shim \"$@\"\nsleep 20\n"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old_path = std::env::var("PATH").unwrap();
    std::env::set_var("PATH", format!("{}:{old_path}", bin.display()));

    let mut req = launch_req(&repo, "unused");
    req.command_override = None;
    let meta = daemon.launch(req, None).unwrap();
    for _ in 0..100 {
        if daemon.scrollback(meta.id).unwrap().contains("claude-shim") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let out = daemon.scrollback(meta.id).unwrap();
    assert!(out.contains("claude-shim"), "{out}");
    assert!(out.contains("do the thing"), "prompt passed through: {out}");

    // Handoff without an override uses the new harness's adapter command.
    let new = daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: None,
            },
        )
        .unwrap();
    for _ in 0..100 {
        if daemon.scrollback(new.id).unwrap().contains("gemini-shim") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let out = daemon.scrollback(new.id).unwrap();
    assert!(out.contains("gemini-shim -i"), "{out}");

    std::env::set_var("PATH", old_path);
    let _ = daemon.kill(new.id);
}

#[test]
fn handoff_reattaches_context_after_a_trailing_newline_is_added() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "sleep 20"), None).unwrap();
    let wt = meta.worktree_path.clone().unwrap();
    // Simulate an entity session's context plus a rules file that lacks a
    // trailing newline.
    std::fs::create_dir_all(wt.join(".openade")).unwrap();
    std::fs::write(wt.join(".openade/context.md"), "# System context: X\n").unwrap();
    std::fs::write(wt.join("GEMINI.md"), "no trailing newline").unwrap();

    daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap();
    let rules = std::fs::read_to_string(wt.join("GEMINI.md")).unwrap();
    assert!(rules.starts_with("no trailing newline\n"));
    assert!(rules.contains("# System context: X"));
}

#[test]
fn colliding_worktree_files_fail_context_and_registration_writes() {
    let _guard = trace_guard();
    // A committed `.openade/context.json` *directory* blocks the bundle write.
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    std::fs::create_dir_all(repo.join(".openade/context.json")).unwrap();
    std::fs::write(repo.join(".openade/context.json/placeholder"), "x").unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run(&["add", "."]);
    run(&["commit", "-m", "collide"]);

    let daemon = daemon_with_catalog(&tmp);
    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("component:default/payments-api".into());
    let bundle = futures_block(daemon.build_bundle("component:default/payments-api", None));
    assert!(matches!(
        daemon.launch(req, bundle),
        Err(DaemonError::Io(_))
    ));

    // A committed `.gemini` *file* blocks the settings-dir creation for
    // Gemini's project-scoped MCP registration.
    let repo2 = tmp.path().join("repo2");
    std::fs::create_dir(&repo2).unwrap();
    init_repo(&repo2);
    std::fs::write(repo2.join(".gemini"), "a file, not a dir").unwrap();
    let run2 = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&repo2)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run2(&["add", "."]);
    run2(&["commit", "-m", "collide"]);
    let mut req = launch_req(&repo2, "true");
    req.harness = Harness::GeminiCli;
    req.mcp_servers = vec![McpServerSpec {
        name: "catalog".into(),
        transport: crate::adapters::McpTransport::Stdio {
            command: "catalog-mcp".into(),
            args: vec![],
        },
    }];
    assert!(matches!(daemon.launch(req, None), Err(DaemonError::Io(_))));
}

/// Tiny block_on for the one sync test needing an async bundle.
fn futures_block<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut)
}

#[test]
fn state_index_failure_during_transition_is_non_fatal() {
    let _guard = trace_guard();
    let (_tmp, daemon, repo) = setup();
    let meta = daemon
        .launch(
            launch_req(&repo, "printf 'Continue? (y/n) '; read x; exit 0"),
            None,
        )
        .unwrap();
    // Let the prompt print, then break the index before the state flip.
    for _ in 0..200 {
        if daemon.scrollback(meta.id).unwrap().contains("(y/n)") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    daemon
        .store()
        .raw_sql("DROP TABLE events; DROP TABLE sessions;");
    // The in-memory state still transitions even though indexing fails.
    wait_state(&daemon, meta.id, SessionState::NeedsInput);
    daemon.write_input(meta.id, b"y\n").unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
}

#[test]
fn running_sessions_without_a_pty_keep_their_state() {
    let (_tmp, daemon, repo) = setup();
    let mut meta = SessionMeta::new("ghost", Harness::ClaudeCode, &repo);
    meta.set_state(SessionState::Running);
    let id = meta.id;
    daemon.insert_test_session(meta);
    // No PTY exists for this id: get() must not panic or change state.
    assert_eq!(daemon.get(id).unwrap().state, SessionState::Running);
}

#[test]
fn handoff_of_a_finished_session_and_repeat_handoffs() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);

    // Old session already terminal: no state rewrite, handoff still works.
    let wt = meta.worktree_path.clone().unwrap();
    std::fs::create_dir_all(wt.join(".openade")).unwrap();
    std::fs::write(wt.join(".openade/context.md"), "# System context: X\n").unwrap();
    let second = daemon
        .handoff(
            meta.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: Some(sh("sleep 20")),
            },
        )
        .unwrap();
    let rules = std::fs::read_to_string(wt.join("GEMINI.md")).unwrap();
    assert!(rules.contains("# System context: X"));

    // A repeat handoff to the same harness: context is already in GEMINI.md
    // and must not be duplicated.
    let third = daemon
        .handoff(
            second.id,
            HandoffRequest {
                harness: Harness::GeminiCli,
                prompt: None,
                command_override: Some(sh("true")),
            },
        )
        .unwrap();
    let rules = std::fs::read_to_string(wt.join("GEMINI.md")).unwrap();
    assert_eq!(rules.matches("# System context: X").count(), 1);
    let _ = daemon.kill(third.id);
}

#[test]
fn publishing_the_same_artifact_twice_fails_cleanly() {
    let (_tmp, daemon, repo) = setup();
    let meta = daemon.launch(launch_req(&repo, "true"), None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    let wt = meta.worktree_path.clone().unwrap();
    std::fs::write(wt.join("README.md"), "hi\nchange\n").unwrap();

    daemon.publish_artifact(meta.id).unwrap();
    // Same session → same review branch → git refuses the second commit.
    assert!(matches!(
        daemon.publish_artifact(meta.id),
        Err(DaemonError::Worktree(_))
    ));
}

// Env mutex across awaits: single-threaded test runtime, env spans the test.
#[allow(clippy::await_holding_lock)]
#[tokio::test]
async fn share_and_pickup_round_trip_through_a_real_workspace_server() {
    let _guard = catalog_mcp::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let (url, token, ws) = crate::workspace::tests::boot_server(&tmp.path().join("srv")).await;
    std::env::set_var(crate::workspace::SERVER_URL_ENV, &url);
    std::env::set_var(crate::workspace::SERVER_TOKEN_ENV, &token);
    std::env::set_var(crate::workspace::SERVER_WORKSPACE_ENV, ws.to_string());

    let daemon = Daemon::open(tmp.path().join("data")).unwrap();
    assert!(daemon.workspace_client().is_some());

    // Nothing is shared until asked; share() uploads the neutral record.
    let mut req = launch_req(&repo, "true");
    req.entity_ref = Some("repo:acme/payments".into());
    let meta = daemon.launch(req, None).unwrap();
    wait_state(&daemon, meta.id, SessionState::Completed);
    std::fs::write(
        meta.worktree_path.clone().unwrap().join("README.md"),
        "hi\nshared work\n",
    )
    .unwrap();
    let shared = daemon.share(meta.id).await.unwrap();
    assert_eq!(shared.shared_by, "casey");
    assert_eq!(shared.harness, "claude-code");
    assert_eq!(shared.entity_ref.as_deref(), Some("repo:acme/payments"));

    // Pickup renders the record for a DIFFERENT harness: new local session,
    // pickup doc present, entity carried, takeover prompt recorded.
    let picked = daemon
        .pickup(PickupRequest {
            workspace_session_id: shared.id,
            harness: Harness::CopilotCli,
            repo_root: repo.clone(),
            checkout: CheckoutMode::default(),
            command_override: Some(sh("true")),
        })
        .await
        .unwrap();
    assert_eq!(picked.harness, Harness::CopilotCli);
    assert!(picked.title.starts_with("pickup: "));
    assert_eq!(picked.entity_ref.as_deref(), Some("repo:acme/payments"));
    let wt = picked.worktree_path.clone().unwrap();
    let doc = std::fs::read_to_string(wt.join(".openade/pickup.md")).unwrap();
    assert!(doc.contains("Shared by **casey**"), "{doc}");
    assert!(doc.contains("originally run on claude-code"), "{doc}");
    assert!(doc.contains("## Original prompts"), "{doc}");
    let events = daemon.store().read_transcript(picked.id).unwrap();
    assert!(events.iter().any(|e| e.kind == EventKind::Prompt
        && e.payload["text"]
            .as_str()
            .is_some_and(|t| t.contains("picking up"))));

    // Bundle read-back: teammates' shared work reaches the next session's
    // context (needs a catalog for the entity itself).
    let daemon2 = daemon.with_catalog(Arc::new(
        catalog_mcp::testutil::MockProvider::with_payments_graph(),
    ));
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    let bundle = daemon2
        .build_bundle("repo:acme/payments-service", None)
        .await;
    // Different entity → workspace entries filtered out, but the doc link
    // is present whenever a workspace is configured.
    let bundle = bundle.unwrap();
    assert!(bundle
        .docs
        .iter()
        .any(|d| d.title.starts_with("Team workspace")));
    assert!(!bundle
        .prior_sessions
        .iter()
        .any(|p| p.session_id.starts_with("workspace-")));
    std::env::remove_var("OPENADE_GH_BIN");

    // Matching entity → the shared session becomes prior context in the
    // next session's bundle, attributed to its sharer.
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    let client = daemon2.workspace_client().unwrap();
    client
        .upload(&serde_json::json!({
            "title": "tune the poller",
            "harness": "gemini-cli",
            "entity_ref": "repo:acme/payments-service",
            "summary": "Raised the poll interval",
            "markdown": "# tune the poller",
            "events": [],
        }))
        .await
        .unwrap();
    let bundle = daemon2
        .build_bundle("repo:acme/payments-service", None)
        .await
        .unwrap();
    assert!(bundle
        .prior_sessions
        .iter()
        .any(|p| p.session_id.starts_with("workspace-") && p.summary.contains("shared by casey")));

    // Server unreachable → the read-back degrades silently (doc link stays,
    // no workspace prior sessions, bundle still builds).
    std::env::set_var(crate::workspace::SERVER_URL_ENV, "http://127.0.0.1:1");
    let bundle = daemon2
        .build_bundle("repo:acme/payments-service", None)
        .await
        .unwrap();
    assert!(bundle
        .docs
        .iter()
        .any(|d| d.title.starts_with("Team workspace")));
    assert!(!bundle
        .prior_sessions
        .iter()
        .any(|p| p.session_id.starts_with("workspace-")));
    std::env::set_var(crate::workspace::SERVER_URL_ENV, &url);
    std::env::remove_var("OPENADE_GH_BIN");

    // Records without prompt events (or with malformed events) still pick
    // up cleanly — the takeover doc just has no "Original prompts" section.
    for events in [
        serde_json::json!([{ "kind": "output" }]),
        serde_json::json!(42),
    ] {
        let bare = client
            .upload(&serde_json::json!({
                "title": "bare notes",
                "harness": "codex-cli",
                "summary": "s",
                "markdown": "# bare",
                "events": events,
            }))
            .await
            .unwrap();
        let picked = daemon2
            .pickup(PickupRequest {
                workspace_session_id: bare.id,
                harness: Harness::ClaudeCode,
                repo_root: repo.clone(),
                checkout: CheckoutMode::default(),
                command_override: Some(sh("true")),
            })
            .await
            .unwrap();
        let doc = std::fs::read_to_string(
            picked
                .worktree_path
                .clone()
                .unwrap()
                .join(".openade/pickup.md"),
        )
        .unwrap();
        assert!(!doc.contains("## Original prompts"), "{doc}");
    }

    // Unconfigured daemon: share fails with instructions; pickup too.
    for var in [
        crate::workspace::SERVER_URL_ENV,
        crate::workspace::SERVER_TOKEN_ENV,
        crate::workspace::SERVER_WORKSPACE_ENV,
    ] {
        std::env::remove_var(var);
    }
    let err = daemon2.share(meta.id).await.unwrap_err();
    assert!(matches!(err, DaemonError::Workspace(_)));
    let err = daemon2
        .pickup(PickupRequest {
            workspace_session_id: shared.id,
            harness: Harness::ClaudeCode,
            repo_root: repo.clone(),
            checkout: CheckoutMode::default(),
            command_override: Some(sh("true")),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, DaemonError::Workspace(_)));
}
