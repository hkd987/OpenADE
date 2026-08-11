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
    pub fn from_env() -> Option<Self> {
        let repo = std::env::var(MEMORY_REPO_ENV)
            .ok()
            .filter(|r| r.split('/').filter(|part| !part.is_empty()).count() == 2)?;
        let gh_bin = catalog_mcp::github::resolve_gh_bin()?;
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
        let output = Command::new(&self.gh_bin)
            .args(args)
            .output()
            .map_err(|e| format!("failed to run {}: {e}", self.gh_bin.display()))?;
        if output.status.success() {
            String::from_utf8(output.stdout).map_err(|e| format!("non-UTF8 gh output: {e}"))
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
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
