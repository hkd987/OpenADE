use super::*;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn init_repo(dir: &Path) {
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
    fs::write(dir.join("README.md"), "hello\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "init"]);
}

fn manager() -> (TempDir, WorktreeManager) {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let mgr = WorktreeManager::new(&repo, tmp.path().join("worktrees"));
    (tmp, mgr)
}

#[test]
fn slugify_sanitizes_titles() {
    assert_eq!(
        WorktreeManager::slugify("Fix flaky test!!"),
        "fix-flaky-test"
    );
    assert_eq!(WorktreeManager::slugify("  "), "task");
    assert_eq!(WorktreeManager::slugify("a b   c"), "a-b-c");
    assert!(WorktreeManager::slugify(&"x".repeat(100)).len() <= 40);
}

#[test]
fn not_a_repo_is_reported() {
    let tmp = TempDir::new().unwrap();
    let mgr = WorktreeManager::new(tmp.path().join("nope"), tmp.path().join("wt"));
    assert!(matches!(
        mgr.create("task"),
        Err(WorktreeError::NotARepo(_))
    ));
}

#[test]
fn ten_parallel_tasks_get_distinct_worktrees_and_branches() {
    // PRD R2 acceptance: 10 simultaneous sessions on one repo, zero
    // cross-session file conflicts.
    let (_tmp, mgr) = manager();
    let wts: Vec<TaskWorktree> = (0..10)
        .map(|_| mgr.create("same task title").unwrap())
        .collect();

    let paths: std::collections::HashSet<_> = wts.iter().map(|w| &w.path).collect();
    let branches: std::collections::HashSet<_> = wts.iter().map(|w| &w.branch).collect();
    assert_eq!(paths.len(), 10);
    assert_eq!(branches.len(), 10);
    for wt in &wts {
        assert!(wt.path.join("README.md").is_file());
        assert!(wt.branch.starts_with(BRANCH_PREFIX));
    }
    // Writes in one worktree do not appear in the others.
    fs::write(wts[0].path.join("only-in-0.txt"), "x").unwrap();
    assert!(!wts[1].path.join("only-in-0.txt").exists());

    // All are tracked by git.
    let listed = mgr.list().unwrap();
    assert_eq!(listed.len(), 11); // primary + 10
}

#[test]
fn dirty_guard_blocks_removal_until_forced() {
    let (_tmp, mgr) = manager();
    let wt = mgr.create("cleanup me").unwrap();

    fs::write(wt.path.join("uncommitted.txt"), "work in progress").unwrap();
    assert!(mgr.is_dirty(&wt.path).unwrap());
    assert!(matches!(
        mgr.remove(&wt.path, false),
        Err(WorktreeError::Dirty(_))
    ));
    assert!(wt.path.exists(), "dirty worktree must not be removed");

    mgr.remove(&wt.path, true).unwrap();
    assert!(!wt.path.exists());
}

#[test]
fn clean_worktree_removal_succeeds_without_force() {
    let (_tmp, mgr) = manager();
    let wt = mgr.create("clean task").unwrap();
    assert!(!mgr.is_dirty(&wt.path).unwrap());
    mgr.remove(&wt.path, false).unwrap();
    assert!(!wt.path.exists());
}

#[test]
fn commit_file_on_branch_reuses_head_and_rejects_existing_branches() {
    let (_tmp, mgr) = manager();
    mgr.commit_file_on_branch(
        "openade/knowledge-test",
        Path::new("docs/openade/sessions/a.md"),
        "artifact body\n",
        "docs: test artifact",
    )
    .unwrap();

    let show = Command::new("git")
        .arg("-C")
        .arg(mgr.repo_root())
        .args(["show", "openade/knowledge-test:docs/openade/sessions/a.md"])
        .output()
        .unwrap();
    assert!(show.status.success());
    assert_eq!(String::from_utf8_lossy(&show.stdout), "artifact body\n");

    // Same branch again: git refuses, the temp worktree is still cleaned.
    let err = mgr.commit_file_on_branch(
        "openade/knowledge-test",
        Path::new("docs/openade/sessions/b.md"),
        "x",
        "msg",
    );
    assert!(matches!(err, Err(WorktreeError::Git { .. })));
    assert_eq!(mgr.list().unwrap().len(), 1, "no stray worktrees remain");

    mgr.prune().unwrap();
}

#[test]
fn diff_covers_uncommitted_and_committed_task_work() {
    let (_tmp, mgr) = manager();
    let wt = mgr.create("diff me").unwrap();
    assert!(!wt.base_commit.is_empty());

    // A fresh worktree has no diff vs its base.
    assert!(mgr.diff(&wt.path, &wt.base_commit).unwrap().is_empty());

    // Uncommitted edit to a tracked file shows up.
    fs::write(wt.path.join("README.md"), "hello\nplus a change\n").unwrap();
    let diff = mgr.diff(&wt.path, &wt.base_commit).unwrap();
    assert!(diff.contains("+plus a change"));

    // Commit it on the task branch — still diffed against the fork point.
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(&wt.path)
            .args(args)
            .output()
            .unwrap();
        assert!(st.status.success());
    };
    run(&["config", "user.email", "t@example.com"]);
    run(&["config", "user.name", "T"]);
    run(&["commit", "-am", "task work"]);
    let diff = mgr.diff(&wt.path, &wt.base_commit).unwrap();
    assert!(diff.contains("+plus a change"));
}

