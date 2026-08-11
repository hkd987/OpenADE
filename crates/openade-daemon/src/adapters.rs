//! Harness adapters (PRD R4).
//!
//! One adapter per supported CLI owns the mapping from OpenADE's
//! harness-neutral session model to that CLI's concrete invocation: launch
//! command, resume semantics, rules file, MCP registration mechanism, and
//! transcript location.
//!
//! ⚠️ CLI flags and file formats below reflect the PRD §7.4 direction table.
//! These tools change monthly — the Phase 0 spike (docs/phase-0-spike.md)
//! verifies every mapping against current CLI versions before we rely on it.

use std::path::{Path, PathBuf};

use openade_core::Harness;
use serde::{Deserialize, Serialize};

use crate::pty::CommandSpec;

/// How a session's MCP servers are exposed to the harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum McpTransport {
    /// Spawn a local process speaking MCP over stdio.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// Connect to a streamable-HTTP MCP endpoint.
    Http { url: String },
}

/// An MCP server a session should have access to (e.g. `catalog-mcp`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerSpec {
    /// Registration name, e.g. `catalog`.
    pub name: String,
    #[serde(flatten)]
    pub transport: McpTransport,
}

/// What a session launch needs from the adapter.
#[derive(Debug, Clone, Default)]
pub struct LaunchRequest {
    /// Initial task prompt, if the harness accepts one on the command line.
    pub prompt: Option<String>,
    /// MCP servers to register for the session.
    pub mcp_servers: Vec<McpServerSpec>,
}

/// Where an MCP registration lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationScope {
    /// A file inside the worktree (`file` is worktree-relative); the daemon
    /// writes it automatically at launch.
    Project,
    /// User-level config (e.g. `~/.codex/config.toml`); surfaced to the
    /// user instead of silently editing their home directory.
    User,
}

/// A config change the harness needs before launch: write/merge `snippet`
/// into `file`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRegistration {
    pub scope: RegistrationScope,
    pub file: PathBuf,
    /// `json` or `toml` — how `snippet` should be merged.
    pub format: String,
    pub snippet: String,
    /// Human-readable note (e.g. "project-scoped; commit or gitignore").
    pub note: String,
}

/// Maps OpenADE's neutral session model onto one concrete harness CLI.
pub trait HarnessAdapter: Send + Sync {
    fn harness(&self) -> Harness;

    /// Command that starts a fresh interactive session in a worktree.
    fn launch_command(&self, req: &LaunchRequest) -> CommandSpec;

    /// Command that resumes a previous session by the harness's own
    /// session reference.
    fn resume_command(&self, session_ref: &str) -> CommandSpec;

    /// The per-project rules file this harness reads (materialized by
    /// `openade_core::rules`).
    fn rules_filename(&self) -> &'static str {
        self.harness().rules_filename()
    }

    /// Config edits required to register the given MCP servers for a session
    /// running in `worktree`.
    fn mcp_registrations(&self, worktree: &Path, servers: &[McpServerSpec])
        -> Vec<McpRegistration>;

    /// Where this harness keeps its own transcripts/session state, relative
    /// to the user's home directory (used by the capture pipeline as a
    /// secondary source; OpenADE's PTY transcript is the primary).
    fn transcript_hint(&self, home: &Path) -> PathBuf;
}

fn mcp_servers_json(servers: &[McpServerSpec]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for s in servers {
        let entry = match &s.transport {
            McpTransport::Stdio { command, args } => serde_json::json!({
                "command": command,
                "args": args,
            }),
            McpTransport::Http { url } => serde_json::json!({
                "type": "http",
                "url": url,
            }),
        };
        map.insert(s.name.clone(), entry);
    }
    serde_json::Value::Object(map)
}

/// Claude Code (`claude`).
pub struct ClaudeAdapter;

impl HarnessAdapter for ClaudeAdapter {
    fn harness(&self) -> Harness {
        Harness::ClaudeCode
    }

    fn launch_command(&self, req: &LaunchRequest) -> CommandSpec {
        let mut spec = CommandSpec::new(self.harness().program());
        if let Some(prompt) = &req.prompt {
            // Interactive session seeded with an initial prompt.
            spec = spec.arg(prompt.clone());
        }
        spec
    }

    fn resume_command(&self, session_ref: &str) -> CommandSpec {
        // Verify: `claude --resume <session-id>` (Phase 0 spike).
        CommandSpec::new(self.harness().program())
            .arg("--resume")
            .arg(session_ref)
    }

    fn mcp_registrations(
        &self,
        _worktree: &Path,
        servers: &[McpServerSpec],
    ) -> Vec<McpRegistration> {
        // Project-scoped `.mcp.json` in the worktree root.
        let snippet = serde_json::json!({ "mcpServers": mcp_servers_json(servers) });
        vec![McpRegistration {
            scope: RegistrationScope::Project,
            file: PathBuf::from(".mcp.json"),
            format: "json".into(),
            snippet: serde_json::to_string_pretty(&snippet).expect("static json"),
            note: "Project-scoped MCP config; Claude Code loads it from the worktree root.".into(),
        }]
    }

    fn transcript_hint(&self, home: &Path) -> PathBuf {
        home.join(".claude").join("projects")
    }
}

/// Codex CLI (`codex`).
pub struct CodexAdapter;

impl HarnessAdapter for CodexAdapter {
    fn harness(&self) -> Harness {
        Harness::CodexCli
    }

    fn launch_command(&self, req: &LaunchRequest) -> CommandSpec {
        let mut spec = CommandSpec::new(self.harness().program());
        if let Some(prompt) = &req.prompt {
            spec = spec.arg(prompt.clone());
        }
        spec
    }

