//! Shared team memory repository.
//!
//! One GitHub repository the whole team has write access to; session
//! knowledge is committed **straight to its default branch** (people just
//! push to main on it) through the user's own `gh` CLI — OpenADE never
//! holds credentials. This complements the per-project review-branch flow:
//! the local repo keeps the human-reviewed `openade/knowledge-*` branch,
//! while the shared repo accumulates the team's live memory immediately.
//!
//! Layout in the shared repo:
//! - `sessions/<slug>.md` — one document per published session
//! - `index.md` — newest-first index (same format as the local one)

use std::path::PathBuf;
use std::process::Command;

use base64::Engine;

/// Configure with `OPENADE_MEMORY_REPO=owner/name`.
pub const MEMORY_REPO_ENV: &str = "OPENADE_MEMORY_REPO";

/// Repo-relative path of the committed team configuration: one
/// `owner/name` line naming the shared memory repository. Committed once
/// by the team, picked up by every member's OpenADE with zero setup.
pub const MEMORY_REPO_FILE: &str = ".openade/memory-repo";

/// `owner/name` from a GitHub remote URL, as a `repo:` entity ref.
/// Understands the shapes `git remote get-url` produces — scp-like
/// (`git@github.com:owner/name.git`), ssh://, and https:// — for
/// github.com and GitHub Enterprise hosts (any host containing "github").
/// Non-GitHub remotes return `None`: they have no `repo:` memory to offer.
pub fn github_entity_from_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    // Normalize the scp-like form to host/owner/name; URL forms
    // (ssh://git@host/owner/name, https://host/owner/name) drop the scheme.
    let rest = match url.strip_prefix("git@") {
        Some(rest) => rest.replacen(':', "/", 1),
        None => url
            .split_once("://")?
            .1
            .trim_start_matches("git@")
            .to_string(),
    };
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let host = parts.next()?;
    if !host.contains("github") {
        return None;
    }
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some(format!("repo:{owner}/{name}"))
}

/// A writable shared memory repository, driven by the local `gh` CLI.
#[derive(Debug, Clone)]
pub struct MemoryRepo {
    gh_bin: PathBuf,
    /// `owner/name`.
    repo: String,
}

impl MemoryRepo {
    pub fn new(gh_bin: impl Into<PathBuf>, repo: impl Into<String>) -> Self {
        MemoryRepo {
            gh_bin: gh_bin.into(),
            repo: repo.into(),
        }
    }

    /// Enabled when `OPENADE_MEMORY_REPO=owner/name` is set and a `gh`
    /// binary resolves (same resolution as the GitHub memory source).
    /// A configured-but-unusable setup is warned about, not silently
    /// dropped — the user asked for shared memory and should hear why
    /// they aren't getting it.
    pub fn from_env() -> Option<Self> {
        let repo = std::env::var(MEMORY_REPO_ENV).ok()?;
        if repo.split('/').filter(|part| !part.is_empty()).count() != 2 {
            tracing::warn!(
                "ignoring {MEMORY_REPO_ENV}={repo:?}: expected owner/name (e.g. acme/team-memory)"
            );
            return None;
        }
        let Some(gh_bin) = catalog_mcp::github::resolve_gh_bin() else {
            let hint = catalog_mcp::github::GH_SETUP_HINT;
            tracing::warn!("{MEMORY_REPO_ENV} is set but no gh CLI was found — {hint}");
            return None;
        };
        Some(MemoryRepo::new(gh_bin, repo))
    }

    /// The shared memory repo a repository declares for itself in its
    /// committed [`MEMORY_REPO_FILE`] (first non-comment line, `owner/name`).
    /// Team-level configuration: commit the file once and every member's
    /// OpenADE reads and writes the same memory with zero personal setup.
    pub fn for_repo(repo_root: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(repo_root.join(MEMORY_REPO_FILE)).ok()?;
        let repo = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))?
            .to_string();
        if repo.split('/').filter(|part| !part.is_empty()).count() != 2 {
            let root = repo_root.display();
            tracing::warn!(
                "ignoring {MEMORY_REPO_FILE} in {root}: expected owner/name, got {repo:?}"
            );
            return None;
        }
        let Some(gh_bin) = catalog_mcp::github::resolve_gh_bin() else {
            let hint = catalog_mcp::github::GH_SETUP_HINT;
            tracing::warn!("{MEMORY_REPO_FILE} names {repo} but no gh CLI was found — {hint}");
            return None;
        };
        Some(MemoryRepo::new(gh_bin, repo))
    }

    /// `owner/name`.
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// Browser URL of the shared repository.
    pub fn html_url(&self) -> String {
        format!("https://github.com/{}", self.repo)
    }

    fn gh(&self, args: &[&str]) -> Result<String, String> {
        let hint = catalog_mcp::github::GH_SETUP_HINT;
        let output = Command::new(&self.gh_bin)
            .args(args)
            .output()
            .map_err(|e| format!("failed to run {}: {e} — {hint}", self.gh_bin.display()))?;
        if output.status.success() {
            String::from_utf8(output.stdout).map_err(|e| format!("non-UTF8 gh output: {e}"))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("gh auth login") || stderr.contains("HTTP 401") {
                Err(format!(
                    "GitHub CLI is not authenticated ({stderr}) — {hint}"
                ))
            } else {
                Err(stderr)
            }
        }
    }

    fn contents_path(&self, path: &str) -> String {
        format!(
            "repos/{}/contents/{}",
            self.repo,
            path.trim_start_matches('/')
        )
    }

    /// Read a file from the shared repo's default branch (None when missing
    /// or unreachable — shared memory reads never fail a session).
    pub fn read_file(&self, path: &str) -> Option<String> {
        self.gh(&[
            "api",
            &self.contents_path(path),
            "-H",
            "Accept: application/vnd.github.raw",
        ])
        .ok()
    }

    /// The blob sha of an existing file (required by the contents API to
    /// update in place).
    fn file_sha(&self, path: &str) -> Option<String> {
        let out = self.gh(&["api", &self.contents_path(path)]).ok()?;
        let v: serde_json::Value = serde_json::from_str(&out).ok()?;
        v.get("sha")?.as_str().map(str::to_string)
    }

    /// Create or update a file **directly on the default branch** of the
    /// shared repo, committed as the local `gh` user.
    pub fn put_file(&self, path: &str, content: &str, message: &str) -> Result<(), String> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
        let api_path = self.contents_path(path);
        let message_field = format!("message={message}");
        let content_field = format!("content={encoded}");
        let mut args = vec![
            "api",
            "-X",
            "PUT",
            api_path.as_str(),
            "-f",
            message_field.as_str(),
            "-f",
            content_field.as_str(),
        ];
        let sha_field = self.file_sha(path).map(|sha| format!("sha={sha}"));
        if let Some(sha_field) = &sha_field {
            args.push("-f");
            args.push(sha_field.as_str());
        }
        self.gh(&args).map(|_| ())
    }

    /// Publish a session document plus the updated index in two commits to
    /// the default branch.
    pub fn publish(
        &self,
        file_path: &str,
        markdown: &str,
        index: &str,
        message: &str,
    ) -> Result<(), String> {
        self.put_file(file_path, markdown, message)?;
        self.put_file("index.md", index, message)
    }
}

#[cfg(test)]
#[path = "memory_repo_tests.rs"]
pub(crate) mod tests;