#[test]
fn files_lists_tracked_and_untracked_but_not_ignored() {
    let (_tmp, mgr) = manager();
    let wt = mgr.create("browse me").unwrap();
    fs::write(wt.path.join("new-file.txt"), "x").unwrap();
    fs::write(wt.path.join(".gitignore"), "ignored.log\n").unwrap();
    fs::write(wt.path.join("ignored.log"), "x").unwrap();

    let files = mgr.files(&wt.path).unwrap();
    assert!(files.contains(&"README.md".to_string()));
    assert!(files.contains(&"new-file.txt".to_string()));
    assert!(files.contains(&".gitignore".to_string()));
    assert!(!files.iter().any(|f| f == "ignored.log"));
}

#[test]
fn create_fails_in_a_repo_without_commits() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("empty-repo");
    fs::create_dir(&repo).unwrap();
    let st = Command::new("git")
        .args(["-C", repo.to_str().unwrap(), "init", "-b", "main"])
        .output()
        .unwrap();
    assert!(st.status.success());
    let mgr = WorktreeManager::new(&repo, tmp.path().join("wt"));
    // No HEAD yet: base-commit resolution fails with a git error.
    assert!(matches!(mgr.create("task"), Err(WorktreeError::Git { .. })));
}

#[test]
fn diff_and_files_surface_git_failures() {
    let (_tmp, mgr) = manager();
    let wt = mgr.create("errors").unwrap();
    assert!(matches!(
        mgr.diff(&wt.path, "not-a-commit"),
        Err(WorktreeError::Git { .. })
    ));
    assert!(matches!(
        mgr.files(Path::new("/")),
        Err(WorktreeError::Git { .. })
    ));
}

#[test]
fn commit_file_on_branch_rejects_a_no_op_commit() {
    let (_tmp, mgr) = manager();
    // Identical content to HEAD: `git commit` refuses (nothing staged), the
    // error propagates, and the throwaway worktree is still cleaned up.
    let err = mgr.commit_file_on_branch(
        "openade/knowledge-noop",
        Path::new("README.md"),
        "hello\n",
        "docs: no-op",
    );
    assert!(matches!(err, Err(WorktreeError::Git { .. })));
    assert_eq!(mgr.list().unwrap().len(), 1);
}

/// A `git` wrapper that fails any invocation whose args contain both the
/// literal subcommand `add` and a path with the given marker — letting tests
/// inject failures into specific git calls without affecting parallel tests.
fn install_failing_git(tmp: &Path, marker: &str) -> String {
    let bin = tmp.join("failgit-bin");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nhas_add=0; has_marker=0\nfor a in \"$@\"; do\n  [ \"$a\" = add ] && has_add=1\n  case \"$a\" in *{marker}*) has_marker=1;; esac\ndone\nif [ $has_add = 1 ] && [ $has_marker = 1 ]; then echo 'injected git failure' >&2; exit 1; fi\nexec /usr/bin/git \"$@\"\n"
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

#[test]
fn git_failures_during_worktree_add_and_artifact_staging_propagate() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    init_repo(&repo);
    let path = install_failing_git(tmp.path(), "gitfail");
    let old_path = std::env::var("PATH").unwrap();
    std::env::set_var("PATH", &path);

    // `git worktree add` fails: worktrees root carries the marker.
    let mgr = WorktreeManager::new(&repo, tmp.path().join("gitfail-worktrees"));
    assert!(matches!(mgr.create("task"), Err(WorktreeError::Git { .. })));

    // `git worktree add --detach` for the throwaway publish worktree fails:
    // the worktrees root carries the marker.
    let mgr = WorktreeManager::new(&repo, tmp.path().join("gitfail-worktrees2"));
    let err = mgr.commit_file_on_branch(
        "openade/knowledge-detach-fail",
        Path::new("docs/a.md"),
        "content\n",
        "msg",
    );
    assert!(matches!(err, Err(WorktreeError::Git { .. })));

    // `git add` inside artifact publication fails: the artifact path carries
    // the marker; the throwaway worktree is still cleaned up.
    let mgr = WorktreeManager::new(&repo, tmp.path().join("clean-worktrees"));
    let err = mgr.commit_file_on_branch(
        "openade/knowledge-inject",
        Path::new("docs/gitfail/a.md"),
        "content\n",
        "msg",
    );
    assert!(matches!(err, Err(WorktreeError::Git { .. })));
    std::env::set_var("PATH", old_path);
    assert_eq!(mgr.list().unwrap().len(), 1, "temp worktree cleaned up");
}
