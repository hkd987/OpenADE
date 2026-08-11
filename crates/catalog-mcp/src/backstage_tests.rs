use super::*;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn payments_entity_json() -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "backstage.io/v1alpha1",
        "kind": "Component",
        "metadata": {
            "name": "payments-api",
            "namespace": "default",
            "title": "Payments API",
            "description": "Handles payments."
        },
        "spec": {"type": "service", "lifecycle": "production"},
        "relations": [
            {"type": "ownedBy", "targetRef": "group:default/payments-team"},
            {"type": "dependsOn", "targetRef": "component:default/ledger"},
            {"type": "providesApi", "targetRef": "api:default/payments-v2"}
        ]
    })
}

async fn provider(server: &MockServer, token: Option<&str>) -> BackstageProvider {
    BackstageProvider::new(BackstageConfig {
        base_url: server.uri(),
        token: token.map(str::to_string),
    })
}

#[tokio::test]
async fn fetches_entity_by_name_with_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/catalog/entities/by-name/component/default/payments-api",
        ))
        .and(header("authorization", "Bearer sekrit"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payments_entity_json()))
        .mount(&server)
        .await;

    let p = provider(&server, Some("sekrit")).await;
    let entity = p
        .get_entity(&"component:default/payments-api".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(entity.display_title(), "Payments API");
    assert_eq!(
        entity.relation_targets("dependsOn"),
        vec!["component:default/ledger"]
    );
}

#[tokio::test]
async fn missing_entity_maps_to_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let err = p
        .get_entity(&"component:default/ghost".parse().unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)), "{err:?}");
}

#[tokio::test]
async fn upstream_errors_carry_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let err = p
        .get_entity(&"component:default/x".parse().unwrap())
        .await
        .unwrap_err();
    match err {
        ProviderError::Upstream { status, message } => {
            assert_eq!(status, 500);
            assert_eq!(message, "boom");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn search_uses_full_text_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/catalog/entities/by-query"))
        .and(query_param("fullTextFilter", "payments"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": [payments_entity_json()]
        })))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let hits = p.search("payments", 5).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].metadata.name, "payments-api");
}

#[test]
fn config_from_env_requires_base_url() {
    // Env-var access is process-global; serialize with the other env tests.
    let _guard = crate::testutil::ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("BACKSTAGE_BASE_URL");
    std::env::remove_var("BACKSTAGE_TOKEN");
    assert!(BackstageConfig::from_env().is_err());

    std::env::set_var("BACKSTAGE_BASE_URL", "https://backstage.example.com");
    std::env::set_var("BACKSTAGE_TOKEN", "");
    let config = BackstageConfig::from_env().unwrap();
    assert_eq!(config.base_url, "https://backstage.example.com");
    assert!(config.token.is_none(), "empty token treated as unset");

    std::env::set_var("BACKSTAGE_TOKEN", "sekrit");
    let config = BackstageConfig::from_env().unwrap();
    assert_eq!(config.token.as_deref(), Some("sekrit"));
    std::env::remove_var("BACKSTAGE_BASE_URL");
    std::env::remove_var("BACKSTAGE_TOKEN");
}

#[tokio::test]
async fn techdocs_404_maps_to_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let err = p
        .get_techdocs_page(&"component:default/x".parse().unwrap(), "missing.md")
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound(_)));
}

#[tokio::test]
async fn search_upstream_error_carries_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503).set_body_string("down"))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let err = p.search("x", 5).await.unwrap_err();
    assert!(matches!(err, ProviderError::Upstream { status: 503, .. }));
}

#[tokio::test]
async fn fetches_techdocs_pages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/techdocs/static/docs/default/component/payments-api/adr-007.md",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("# ADR-007\nUse idempotency keys."),
        )
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let page = p
        .get_techdocs_page(
            &"component:default/payments-api".parse().unwrap(),
            "/adr-007.md",
        )
        .await
        .unwrap();
    assert!(page.contains("idempotency keys"));
}

#[tokio::test]
async fn default_trait_helpers_walk_relations() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(
            "/api/catalog/entities/by-name/component/default/payments-api",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(payments_entity_json()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(
            "/api/catalog/entities/by-name/group/default/payments-team",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apiVersion": "backstage.io/v1alpha1",
            "kind": "Group",
            "metadata": {"name": "payments-team", "title": "Payments Team"},
            "spec": {"type": "team"}
        })))
        .mount(&server)
        .await;
    // ledger + payments-v2 unresolvable → skipped/unresolved, not fatal.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let p = provider(&server, None).await;
    let entity = p
        .get_entity(&"component:default/payments-api".parse().unwrap())
        .await
        .unwrap();

    let owners = p.owners_of(&entity).await.unwrap();
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].display_title(), "Payments Team");

    let deps = p.dependencies_of(&entity).await.unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].1, "component:default/ledger");
    assert!(
        deps[0].2.is_none(),
        "unresolvable dep is kept as a bare ref"
    );

    let apis = p.apis_of(&entity).await.unwrap();
    assert_eq!(apis.len(), 1);
    assert_eq!(apis[0].0, "providesApi");
}

#[tokio::test]
async fn invalid_json_bodies_are_transport_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("this is not json"))
        .mount(&server)
        .await;
    let p = provider(&server, None).await;
    let err = p
        .get_entity(&"component:default/x".parse().unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
    let err = p.search("x", 5).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
}

#[tokio::test]
async fn unreachable_backstage_is_a_transport_error() {
    // Nothing listens on port 1.
    let p = BackstageProvider::new(BackstageConfig {
        base_url: "http://127.0.0.1:1".into(),
        token: None,
    });
    let err = p.search("x", 5).await.unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
}

#[tokio::test]
async fn truncated_response_bodies_are_transport_errors() {
    use tokio::io::AsyncWriteExt;
    // A raw socket that promises 100 bytes and closes after 5: header
    // parsing succeeds, reading the body fails mid-stream.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            use tokio::io::AsyncReadExt;
            let _ = sock.read(&mut buf).await;
            let _ = sock
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\n\r\nshort")
                .await;
            drop(sock);
        }
    });
    let p = BackstageProvider::new(BackstageConfig {
        base_url: format!("http://{addr}"),
        token: None,
    });
    let err = p
        .get_techdocs_page(&"component:default/x".parse().unwrap(), "page.md")
        .await
        .unwrap_err();
    assert!(matches!(err, ProviderError::Transport(_)), "{err:?}");
}
