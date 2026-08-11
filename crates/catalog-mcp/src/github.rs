//! GitHub repositories as a memory source, through the user's local `gh` CLI.
//!
//! Same posture as the harness CLIs (PRD §7.5): **OpenADE never touches
//! GitHub credentials**. The provider shells out to the locally-installed,
//! locally-authenticated `gh` binary, which owns auth (`gh auth login`),
//! GitHub Enterprise hosts, and rate limits.
//!
//! Entity mapping: a repository is `repo:{owner}/{name}` — the owner is the
//! namespace, fitting the `kind:namespace/name` ref format with no parser
//! changes. Ownership comes from CODEOWNERS; README/docs/ADRs are served as
//! "techdocs" pages via the contents API.

use std::path::PathBuf;

use async_trait::async_trait;

use crate::provider::{
    CatalogProvider, Entity, EntityLink, EntityMetadata, EntityRef, ProviderError, Relation,
};

/// Entity kind served by this source.
pub const REPO_KIND: &str = "repo";

/// CODEOWNERS locations, in GitHub's documented precedence order.
const CODEOWNERS_PATHS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

/// [`CatalogProvider`] backed by the user's `gh` CLI.
pub struct GithubProvider {
    gh_bin: PathBuf,
}

/// Resolve an executable on PATH (no shell involved).
fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Resolve the `gh` binary the way every gh-backed feature does:
/// `OPENADE_GH_BIN` override first, then PATH.
pub fn resolve_gh_bin() -> Option<PathBuf> {
    if let Some(bin) = std::env::var_os("OPENADE_GH_BIN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(bin));
    }
    find_on_path("gh")
}

impl GithubProvider {
    pub fn new(gh_bin: impl Into<PathBuf>) -> Self {
        GithubProvider {
            gh_bin: gh_bin.into(),
        }
    }

    /// Build from the environment. Enabled when a `gh` binary resolves
    /// (`OPENADE_GH_BIN` override, else PATH); `OPENADE_GITHUB_MEMORY=0`
    /// disables the source explicitly.
    pub fn from_env() -> Result<Self, String> {
        if std::env::var("OPENADE_GITHUB_MEMORY").as_deref() == Ok("0") {
            return Err("github memory source disabled (OPENADE_GITHUB_MEMORY=0)".into());
        }
        resolve_gh_bin().map(GithubProvider::new).ok_or_else(|| {
            "gh CLI not found on PATH (install GitHub CLI and run `gh auth login` \
             to enable the GitHub memory source)"
                .into()
        })
    }

