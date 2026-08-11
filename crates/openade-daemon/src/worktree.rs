//! Git worktree isolation (PRD R2).
//!
//! Every task gets its own worktree on its own branch, so parallel sessions
//! on one repository never fight over a checkout. Worktree creation and
//! removal shell out to the `git` CLI — worktree edge cases are safer via
//! the CLI than via library bindings (PRD §7.2) — while callers can still
//! use libraries for status/diff.

use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

/// Branch prefix for OpenADE task branches.
pub const BRANCH_PREFIX: &str = "openade/";

/// A created task worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWorktree {
    /// Unique worktree name (last path component).
    pub name: String,
    /// Absolute path of the worktree checkout.
    pub path: PathBuf,
    /// The task branch checked out in the worktree.
    pub branch: String,
}

/// Errors from worktree operations.
#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("`{0}` is not a git repository (or git is not installed)")]
    NotARepo(PathBuf),
    #[error("worktree at {0} has uncommitted changes; pass force to remove anyway")]
    Dirty(PathBuf),
    #[error("git {args:?} failed: {stderr}")]
    Git { args: Vec<String>, stderr: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Creates and removes task worktrees for one repository.
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
    worktrees_root: PathBuf,
}

impl WorktreeManager {
    /// `repo_root` is the primary checkout; `worktrees_root` is the directory
    /// task worktrees are created under (kept **outside** the repository so
    /// they never show up in its status).
    pub fn new(repo_root: impl Into<PathBuf>, worktrees_root: impl Into<PathBuf>) -> Self {
        WorktreeManager {
            repo_root: repo_root.into(),
            worktrees_root: worktrees_root.into(),
        }
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String, WorktreeError> {
        let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(WorktreeError::Git {
                args: args.iter().map(|s| s.to_string()).collect(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }

    fn ensure_repo(&self) -> Result<(), WorktreeError> {
        self.git(&self.repo_root, &["rev-parse", "--git-dir"])
            .map(|_| ())
            .map_err(|_| WorktreeError::NotARepo(self.repo_root.clone()))
    }

    /// Turn a task title into a filesystem/branch-safe slug.
    pub fn slugify(title: &str) -> String {
        let mut slug: String = title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        while slug.contains("--") {
            slug = slug.replace("--", "-");
        }
        let slug = slug.trim_matches('-');
        let slug = if slug.is_empty() { "task" } else { slug };
        slug.chars().take(40).collect()
    }

    /// Create a new worktree + branch for a task. The name embeds a random
    /// suffix, so concurrent creations for identical titles never collide.
    pub fn create(&self, task_title: &str) -> Result<TaskWorktree, WorktreeError> {
        self.ensure_repo()?;
        std::fs::create_dir_all(&self.worktrees_root)?;

        let short_id = Uuid::new_v4().simple().to_string()[..8].to_string();
        let name = format!("{}-{short_id}", Self::slugify(task_title));
        let branch = format!("{BRANCH_PREFIX}{name}");
        let path = self.worktrees_root.join(&name);
        let path_str = path.to_string_lossy().into_owned();

        self.git(
            &self.repo_root,
            &["worktree", "add", "-b", &branch, &path_str],
        )?;
        Ok(TaskWorktree { name, path, branch })
    }

    /// Whether a worktree has uncommitted changes (staged, unstaged, or
    /// untracked).
    pub fn is_dirty(&self, worktree_path: &Path) -> Result<bool, WorktreeError> {
        let out = self.git(worktree_path, &["status", "--porcelain"])?;
        Ok(!out.trim().is_empty())
    }

    /// Remove a task worktree. Refuses to destroy uncommitted work unless
    /// `force` is set (the dirty-state guard from PRD R2). The task branch is
    /// kept — deleting unmerged branches is the user's call, not ours.
    pub fn remove(&self, worktree_path: &Path, force: bool) -> Result<(), WorktreeError> {
        if !force && self.is_dirty(worktree_path)? {
            return Err(WorktreeError::Dirty(worktree_path.to_path_buf()));
        }
        let path_str = worktree_path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path_str);
        self.git(&self.repo_root, &args)?;
        Ok(())
    }

    /// List worktree checkout paths known to the repository (includes the
    /// primary checkout).
    pub fn list(&self) -> Result<Vec<PathBuf>, WorktreeError> {
        let out = self.git(&self.repo_root, &["worktree", "list", "--porcelain"])?;
        Ok(out
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from)
            .collect())
    }

    /// Prune stale worktree bookkeeping (e.g. after a worktree directory was
    /// deleted manually).
    pub fn prune(&self) -> Result<(), WorktreeError> {
        self.git(&self.repo_root, &["worktree", "prune"])
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
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
}
