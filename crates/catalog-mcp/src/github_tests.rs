use super::*;
use std::fs;
use std::path::Path;

/// Install a fake `gh` script (the same shim pattern the e2e suite uses for
/// harness CLIs) and return a provider pointed at it.
fn provider_with_shim(dir: &Path, script_body: &str) -> GithubProvider {
    let shim = dir.join("gh");
    fs::write(&shim, format!("#!/bin/sh\n{script_body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
    }
    GithubProvider::new(shim)
}

/// A well-behaved `gh` for the acme/payments-service fixture repo.
fn fixture_shim(dir: &Path) -> GithubProvider {
    provider_with_shim(
        dir,
        r#"case "$*" in
  "repo view acme/payments-service --json"*)
    cat <<'EOF'
{"name":"payments-service","owner":{"login":"acme"},
 "description":"Handles payment authorization and capture.",
 "url":"https://github.com/acme/payments-service",
 "homepageUrl":"https://payments.acme.dev",
 "repositoryTopics":[{"name":"payments"},{"name":"tier-1"}],
 "primaryLanguage":{"name":"Rust"},
 "isArchived":false,
 "defaultBranchRef":{"name":"main"}}
EOF
    ;;
  "api repos/acme/payments-service/contents/.github/CODEOWNERS"*)
    printf '# global owners\n* @acme/payments-team @lundin\ndocs/* @acme/docs-team\n'
    ;;
  "api repos/acme/payments-service/contents/README.md"*)
    printf '# Payments Service\nHandles payments.\n'
    ;;
  "search repos payments"*)
    printf '[{"fullName":"acme/payments-service","description":"Payments","url":"https://github.com/acme/payments-service"},{"fullName":"acme/ledger","description":"","url":"https://github.com/acme/ledger"}]'
    ;;
  *)
    echo "gh: Not Found (HTTP 404)" >&2
    exit 1
    ;;
esac"#,
    )
}

#[test]
fn parse_codeowners_takes_the_last_global_rule() {
    let content = "# comment\n\n* @acme/old-team\ndocs/* @acme/docs-team\n* @acme/payments-team @lundin owner@example.com\n";
    assert_eq!(
        parse_codeowners(content),
        vec!["@acme/payments-team", "@lundin", "owner@example.com"]
    );
    assert!(parse_codeowners("docs/* @acme/docs-team\n").is_empty());
    assert!(parse_codeowners("").is_empty());
    assert!(parse_codeowners("# only comments\n").is_empty());
}

#[test]
fn codeowner_tokens_map_to_entity_refs() {
    assert_eq!(
        codeowner_to_ref("@acme/payments-team").as_deref(),
        Some("group:acme/payments-team")
    );
    assert_eq!(
        codeowner_to_ref("@lundin").as_deref(),
        Some("user:github/lundin")
    );
    // Email owners have no stable entity ref.
    assert_eq!(codeowner_to_ref("owner@example.com"), None);
}

#[test]
fn http_status_parsing() {
    assert_eq!(parse_http_status("gh: Forbidden (HTTP 403)"), Some(403));
    assert_eq!(parse_http_status("HTTP 500 something broke"), Some(500));
    assert_eq!(parse_http_status("no status here"), None);
}

#[tokio::test]
async fn repo_entity_maps_metadata_topics_and_codeowners() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = fixture_shim(tmp.path());
    let entity = provider
        .get_entity(&"repo:acme/payments-service".parse().unwrap())
        .await
        .unwrap();

    assert_eq!(entity.kind, REPO_KIND);
    assert_eq!(
        entity.entity_ref().to_string(),
        "repo:acme/payments-service"
    );
    assert_eq!(entity.display_title(), "acme/payments-service");
    assert_eq!(
        entity.metadata.description.as_deref(),
        Some("Handles payment authorization and capture.")
    );
    assert_eq!(entity.metadata.tags, vec!["payments", "tier-1"]);
    assert_eq!(entity.spec["type"], "Rust");
    assert_eq!(entity.spec["lifecycle"], "active");
    assert_eq!(entity.spec["default_branch"], "main");
    assert_eq!(entity.metadata.links.len(), 2);

    // CODEOWNERS global rule: team + user, email skipped.
    assert_eq!(
        entity.relation_targets("ownedBy"),
        vec!["group:acme/payments-team", "user:github/lundin"]
    );
}