    /// Run `gh` with `args`; map failures onto the provider error contract.
    async fn gh(&self, args: &[&str], context: &str) -> Result<String, ProviderError> {
        let output = tokio::process::Command::new(&self.gh_bin)
            .args(args)
            .output()
            .await
            .map_err(|e| {
                ProviderError::Transport(format!(
                    "failed to run {}: {e} (is the GitHub CLI installed?)",
                    self.gh_bin.display()
                ))
            })?;
        if output.status.success() {
            return String::from_utf8(output.stdout).map_err(|e| {
                ProviderError::Transport(format!("gh produced non-UTF8 output: {e}"))
            });
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // `gh api` reports "gh: Not Found (HTTP 404)"; `gh repo view` reports
        // "Could not resolve to a Repository ...".
        if stderr.contains("HTTP 404") || stderr.contains("Could not resolve") {
            return Err(ProviderError::NotFound(context.to_string()));
        }
        if let Some(status) = parse_http_status(&stderr) {
            return Err(ProviderError::Upstream {
                status,
                message: stderr,
            });
        }
        Err(ProviderError::Transport(format!("gh failed: {stderr}")))
    }

    fn require_repo_ref(entity_ref: &EntityRef) -> Result<(), ProviderError> {
        if entity_ref.kind != REPO_KIND {
            return Err(ProviderError::NotFound(format!(
                "{entity_ref} (the github memory source serves repo:owner/name refs)"
            )));
        }
        Ok(())
    }

    /// `ownedBy` relations for a repo: CODEOWNERS global rule when present,
    /// else the repository owner itself.
    async fn ownership_relations(&self, owner: &str, name: &str) -> Vec<Relation> {
        for path in CODEOWNERS_PATHS {
            let api_path = format!("repos/{owner}/{name}/contents/{path}");
            let args = ["api", &api_path, "-H", "Accept: application/vnd.github.raw"];
            if let Ok(content) = self.gh(&args, &api_path).await {
                let owners = parse_codeowners(&content);
                if !owners.is_empty() {
                    return owners
                        .iter()
                        .filter_map(|token| codeowner_to_ref(token))
                        .map(|target_ref| Relation {
                            relation_type: "ownedBy".into(),
                            target_ref,
                        })
                        .collect();
                }
            }
        }
        vec![Relation {
            relation_type: "ownedBy".into(),
            target_ref: format!("group:github/{owner}"),
        }]
    }
}

/// Extract an "HTTP NNN" status from gh's stderr, if present.
fn parse_http_status(stderr: &str) -> Option<u16> {
    let idx = stderr.find("HTTP ")?;
    stderr[idx + 5..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

/// Owners from the global (`*`) rule of a CODEOWNERS file.
///
/// CODEOWNERS maps path patterns to owners; the `*` rule is the repo-wide
/// default. Later rules win in GitHub's semantics, so the *last* `*` rule is
/// authoritative.
pub fn parse_codeowners(content: &str) -> Vec<String> {
    let mut global: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // The line is trimmed and non-empty, so a first token always exists.
        let mut parts = line.split_whitespace();
        if parts.next() == Some("*") {
            global = parts.map(str::to_string).collect();
        }
    }
    global
}

/// Map a CODEOWNERS token to an entity ref: `@org/team` → `group:{org}/{team}`,
/// `@user` → `user:github/{user}`. Email owners are skipped (no stable ref).
fn codeowner_to_ref(token: &str) -> Option<String> {
    let handle = token.strip_prefix('@')?;
    Some(match handle.split_once('/') {
        Some((org, team)) => format!("group:{org}/{team}"),
        None => format!("user:github/{handle}"),
    })
}

/// Pull `.name` out of gh JSON values that may be objects or plain strings
/// (`repositoryTopics` and `primaryLanguage` shapes vary by gh version).
fn name_of(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("name")?.as_str().map(str::to_string))
}

#[async_trait]
impl CatalogProvider for GithubProvider {
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError> {
        Self::require_repo_ref(entity_ref)?;
        let (owner, name) = (&entity_ref.namespace, &entity_ref.name);
        let full = format!("{owner}/{name}");
        let out = self
            .gh(
                &[
                    "repo",
                    "view",
                    &full,
                    "--json",
                    "name,owner,description,url,homepageUrl,repositoryTopics,\
                     primaryLanguage,isArchived,defaultBranchRef",
                ],
                &entity_ref.to_string(),
            )
            .await?;
        let v: serde_json::Value = serde_json::from_str(&out).map_err(|e| {
            ProviderError::Transport(format!("unexpected gh repo view output: {e}"))
        })?;

        let mut links = Vec::new();
        if let Some(url) = v.get("url").and_then(|u| u.as_str()) {
            links.push(EntityLink {
                url: url.to_string(),
                title: Some("Repository".into()),
            });
        }
        if let Some(home) = v.get("homepageUrl").and_then(|u| u.as_str()) {
            if !home.is_empty() {
                links.push(EntityLink {
                    url: home.to_string(),
                    title: Some("Homepage".into()),
                });
            }
        }

        let tags = v
            .get("repositoryTopics")
            .and_then(|t| t.as_array())
            .map(|topics| topics.iter().filter_map(name_of).collect())
            .unwrap_or_default();

        let lifecycle = if v.get("isArchived").and_then(|a| a.as_bool()) == Some(true) {
            "archived"
        } else {
            "active"
        };
        let spec = serde_json::json!({
            "type": v.get("primaryLanguage").and_then(name_of),
            "lifecycle": lifecycle,
            "default_branch": v.pointer("/defaultBranchRef/name").and_then(|b| b.as_str()),
        });

        let relations = self.ownership_relations(owner, name).await;

        Ok(Entity {
            api_version: "openade.dev/github-v1".into(),
            kind: REPO_KIND.into(),
            metadata: EntityMetadata {
                name: name.clone(),
                namespace: Some(owner.clone()),
                title: Some(full),
                description: v
                    .get("description")
                    .and_then(|d| d.as_str())
                    .filter(|d| !d.is_empty())
                    .map(str::to_string),
                tags,
                links,
                ..Default::default()
            },
            spec,
            relations,
        })
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Entity>, ProviderError> {
        let limit_str = limit.max(1).to_string();
        let out = self
            .gh(
                &[
                    "search",
                    "repos",
                    query,
                    "--limit",
                    &limit_str,
                    "--json",
                    "fullName,description,url",
                ],
                query,
            )
            .await?;
        let items: Vec<serde_json::Value> = serde_json::from_str(&out)
            .map_err(|e| ProviderError::Transport(format!("unexpected gh search output: {e}")))?;
        Ok(items
            .into_iter()
            .filter_map(|item| {
                let full = item.get("fullName")?.as_str()?.to_string();
                let (owner, name) = full.split_once('/')?;
                Some(Entity {
                    api_version: "openade.dev/github-v1".into(),
                    kind: REPO_KIND.into(),
                    metadata: EntityMetadata {
                        name: name.to_string(),
                        namespace: Some(owner.to_string()),
                        title: Some(full.clone()),
                        description: item
                            .get("description")
                            .and_then(|d| d.as_str())
                            .filter(|d| !d.is_empty())
                            .map(str::to_string),
                        links: item
                            .get("url")
                            .and_then(|u| u.as_str())
                            .map(|url| {
                                vec![EntityLink {
                                    url: url.to_string(),
                                    title: Some("Repository".into()),
                                }]
                            })
                            .unwrap_or_default(),
                        ..Default::default()
                    },
                    spec: serde_json::Value::Null,
                    relations: Vec::new(),
                })
            })
            .collect())
    }

    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError> {
        Self::require_repo_ref(entity_ref)?;
        let page = page_path.trim_start_matches('/');
        let api_path = format!(
            "repos/{}/{}/contents/{page}",
            entity_ref.namespace, entity_ref.name
        );
        self.gh(
            &["api", &api_path, "-H", "Accept: application/vnd.github.raw"],
            &format!("{page} in {entity_ref}"),
        )
        .await
    }
}

#[cfg(test)]
#[path = "github_tests.rs"]
mod tests;
