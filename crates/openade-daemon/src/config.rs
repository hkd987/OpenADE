//! Persisted daemon settings (`config.json` in the data dir).
//!
//! Collected by the desktop app's first-run onboarding so users get memory
//! working without editing shell profiles. Environment variables always win
//! over the stored file — an operator's explicit env is never silently
//! overridden — and the file only exists once the user (or the onboarding
//! flow) saves something.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// File name inside the daemon data dir.
pub const CONFIG_FILE: &str = "config.json";

/// User-editable daemon settings. Every field is optional: absent means
/// "not configured here" (the environment may still configure it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Backstage catalog base URL (memory source).
    pub backstage_base_url: Option<String>,
    /// Static bearer token for Backstage.
    pub backstage_token: Option<String>,
    /// Daemon-wide shared team memory repo (`owner/name`).
    pub memory_repo: Option<String>,
    /// Whether first-run onboarding has been completed (or skipped).
    pub onboarded: bool,
    /// Multiplayer workspace server base URL (self-hosted `openade-server`).
    pub server_url: Option<String>,
    /// Member token for the workspace server.
    pub server_token: Option<String>,
    /// Workspace id sessions are shared to / read from.
    pub server_workspace: Option<i64>,
}

impl Settings {
    /// Load from the data dir; a missing or unreadable file is simply the
    /// default (first run).
    pub fn load(data_dir: &Path) -> Settings {
        std::fs::read_to_string(data_dir.join(CONFIG_FILE))
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Persist to the data dir.
    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(data_dir.join(CONFIG_FILE), json).map_err(|e| e.to_string())
    }

    /// The effective Backstage configuration: environment first, stored
    /// settings otherwise.
    pub fn effective_backstage(&self) -> Option<catalog_mcp::backstage::BackstageConfig> {
        if let Ok(config) = catalog_mcp::backstage::BackstageConfig::from_env() {
            return Some(config);
        }
        self.backstage_base_url
            .as_ref()
            .filter(|url| !url.is_empty())
            .map(|url| catalog_mcp::backstage::BackstageConfig {
                base_url: url.clone(),
                token: self
                    .backstage_token
                    .clone()
                    .filter(|token| !token.is_empty()),
            })
    }

    /// The effective daemon-wide shared memory repo name: environment
    /// first, stored settings otherwise.
    pub fn effective_memory_repo(&self) -> Option<String> {
        std::env::var(crate::memory_repo::MEMORY_REPO_ENV)
            .ok()
            .filter(|r| !r.is_empty())
            .or_else(|| self.memory_repo.clone().filter(|r| !r.is_empty()))
    }

    /// Whether the daemon counts as onboarded: the user completed (or
    /// skipped) onboarding, or the environment already configures memory —
    /// an operator with env vars set doesn't need a welcome screen.
    pub fn effective_onboarded(&self) -> bool {
        self.onboarded
            || std::env::var("BACKSTAGE_BASE_URL").is_ok_and(|v| !v.is_empty())
            || std::env::var(crate::memory_repo::MEMORY_REPO_ENV).is_ok_and(|v| !v.is_empty())
    }
}

impl Settings {
    /// The effective multiplayer connection: env overrides
    /// (`OPENADE_SERVER_URL`/`_TOKEN`/`_WORKSPACE`) win over stored
    /// settings; all three parts must resolve or multiplayer is off.
    pub fn effective_workspace(&self) -> Option<crate::workspace::WorkspaceClient> {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let url = env(crate::workspace::SERVER_URL_ENV)
            .or_else(|| self.server_url.clone().filter(|v| !v.is_empty()))?;
        let token = env(crate::workspace::SERVER_TOKEN_ENV)
            .or_else(|| self.server_token.clone().filter(|v| !v.is_empty()))?;
        let workspace = env(crate::workspace::SERVER_WORKSPACE_ENV)
            .and_then(|v| v.parse().ok())
            .or(self.server_workspace)?;
        Some(crate::workspace::WorkspaceClient::new(
            url, token, workspace,
        ))
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