#[tokio::test]
async fn missing_codeowners_falls_back_to_the_repo_owner() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = provider_with_shim(
        tmp.path(),
        r#"case "$*" in
  "repo view acme/bare --json"*)
    printf '{"name":"bare","owner":{"login":"acme"},"description":null,"url":"https://github.com/acme/bare","homepageUrl":"","repositoryTopics":[],"primaryLanguage":null,"isArchived":true,"defaultBranchRef":{"name":"main"}}'
    ;;
  *)
    echo "gh: Not Found (HTTP 404)" >&2
    exit 1
    ;;
esac"#,
    );
    let entity = provider
        .get_entity(&"repo:acme/bare".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        entity.relation_targets("ownedBy"),
        vec!["group:github/acme"]
    );
    assert_eq!(entity.spec["lifecycle"], "archived");
    assert!(entity.metadata.description.is_none());
    assert!(entity.metadata.tags.is_empty());
    // Empty homepage is not a link.
    assert_eq!(entity.metadata.links.len(), 1);
}

#[tokio::test]
async fn search_maps_results_to_repo_entities() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = fixture_shim(tmp.path());
    let hits = provider.search("payments", 5).await.unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].entity_ref().to_string(),
        "repo:acme/payments-service"
    );
    assert_eq!(hits[0].metadata.description.as_deref(), Some("Payments"));
    // Empty description filtered to None.
    assert!(hits[1].metadata.description.is_none());
}

#[tokio::test]
async fn techdocs_serves_file_contents_and_maps_404() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = fixture_shim(tmp.path());
    let readme = provider
        .get_techdocs_page(&"repo:acme/payments-service".parse().unwrap(), "/README.md")
        .await
        .unwrap();
    assert!(readme.contains("# Payments Service"));

    let err = provider
        .get_techdocs_page(&"repo:acme/payments-service".parse().unwrap(), "missing.md")
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn non_repo_refs_are_not_served() {
    let tmp = tempfile::tempdir().unwrap();
    let provider = fixture_shim(tmp.path());
    let err = provider
        .get_entity(&"component:default/payments-api".parse().unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)));
    let err = provider
        .get_techdocs_page(&"component:default/x".parse().unwrap(), "a.md")
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)));
}

#[tokio::test]
async fn gh_failure_modes_map_to_the_error_contract() {
    let tmp = tempfile::tempdir().unwrap();

    // Missing repo → NotFound (gh repo view phrasing).
    let provider = provider_with_shim(
        tmp.path(),
        "echo 'GraphQL: Could not resolve to a Repository with the name acme/ghost.' >&2; exit 1",
    );
    let err = provider
        .get_entity(&"repo:acme/ghost".parse().unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");

    // Upstream HTTP failure with a parseable status.
    let provider = provider_with_shim(tmp.path(), "echo 'gh: Forbidden (HTTP 403)' >&2; exit 1");
    let err = provider.search("x", 3).await.unwrap_err();
    assert!(
        matches!(err, ProviderError::Upstream { status: 403, .. }),
        "{err:?}"
    );

    // Unparseable failure → Transport.
    let provider = provider_with_shim(tmp.path(), "echo 'kaboom' >&2; exit 1");
    let err = provider.search("x", 3).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");

    // Junk stdout on success → Transport (JSON parse).
    let provider = provider_with_shim(tmp.path(), "printf 'not json'");
    let err = provider
        .get_entity(&"repo:acme/junk".parse().unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
    let err = provider.search("x", 3).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");

    // Missing binary → Transport.
    let provider = GithubProvider::new(tmp.path().join("not-a-real-gh"));
    let err = provider.search("x", 3).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
}

#[test]
fn from_env_resolves_override_and_disable() {
    // Env vars are process-global; serialize with the other env tests.
    let _guard = crate::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    let provider = GithubProvider::from_env().unwrap();
    assert_eq!(provider.gh_bin, PathBuf::from("/custom/gh"));

    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");
    assert!(GithubProvider::from_env().is_err());

    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    std::env::remove_var("OPENADE_GH_BIN");
    // With no override, resolution depends on PATH; both outcomes are valid —
    // just assert it doesn't panic.
    let _ = GithubProvider::from_env();
}
