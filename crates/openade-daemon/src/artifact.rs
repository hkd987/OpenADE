//! Knowledge artifacts (PRD R6).
//!
//! On demand (session end), the transcript is summarized into a markdown
//! artifact and committed to the repository on a dedicated review branch
//! (`openade/knowledge-*`) under `docs/openade/sessions/`. A human reviews
//! and merges through the normal Git flow — OpenADE never publishes directly
//! (PRD §7.2 "Knowledge write-back").
//!
//! Summarization is pluggable (PRD Q5): [`TemplateSummarizer`] is the
//! deterministic, zero-config default; a model-backed summarizer (the
//! session's own harness or a user-configured endpoint) implements the same
//! [`Summarizer`] trait.

use std::path::PathBuf;

use openade_core::session::SessionMeta;
use serde::{Deserialize, Serialize};

use crate::transcript::{EventKind, SessionEvent};

/// Directory (inside the repository) where artifacts land — a TechDocs-able
/// docs path so Backstage picks them up once merged.
pub const ARTIFACT_DIR: &str = "docs/openade/sessions";

/// Branch prefix for knowledge review branches.
pub const KNOWLEDGE_BRANCH_PREFIX: &str = "openade/knowledge-";

/// A produced knowledge artifact, ready for human review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    /// Review branch in the repository carrying the commit.
    pub branch: String,
    /// Repo-relative path of the artifact file.
    pub file: PathBuf,
    /// One-line summary (also recorded as the session outcome).
    pub summary: String,
    /// Full markdown content.
    pub markdown: String,
}

/// Produces the artifact markdown from what the session left behind.
pub trait Summarizer: Send + Sync {
    /// One line: what happened (feeds prior-session context for the entity).
    fn summary_line(&self, meta: &SessionMeta, events: &[SessionEvent], diff: &str) -> String;

    /// Full markdown body of the knowledge artifact.
    fn summarize(&self, meta: &SessionMeta, events: &[SessionEvent], diff: &str) -> String;
}

/// Deterministic, zero-config summarizer: structures what is known from the
/// transcript and diff without calling any model.
pub struct TemplateSummarizer;

/// Files touched, extracted from unified diff headers.
fn changed_files(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|l| l.strip_prefix("+++ b/"))
        .map(str::to_string)
        .collect()
}

/// The latest agent/user-recorded outcome payload, if any.
fn recorded_outcome(events: &[SessionEvent]) -> Option<&serde_json::Value> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Outcome)
        .map(|e| &e.payload)
}

impl Summarizer for TemplateSummarizer {
    fn summary_line(&self, meta: &SessionMeta, events: &[SessionEvent], diff: &str) -> String {
        if let Some(summary) = recorded_outcome(events)
            .and_then(|o| o.get("summary"))
            .and_then(|s| s.as_str())
        {
            return summary.to_string();
        }
        let files = changed_files(diff);
        if files.is_empty() {
            format!(
                "{} ({}): no file changes recorded",
                meta.title, meta.harness
            )
        } else {
            format!(
                "{} ({}): touched {} file(s) incl. {}",
                meta.title,
                meta.harness,
                files.len(),
                files[0]
            )
        }
    }

    fn summarize(&self, meta: &SessionMeta, events: &[SessionEvent], diff: &str) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Session: {}\n\n", meta.title));
        out.push_str(&format!("- **Session id:** `{}`\n", meta.id));
        out.push_str(&format!("- **Harness:** {}\n", meta.harness.display_name()));
        if let Some(entity) = &meta.entity_ref {
            out.push_str(&format!("- **Entity:** `{entity}`\n"));
        }
        if let Some(branch) = &meta.branch {
            out.push_str(&format!("- **Task branch:** `{branch}`\n"));
        }
        out.push_str(&format!(
            "- **Started:** {}\n",
            meta.created_at.format("%Y-%m-%d %H:%M UTC")
        ));

        let prompts: Vec<&str> = events
            .iter()
            .filter(|e| e.kind == EventKind::Prompt)
            .filter_map(|e| e.payload.get("text").and_then(|t| t.as_str()))
            .collect();
        if !prompts.is_empty() {
            out.push_str("\n## Task\n\n");
            for p in prompts {
                out.push_str(&format!("> {p}\n"));
            }
        }

        out.push_str("\n## What changed\n\n");
        let files = changed_files(diff);
        if files.is_empty() {
            out.push_str("No file changes were recorded in the task worktree.\n");
        } else {
            for f in &files {
                out.push_str(&format!("- `{f}`\n"));
            }
        }

        if let Some(outcome) = recorded_outcome(events) {
            out.push_str("\n## Outcome\n\n");
            if let Some(s) = outcome.get("summary").and_then(|s| s.as_str()) {
                out.push_str(&format!("{s}\n"));
            }
            for (key, heading) in [
                ("decisions", "Decisions made"),
                ("gotchas", "Gotchas discovered"),
            ] {
                if let Some(items) = outcome.get(key).and_then(|v| v.as_array()) {
                    out.push_str(&format!("\n### {heading}\n\n"));
                    for item in items {
                        if let Some(text) = item.as_str() {
                            out.push_str(&format!("- {text}\n"));
                        }
                    }
                }
            }
        }

        out.push_str(&format!(
            "\n---\n\n*Captured by OpenADE from session `{}`. Review before merging; \
             this document becomes entity documentation.*\n",
            meta.id
        ));
        out
    }
}

/// Repo-relative path of the living documentation index that aggregates all
/// session artifacts (newest first). Regenerated on every publication so the
/// docs stay navigable as they accumulate.
pub const INDEX_FILE: &str = "docs/openade/sessions/index.md";

/// One line of the session knowledge index.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// Artifact file name (relative to the index).
    pub file_name: String,
    pub title: String,
    pub summary: String,
    /// ISO date of the session.
    pub date: String,
    pub harness: String,
}

/// Merge a new entry into the (possibly missing) existing index, newest
/// first. Re-publishing the same artifact replaces its entry instead of
/// duplicating it.
pub fn upsert_index(existing: Option<&str>, entry: &IndexEntry) -> String {
    let mut lines: Vec<String> = existing
        .map(|s| {
            s.lines()
                .filter(|l| l.starts_with("- ["))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let marker = format!("]({})", entry.file_name);
    lines.retain(|l| !l.contains(&marker));
    lines.insert(
        0,
        format!(
            "- [{}]({}) — {} *({}, {})*",
            entry.title, entry.file_name, entry.summary, entry.date, entry.harness
        ),
    );
    format!(
        "# Session knowledge index\n\n\
         Living documentation captured from OpenADE sessions, newest first.\n\
         Each entry links to the full session record.\n\n{}\n",
        lines.join("\n")
    )
}

/// Slug + short id for artifact filenames and branches.
pub fn artifact_slug(meta: &SessionMeta) -> String {
    let short = meta.id.simple().to_string()[..8].to_string();
    format!(
        "{}-{}-{}",
        meta.created_at.format("%Y-%m-%d"),
        crate::worktree::WorktreeManager::slugify(&meta.title),
        short
    )
}

#[cfg(test)]
#[path = "artifact_tests.rs"]
mod tests;
