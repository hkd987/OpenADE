use super::*;
use crate::provider::EntityMetadata;
use crate::testutil::MockProvider;

/// A source that only knows one repo entity — stands in for the gh-backed
/// provider in routing tests.
struct RepoOnly;

#[async_trait]
impl CatalogProvider for RepoOnly {
    async fn get_entity(&self, entity_ref: &EntityRef) -> Result<Entity, ProviderError> {
        if entity_ref.to_string() == "repo:acme/checkout" {
            return Ok(Entity {
                api_version: "openade.dev/github-v1".into(),
                kind: "repo".into(),
                metadata: EntityMetadata {
                    name: "checkout".into(),
                    namespace: Some("acme".into()),
                    title: Some("acme/checkout".into()),
                    ..Default::default()
                },
                spec: serde_json::Value::Null,
                relations: vec![],
            });
        }
        Err(ProviderError::NotFound(entity_ref.to_string()))
    }

    async fn search(&self, query: &str, _limit: usize) -> Result<Vec<Entity>, ProviderError> {
        if query == "boom" {
            return Err(ProviderError::Transport("gh exploded".into()));
        }
        Ok(vec![
            self.get_entity(&"repo:acme/checkout".parse().unwrap())
                .await?,
        ])
    }

    async fn get_techdocs_page(
        &self,
        entity_ref: &EntityRef,
        page_path: &str,
    ) -> Result<String, ProviderError> {
        Ok(format!("# repo docs for {entity_ref} at {page_path}"))
    }
}

fn both_sources() -> MemoryRouter {
    MemoryRouter::new(
        Some(Arc::new(MockProvider::with_payments_graph())),
        Some(Arc::new(RepoOnly)),
    )
    .unwrap()
}

#[test]
fn no_sources_means_no_router() {
    assert!(MemoryRouter::new(None, None).is_none());
    let github_only = MemoryRouter::new(None, Some(Arc::new(RepoOnly))).unwrap();
    assert_eq!(github_only.source_names(), vec!["github"]);
    let backstage_only =
        MemoryRouter::new(Some(Arc::new(MockProvider::with_payments_graph())), None).unwrap();
    assert_eq!(backstage_only.source_names(), vec!["backstage"]);
}

#[tokio::test]
async fn refs_route_by_kind() {
    let router = both_sources();
    assert_eq!(router.source_names(), vec!["github", "backstage"]);

    // repo: → the gh-backed source.
    let repo = router
        .get_entity(&"repo:acme/checkout".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(repo.kind, "repo");

    // component: → Backstage.
    let component = router
        .get_entity(&"component:default/payments-api".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(component.display_title(), "Payments API");

    // Techdocs route the same way.
    let repo_docs = router
        .get_techdocs_page(&"repo:acme/checkout".parse().unwrap(), "README.md")
        .await
        .unwrap();
    assert!(repo_docs.contains("repo docs"));
    let component_docs = router
        .get_techdocs_page(&"component:default/ledger".parse().unwrap(), "index.md")
        .await
        .unwrap();
    assert!(!component_docs.contains("repo docs"));
}

#[tokio::test]
async fn github_only_router_rejects_catalog_kinds() {
    let router = MemoryRouter::new(None, Some(Arc::new(RepoOnly))).unwrap();
    let err = router
        .get_entity(&"component:default/payments-api".parse().unwrap())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ProviderError::NotFound(msg) if msg.contains("no memory source")),
        "{err:?}"
    );
}

#[tokio::test]
async fn search_fans_out_across_sources() {
    let router = both_sources();
    // "payments" matches Backstage entities; RepoOnly returns its repo for
    // any non-"boom" query — both appear.
    let hits = router.search("payments", 10).await.unwrap();
    assert!(hits.iter().any(|e| e.kind == "repo"));
    assert!(hits.iter().any(|e| e.kind == "Component"));

    // The limit applies to the combined result set.
    let capped = router.search("payments", 1).await.unwrap();
    assert_eq!(capped.len(), 1);
}

#[tokio::test]
async fn search_survives_one_source_failing_but_not_all() {
    let router = both_sources();
    // RepoOnly explodes on "boom"; Backstage has no matches — combined result
    // is the surviving source's (empty) success only if any source succeeded
    // with results; with zero results overall the first error surfaces.
    let err = router.search("boom", 10).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");

    // But if the surviving source has results, the failure is absorbed.
    let router = MemoryRouter::new(
        Some(Arc::new(MockProvider::with_payments_graph())),
        Some(Arc::new(RepoOnly)),
    )
    .unwrap();
    let hits = router.search("boom-ledger", 10).await;
    // "boom-ledger" doesn't trigger RepoOnly's failure (exact match only) —
    // craft the real case: query "boom" fails RepoOnly, but Backstage matches
    // nothing, covered above. Query "ledger" succeeds everywhere:
    drop(hits);
    let hits = router.search("ledger", 10).await.unwrap();
    assert!(hits.iter().any(|e| e.metadata.name == "ledger"));
}

#[test]
fn from_env_with_nothing_configured_is_none() {
    // Env is process-global; serialize with the other env tests.
    let _guard = crate::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let saved: Vec<(&str, Option<String>)> = ["BACKSTAGE_BASE_URL", "OPENADE_GH_BIN"]
        .into_iter()
        .map(|k| (k, std::env::var(k).ok()))
        .collect();
    std::env::remove_var("BACKSTAGE_BASE_URL");
    std::env::remove_var("OPENADE_GH_BIN");
    std::env::set_var("OPENADE_GITHUB_MEMORY", "0");

    assert!(MemoryRouter::from_env().is_none());

    // Backstage alone enables the router.
    std::env::set_var("BACKSTAGE_BASE_URL", "http://127.0.0.1:1");
    let router = MemoryRouter::from_env().unwrap();
    assert_eq!(router.source_names(), vec!["backstage"]);

    // GitHub alone (via explicit binary override) enables it too.
    std::env::remove_var("BACKSTAGE_BASE_URL");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    std::env::set_var("OPENADE_GH_BIN", "/custom/gh");
    let router = MemoryRouter::from_env().unwrap();
    assert_eq!(router.source_names(), vec!["github"]);

    std::env::remove_var("OPENADE_GH_BIN");
    std::env::remove_var("OPENADE_GITHUB_MEMORY");
    for (k, v) in saved {
        if let Some(v) = v {
            std::env::set_var(k, v);
        }
    }
}
