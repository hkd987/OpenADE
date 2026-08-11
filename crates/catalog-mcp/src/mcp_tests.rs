use super::*;
use crate::testutil::MockProvider;

fn server() -> McpServer<MockProvider> {
    McpServer::new(MockProvider::with_payments_graph())
}

async fn call(server: &McpServer<MockProvider>, msg: Value) -> Value {
    server
        .handle_message(&msg.to_string())
        .await
        .expect("expected a response")
}

#[tokio::test]
async fn initialize_handshake() {
    let s = server();
    let res = call(
        &s,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }}),
    )
    .await;
    assert_eq!(res["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(res["result"]["serverInfo"]["name"], SERVER_NAME);
    assert!(res["result"]["capabilities"]["tools"].is_object());

    // The initialized notification gets no response.
    assert!(s
        .handle_message(
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string()
        )
        .await
        .is_none());
}

#[tokio::test]
async fn tools_list_exposes_the_six_prd_tools() {
    let res = call(
        &server(),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    let tools = res["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec![
            "get_entity",
            "get_owner",
            "get_dependencies",
            "get_apis_for_entity",
            "search_catalog",
            "get_techdocs_page"
        ]
    );
    for t in tools {
        assert!(
            t["inputSchema"]["type"] == "object",
            "tool {t} lacks an input schema"
        );
        assert!(t["description"].as_str().unwrap().len() > 10);
    }
}

async fn call_tool(s: &McpServer<MockProvider>, name: &str, args: Value) -> (Value, bool) {
    let res = call(
        s,
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": name, "arguments": args
        }}),
    )
    .await;
    let is_error = res["result"]["isError"].as_bool().unwrap();
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    let value = serde_json::from_str(text).unwrap_or(Value::String(text.to_string()));
    (value, is_error)
}

#[tokio::test]
async fn get_entity_round_trips() {
    let (value, is_error) = call_tool(
        &server(),
        "get_entity",
        json!({"entity_ref": "component:default/payments-api"}),
    )
    .await;
    assert!(!is_error);
    assert_eq!(value["metadata"]["title"], "Payments API");
}

#[tokio::test]
async fn get_owner_resolves_the_owning_group() {
    let (value, is_error) = call_tool(
        &server(),
        "get_owner",
        json!({"entity_ref": "component:default/payments-api"}),
    )
    .await;
    assert!(!is_error);
    assert_eq!(value["owned_by"][0], "group:default/payments-team");
    assert_eq!(value["owners"][0]["metadata"]["title"], "Payments Team");
}

#[tokio::test]
async fn get_dependencies_and_apis_walk_relations() {
    let s = server();
    let (deps, _) = call_tool(
        &s,
        "get_dependencies",
        json!({"entity_ref": "component:default/payments-api"}),
    )
    .await;
    assert_eq!(
        deps["dependencies"][0]["target_ref"],
        "component:default/ledger"
    );
    assert_eq!(
        deps["dependencies"][0]["entity"]["metadata"]["name"],
        "ledger"
    );

    let (apis, _) = call_tool(
        &s,
        "get_apis_for_entity",
        json!({"entity_ref": "component:default/payments-api"}),
    )
    .await;
    assert_eq!(apis["apis"][0]["relation"], "providesApi");
    // payments-v2 is not resolvable in the mock catalog.
    assert!(apis["apis"][0]["entity"].is_null());
}

#[tokio::test]
async fn search_and_techdocs() {
    let s = server();
    let (hits, _) = call_tool(&s, "search_catalog", json!({"query": "ledger"})).await;
    assert_eq!(hits["items"].as_array().unwrap().len(), 1);

    let (page, _) = call_tool(
        &s,
        "get_techdocs_page",
        json!({"entity_ref": "component:default/payments-api", "path": "index.md"}),
    )
    .await;
    assert!(page["page"].as_str().unwrap().contains("payments-api"));
}

#[tokio::test]
async fn errors_are_tool_results_not_protocol_errors() {
    let s = server();
    let (value, is_error) = call_tool(
        &s,
        "get_entity",
        json!({"entity_ref": "component:default/ghost"}),
    )
    .await;
    assert!(is_error);
    assert!(value.as_str().unwrap().contains("not found"));

    let (value, is_error) = call_tool(&s, "get_entity", json!({})).await;
    assert!(is_error);
    assert!(value.as_str().unwrap().contains("entity_ref"));

    let (_, is_error) = call_tool(&s, "no_such_tool", json!({})).await;
    assert!(is_error);
}

#[tokio::test]
async fn ping_and_malformed_json_are_handled() {
    let s = server();
    let res = call(&s, json!({"jsonrpc": "2.0", "id": 7, "method": "ping"})).await;
    assert_eq!(res["result"], json!({}));

    let res = s.handle_message("this is not json").await.unwrap();
    assert_eq!(res["error"]["code"], -32700);
    assert!(res["id"].is_null());
}

#[tokio::test]
async fn unknown_method_is_a_json_rpc_error() {
    let res = call(
        &server(),
        json!({"jsonrpc": "2.0", "id": 9, "method": "resources/list"}),
    )
    .await;
    assert_eq!(res["error"]["code"], -32601);
}

#[tokio::test]
async fn serve_speaks_newline_delimited_json_over_streams() {
    let s = server();
    // Includes a blank line, which the server must skip.
    let input = format!(
        "{}\n{}\n\n{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    );
    let mut output: Vec<u8> = Vec::new();
    s.serve(input.as_bytes(), &mut output).await.unwrap();

    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "notification must not produce a response");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["id"], 2);
    assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 6);
}
