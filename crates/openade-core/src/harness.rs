//! The coding agent harnesses OpenADE can orchestrate.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A supported coding agent CLI.
///
/// OpenADE does not ship or proxy any model access: users authenticate each
/// harness through its own native CLI. OpenADE only spawns, supervises, and
/// contextualizes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Harness {
    /// Anthropic's Claude Code CLI (`claude`).
    ClaudeCode,
    /// OpenAI's Codex CLI (`codex`).
    CodexCli,
    /// Google's Gemini CLI (`gemini`).
    GeminiCli,
}

impl Harness {
    /// All supported harnesses.
    pub const ALL: [Harness; 3] = [Harness::ClaudeCode, Harness::CodexCli, Harness::GeminiCli];

    /// Stable machine identifier (matches the serde representation).
    pub fn id(&self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude-code",
            Harness::CodexCli => "codex-cli",
            Harness::GeminiCli => "gemini-cli",
        }
    }

    /// Human-readable name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Harness::ClaudeCode => "Claude Code",
            Harness::CodexCli => "Codex CLI",
            Harness::GeminiCli => "Gemini CLI",
        }
    }

    /// The executable each harness installs on PATH.
    pub fn program(&self) -> &'static str {
        match self {
            Harness::ClaudeCode => "claude",
            Harness::CodexCli => "codex",
            Harness::GeminiCli => "gemini",
        }
    }

    /// The per-project rules file this harness reads.
    ///
    /// OpenADE keeps one canonical rules source (see [`crate::rules`]) and
    /// materializes it to each of these filenames so behavior does not change
    /// when the user switches harness.
    pub fn rules_filename(&self) -> &'static str {
        match self {
            Harness::ClaudeCode => "CLAUDE.md",
            Harness::CodexCli => "AGENTS.md",
            Harness::GeminiCli => "GEMINI.md",
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// Error returned when parsing an unknown harness identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown harness id: {0:?} (expected one of: claude-code, codex-cli, gemini-cli)")]
pub struct UnknownHarness(pub String);

impl FromStr for Harness {
    type Err = UnknownHarness;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-code" | "claude" => Ok(Harness::ClaudeCode),
            "codex-cli" | "codex" => Ok(Harness::CodexCli),
            "gemini-cli" | "gemini" => Ok(Harness::GeminiCli),
            other => Err(UnknownHarness(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_serde_and_from_str() {
        for h in Harness::ALL {
            let json = serde_json::to_string(&h).unwrap();
            assert_eq!(json, format!("\"{}\"", h.id()));
            let back: Harness = serde_json::from_str(&json).unwrap();
            assert_eq!(back, h);
            assert_eq!(h.id().parse::<Harness>().unwrap(), h);
        }
    }

    #[test]
    fn short_aliases_parse() {
        assert_eq!("claude".parse::<Harness>().unwrap(), Harness::ClaudeCode);
        assert_eq!("codex".parse::<Harness>().unwrap(), Harness::CodexCli);
        assert_eq!("gemini".parse::<Harness>().unwrap(), Harness::GeminiCli);
        assert!("cursor".parse::<Harness>().is_err());
    }

    #[test]
    fn rules_filenames_are_distinct() {
        let names: std::collections::HashSet<_> =
            Harness::ALL.iter().map(|h| h.rules_filename()).collect();
        assert_eq!(names.len(), Harness::ALL.len());
    }
}