    fn resume_command(&self, session_ref: &str) -> CommandSpec {
        // Verify: `codex resume <session-id>` (Phase 0 spike, PRD Q1).
        CommandSpec::new(self.harness().program())
            .arg("resume")
            .arg(session_ref)
    }

    fn mcp_registrations(
        &self,
        _worktree: &Path,
        servers: &[McpServerSpec],
    ) -> Vec<McpRegistration> {
        // Codex reads MCP servers from ~/.codex/config.toml ([mcp_servers.*]).
        let mut toml = String::new();
        for s in servers {
            match &s.transport {
                McpTransport::Stdio { command, args } => {
                    toml.push_str(&format!(
                        "[mcp_servers.{}]\ncommand = {:?}\n",
                        s.name, command
                    ));
                    if !args.is_empty() {
                        let list: Vec<String> = args.iter().map(|a| format!("{a:?}")).collect();
                        toml.push_str(&format!("args = [{}]\n", list.join(", ")));
                    }
                }
                McpTransport::Http { url } => {
                    toml.push_str(&format!("[mcp_servers.{}]\nurl = {:?}\n", s.name, url));
                }
            }
            toml.push('\n');
        }
        vec![McpRegistration {
            scope: RegistrationScope::User,
            file: PathBuf::from("~/.codex/config.toml"),
            format: "toml".into(),
            snippet: toml.trim_end().to_string(),
            note: "User-scoped; Codex CLI has no project-scoped MCP config as of PRD writing — \
                   verify in Phase 0 spike."
                .into(),
        }]
    }

    fn transcript_hint(&self, home: &Path) -> PathBuf {
        home.join(".codex").join("sessions")
    }
}

/// Gemini CLI (`gemini`).
pub struct GeminiAdapter;

impl HarnessAdapter for GeminiAdapter {
    fn harness(&self) -> Harness {
        Harness::GeminiCli
    }

    fn launch_command(&self, req: &LaunchRequest) -> CommandSpec {
        let mut spec = CommandSpec::new(self.harness().program());
        if let Some(prompt) = &req.prompt {
            // Verify: `gemini -i <prompt>` starts interactive with a seed
            // prompt (Phase 0 spike).
            spec = spec.arg("-i").arg(prompt.clone());
        }
        spec
    }

    fn resume_command(&self, session_ref: &str) -> CommandSpec {
        // Verify: checkpoint/resume semantics (Phase 0 spike, PRD Q1).
        CommandSpec::new(self.harness().program())
            .arg("--resume")
            .arg(session_ref)
    }

    fn mcp_registrations(
        &self,
        _worktree: &Path,
        servers: &[McpServerSpec],
    ) -> Vec<McpRegistration> {
        // Project-scoped .gemini/settings.json with an mcpServers block.
        let snippet = serde_json::json!({ "mcpServers": mcp_servers_json(servers) });
        vec![McpRegistration {
            scope: RegistrationScope::Project,
            file: PathBuf::from(".gemini/settings.json"),
            format: "json".into(),
            snippet: serde_json::to_string_pretty(&snippet).expect("static json"),
            note: "Project-scoped Gemini CLI settings in the worktree.".into(),
        }]
    }

    fn transcript_hint(&self, home: &Path) -> PathBuf {
        home.join(".gemini").join("tmp")
    }
}

/// GitHub Copilot CLI (`copilot`).
pub struct CopilotAdapter;

impl HarnessAdapter for CopilotAdapter {
    fn harness(&self) -> Harness {
        Harness::CopilotCli
    }

    fn launch_command(&self, req: &LaunchRequest) -> CommandSpec {
        let mut spec = CommandSpec::new(self.harness().program());
        if let Some(prompt) = &req.prompt {
            // Verify: `copilot -p <prompt>` seeds the session with an
            // initial prompt (Phase 0 spike).
            spec = spec.arg("-p").arg(prompt.clone());
        }
        spec
    }

    fn resume_command(&self, session_ref: &str) -> CommandSpec {
        // Verify: `copilot --resume <session-id>` (Phase 0 spike, PRD Q1).
        CommandSpec::new(self.harness().program())
            .arg("--resume")
            .arg(session_ref)
    }

    fn mcp_registrations(
        &self,
        _worktree: &Path,
        servers: &[McpServerSpec],
    ) -> Vec<McpRegistration> {
        // Copilot CLI reads MCP servers from ~/.copilot/mcp-config.json.
        let snippet = serde_json::json!({ "mcpServers": mcp_servers_json(servers) });
        vec![McpRegistration {
            scope: RegistrationScope::User,
            file: PathBuf::from("~/.copilot/mcp-config.json"),
            format: "json".into(),
            snippet: serde_json::to_string_pretty(&snippet).expect("static json"),
            note: "User-scoped; Copilot CLI has no project-scoped MCP config as of PRD \
                   writing — verify in Phase 0 spike."
                .into(),
        }]
    }

    fn transcript_hint(&self, home: &Path) -> PathBuf {
        home.join(".copilot").join("history-session-state")
    }
}

/// The adapter for a harness.
pub fn adapter_for(harness: Harness) -> &'static dyn HarnessAdapter {
    match harness {
        Harness::ClaudeCode => &ClaudeAdapter,
        Harness::CodexCli => &CodexAdapter,
        Harness::GeminiCli => &GeminiAdapter,
        Harness::CopilotCli => &CopilotAdapter,
    }
}

#[cfg(test)]
#[path = "adapters_tests.rs"]
mod tests;
