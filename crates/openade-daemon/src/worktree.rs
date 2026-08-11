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
    /// The commit the task branch forked from (diff base).
    pub base_commit: String,
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

        let base_commit = self
            .git(&self.repo_root, &["rev-parse", "HEAD"])?
            .trim()
            .to_string();
        self.git(
            &self.repo_root,
            &["worktree", "add", "-b", &branch, &path_str],
        )?;
        Ok(TaskWorktree {
            name,
            path,
            branch,
            base_commit,
        })
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

    /// Commit a single file onto a new branch (from the repo's current HEAD)
    /// without touching the primary checkout or any task worktree — used for
    /// knowledge artifacts (PRD R6: artifacts land on a review branch).
    ///
    /// Uses a throwaway detached worktree so the user's checkout, index, and
    /// running sessions are never disturbed.
    pub fn commit_file_on_branch(
        &self,
        branch: &str,
        rel_path: &Path,
        content: &str,
        message: &str,
    ) -> Result<(), WorktreeError> {
        self.ensure_repo()?;
        std::fs::create_dir_all(&self.worktrees_root)?;
        let tmp = self
            .worktrees_root
            .join(format!(".publish-{}", Uuid::new_v4().simple()));
        let tmp_str = tmp.to_string_lossy().into_owned();

        self.git(
            &self.repo_root,
            &["worktree", "add", "--detach", &tmp_str, "HEAD"],
        )?;
        let result = (|| {
            self.git(&tmp, &["checkout", "-b", branch])?;
            // The target sits inside the (absolute) temp worktree, so a
            // parent always exists.
            let target = tmp.join(rel_path);
            let parent = target.parent().expect("artifact path has a parent");
            std::fs::create_dir_all(parent)?;
            std::fs::write(&target, content)?;
            let rel = rel_path.to_string_lossy().into_owned();
            self.git(&tmp, &["add", &rel])?;
            // Explicit identity: artifact commits must work on machines
            // without global git config.
            self.git(
                &tmp,
                &[
                    "-c",
                    "user.name=OpenADE",
                    "-c",
                    "user.email=openade@localhost",
                    "commit",
                    "-m",
                    message,
                ],
            )?;
            Ok(())
        })();
        // Always clean up the throwaway worktree; the branch keeps the commit.
        let _ = self.git(
            &self.repo_root,
            &["worktree", "remove", "--force", &tmp_str],
        );
        result
    }

    /// The task's full diff: everything in the worktree (committed on the
    /// task branch + staged + unstaged) relative to `base_commit`.
    pub fn diff(&self, worktree_path: &Path, base_commit: &str) -> Result<String, WorktreeError> {
        self.git(worktree_path, &["diff", base_commit])
    }

    /// Files present in a worktree: tracked plus untracked-but-not-ignored,
    /// sorted, repo-relative.
    pub fn files(&self, worktree_path: &Path) -> Result<Vec<String>, WorktreeError> {
        let out = self.git(
            worktree_path,
            &["ls-files", "--cached", "--others", "--exclude-standard"],
        )?;
        let mut files: Vec<String> = out.lines().map(str::to_string).collect();
        files.sort();
        files.dedup();
        Ok(files)
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
