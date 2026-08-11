//! The versioned context bundle injected into agent sessions (PRD §7.3).
//!
//! A bundle is a compact, budgeted summary of what the catalog knows about
//! the entity a session was launched from: the entity card, its owner, its
//! nearest dependencies, its API surfaces, links to docs/ADRs, and summaries
//! of prior sessions on the same entity. Everything deeper is fetched
//! on demand through the `catalog-mcp` tools.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current bundle schema version. Bump on breaking changes and keep readers
/// tolerant of older versions.
pub const CONTEXT_BUNDLE_VERSION: u32 = 1;

/// Soft token budget for the injected bundle (PRD §7.3: ~2–4K tokens).
/// [`ContextBundle::estimated_tokens`] lets callers check before injecting.
pub const CONTEXT_BUNDLE_TOKEN_BUDGET: usize = 4000;

/// The compact context injected at session launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBundle {
    /// Schema version, see [`CONTEXT_BUNDLE_VERSION`].
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub entity: EntityCard,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<OwnerCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<RelatedEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apis: Vec<ApiCard>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<DocLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prior_sessions: Vec<PriorSessionSummary>,
}

/// Summary card for the entity the session targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityCard {
    /// Full entity reference, e.g. `component:default/payments-api`.
    pub entity_ref: String,
    /// Display title (falls back to the entity name).
    pub title: String,
    /// Backstage kind: Component, API, System, ...
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Who owns the entity and how to reach them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerCard {
    /// e.g. `group:default/payments-team`.
    pub entity_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Contact channel (email, chat link, ...), if the catalog knows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
}

/// A related entity reached through a catalog relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedEntity {
    pub entity_ref: String,
    /// Relation type, e.g. `dependsOn`, `dependencyOf`.
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// An API surface the entity provides or consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCard {
    pub entity_ref: String,
    /// `providesApi` or `consumesApi`.
    pub relation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A pointer to documentation (TechDocs page, ADR, runbook, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocLink {
    pub title: String,
    pub url: String,
}

/// A summary of a prior OpenADE session on the same entity (P1: fed back so
/// every session makes the next one smarter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorSessionSummary {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub summary: String,
}

impl ContextBundle {
    /// Create an empty bundle for an entity, at the current schema version.
    pub fn new(entity: EntityCard) -> Self {
        ContextBundle {
            version: CONTEXT_BUNDLE_VERSION,
            generated_at: Utc::now(),
            entity,
            owner: None,
            dependencies: Vec::new(),
            apis: Vec::new(),
            docs: Vec::new(),
            prior_sessions: Vec::new(),
        }
    }

    /// Rough token estimate of the rendered markdown (chars / 4 heuristic).
    ///
    /// Callers should compare against [`CONTEXT_BUNDLE_TOKEN_BUDGET`] and trim
    /// (dependencies / prior sessions first) before injecting.
    pub fn estimated_tokens(&self) -> usize {
        self.to_markdown().chars().count() / 4
    }

    /// Whether the rendered bundle fits the soft token budget.
    pub fn within_budget(&self) -> bool {
        self.estimated_tokens() <= CONTEXT_BUNDLE_TOKEN_BUDGET
    }

    /// Render the bundle as markdown suitable for injection into a session's
    /// system context. Deeper retrieval happens via `catalog-mcp` tools.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let e = &self.entity;

        out.push_str(&format!("# System context: {}\n\n", e.title));
        out.push_str(&format!(
            "- **Entity:** `{}` (kind: {}",
            e.entity_ref, e.kind
        ));
        if let Some(t) = &e.entity_type {
            out.push_str(&format!(", type: {t}"));
        }
        if let Some(l) = &e.lifecycle {
            out.push_str(&format!(", lifecycle: {l}"));
        }
        out.push_str(")\n");
        if let Some(d) = &e.description {
            out.push_str(&format!("- **Description:** {d}\n"));
        }
        if !e.tags.is_empty() {
            out.push_str(&format!("- **Tags:** {}\n", e.tags.join(", ")));
        }

        if let Some(owner) = &self.owner {
            let name = owner.display_name.as_deref().unwrap_or(&owner.entity_ref);
            out.push_str(&format!("- **Owner:** {name} (`{}`)", owner.entity_ref));
            if let Some(c) = &owner.contact {
                out.push_str(&format!(" — contact: {c}"));
            }
            out.push('\n');
        }

        if !self.dependencies.is_empty() {
            out.push_str("\n## Dependencies\n\n");
            for dep in &self.dependencies {
                out.push_str(&format!("- `{}` ({})", dep.entity_ref, dep.relation));
                if let Some(t) = &dep.title {
                    out.push_str(&format!(" — {t}"));
                }
                if let Some(d) = &dep.description {
                    out.push_str(&format!(": {d}"));
                }
                out.push('\n');
            }
        }

        if !self.apis.is_empty() {
            out.push_str("\n## APIs\n\n");
            for api in &self.apis {
                out.push_str(&format!("- `{}` ({})", api.entity_ref, api.relation));
                if let Some(t) = &api.api_type {
                    out.push_str(&format!(" — type: {t}"));
                }
                if let Some(d) = &api.description {
                    out.push_str(&format!(": {d}"));
                }
                out.push('\n');
            }
        }

        if !self.docs.is_empty() {
            out.push_str("\n## Documentation\n\n");
            for doc in &self.docs {
                out.push_str(&format!("- [{}]({})\n", doc.title, doc.url));
            }
        }

        if !self.prior_sessions.is_empty() {
            out.push_str("\n## Prior sessions on this entity\n\n");
            for s in &self.prior_sessions {
                out.push_str(&format!("- ({}) {}\n", s.session_id, s.summary));
            }
        }

        out.push_str(
            "\n> Deeper context (full entity data, TechDocs pages, catalog search) is \
             available on demand through the `catalog` MCP tools.\n",
        );
        out
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
