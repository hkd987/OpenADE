//! A thin MCP server over stdio.
//!
//! MCP's stdio transport is newline-delimited JSON-RPC 2.0. The surface this
//! server needs — `initialize`, `tools/list`, `tools/call`, `ping` — is small
//! enough that we implement it directly rather than depending on an SDK.
//! That keeps the context layer dependency-light and pins the wire behavior
//! in our own tests; if the tool surface grows (resources, prompts,
//! streamable HTTP), migrating to the official Rust SDK is the expected move
//! (see docs/catalog-mcp-tools.md).

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::provider::{CatalogProvider, EntityRef, ProviderError};

/// MCP protocol revision this server targets.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
/// Server name reported in `initialize`.
pub const SERVER_NAME: &str = "catalog-mcp";

/// The MCP server: dispatches tool calls to a [`CatalogProvider`].
pub struct McpServer<P: CatalogProvider> {
    provider: P,
}

/// The six catalog tools (PRD R5), as MCP tool definitions.
pub fn tool_definitions() -> Vec<Value> {
    let entity_ref_prop = json!({
        "type": "string",
        "description": "Entity reference, `kind:namespace/name` (namespace defaults to `default`), e.g. `component:default/payments-api`."
    });
    let entity_ref_schema = json!({
        "type": "object",
        "properties": { "entity_ref": entity_ref_prop },
        "required": ["entity_ref"]
    });
    vec![
        json!({
            "name": "get_entity",
            "description": "Fetch a catalog entity (metadata, spec, relations) by reference.",
            "inputSchema": entity_ref_schema
        }),
        json!({
            "name": "get_owner",
            "description": "Who owns an entity: resolves `ownedBy` relations to owner entities (team, contact links).",
            "inputSchema": entity_ref_schema
        }),
        json!({
            "name": "get_dependencies",
            "description": "What an entity depends on: `dependsOn` edges from the catalog relations graph, with target entities resolved when possible.",
            "inputSchema": entity_ref_schema
        }),
        json!({
            "name": "get_apis_for_entity",
            "description": "API surfaces an entity provides or consumes (`providesApi` / `consumesApi` relations).",
            "inputSchema": entity_ref_schema
        }),
        json!({
            "name": "search_catalog",
            "description": "Full-text search over the software catalog.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms." },
                    "limit": { "type": "integer", "description": "Max results (default 10, max 50)." }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "get_techdocs_page",
            "description": "Fetch a TechDocs page for an entity (e.g. an ADR or runbook), by page path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "entity_ref": entity_ref_prop,
                    "path": { "type": "string", "description": "Page path inside the entity's docs site, e.g. `index.html` or `adrs/adr-001.md`." }
                },
                "required": ["entity_ref", "path"]
            }
        }),
    ]
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Tool output: MCP `tools/call` result with a single text content block.
fn tool_text_result(id: Value, text: String, is_error: bool) -> Value {
    rpc_result(
        id,
        json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
    )
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing required string argument {key:?}"))
}

fn parse_ref(args: &Value) -> Result<EntityRef, String> {
    arg_str(args, "entity_ref")?
        .parse()
        .map_err(|e: crate::provider::InvalidEntityRef| e.to_string())
}

impl<P: CatalogProvider> McpServer<P> {
    pub fn new(provider: P) -> Self {
        McpServer { provider }
    }

    async fn call_tool(&self, name: &str, args: &Value) -> Result<Value, ProviderError> {
        match name {
            "get_entity" => {
                let entity_ref = parse_ref(args).map_err(ProviderError::Transport)?;
                let entity = self.provider.get_entity(&entity_ref).await?;
                Ok(serde_json::to_value(entity).unwrap_or_default())
            }
            "get_owner" => {
                let entity_ref = parse_ref(args).map_err(ProviderError::Transport)?;
                let entity = self.provider.get_entity(&entity_ref).await?;
                let owners = self.provider.owners_of(&entity).await?;
                Ok(json!({
                    "owned_by": entity.relation_targets("ownedBy"),
                    "owners": owners,
                }))
            }
            "get_dependencies" => {
                let entity_ref = parse_ref(args).map_err(ProviderError::Transport)?;
                let entity = self.provider.get_entity(&entity_ref).await?;
                let deps = self.provider.dependencies_of(&entity).await?;
                Ok(json!({
                    "dependencies": deps
                        .into_iter()
                        .map(|(relation, target_ref, resolved)| json!({
                            "relation": relation,
                            "target_ref": target_ref,
                            "entity": resolved,
                        }))
                        .collect::<Vec<_>>()
                }))
            }
            "get_apis_for_entity" => {
                let entity_ref = parse_ref(args).map_err(ProviderError::Transport)?;
                let entity = self.provider.get_entity(&entity_ref).await?;
                let apis = self.provider.apis_of(&entity).await?;
                Ok(json!({
                    "apis": apis
                        .into_iter()
                        .map(|(relation, target_ref, resolved)| json!({
                            "relation": relation,
                            "target_ref": target_ref,
                            "entity": resolved,
                        }))
                        .collect::<Vec<_>>()
                }))
            }
            "search_catalog" => {
                let query = arg_str(args, "query").map_err(ProviderError::Transport)?;
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|n| n.clamp(1, 50) as usize)
                    .unwrap_or(10);
                let items = self.provider.search(query, limit).await?;
                Ok(json!({ "items": items }))
            }
            "get_techdocs_page" => {
                let entity_ref = parse_ref(args).map_err(ProviderError::Transport)?;
                let path = arg_str(args, "path").map_err(ProviderError::Transport)?;
                let page = self.provider.get_techdocs_page(&entity_ref, path).await?;
                Ok(json!({ "page": page }))
            }
            other => Err(ProviderError::Transport(format!("unknown tool: {other}"))),
        }
    }

    /// Handle one JSON-RPC message; returns the response (None for
    /// notifications).
    pub async fn handle_message(&self, raw: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(e) => {
                return Some(rpc_error(Value::Null, -32700, format!("parse error: {e}")));
            }
        };
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or_default();

        // Notifications get no response.
        let id = id?;

        match method {
            "initialize" => {
                // Echo the client's protocol version if it sent one we can
                // work with; otherwise offer ours.
                let client_version = msg
                    .pointer("/params/protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or(PROTOCOL_VERSION);
                Some(rpc_result(
                    id,
                    json!({
                        "protocolVersion": client_version,
                        "capabilities": { "tools": {} },
                        "serverInfo": {
                            "name": SERVER_NAME,
                            "version": env!("CARGO_PKG_VERSION"),
                        }
                    }),
                ))
            }
            "ping" => Some(rpc_result(id, json!({}))),
            "tools/list" => Some(rpc_result(id, json!({ "tools": tool_definitions() }))),
            "tools/call" => {
                let name = msg
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let empty = json!({});
                let args = msg.pointer("/params/arguments").unwrap_or(&empty);
                match self.call_tool(name, args).await {
                    Ok(value) => Some(tool_text_result(
                        id,
                        serde_json::to_string_pretty(&value).unwrap_or_default(),
                        false,
                    )),
                    // Tool-level failures (entity not found, bad args) are
                    // reported as tool results with isError, per MCP.
                    Err(err) => Some(tool_text_result(id, err.to_string(), true)),
                }
            }
            other => Some(rpc_error(id, -32601, format!("method not found: {other}"))),
        }
    }

    /// Serve newline-delimited JSON-RPC until EOF.
    pub async fn serve<R, W>(&self, reader: R, mut writer: W) -> std::io::Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle_message(&line).await {
                let mut out = response.to_string();
                out.push('\n');
                writer.write_all(out.as_bytes()).await?;
                writer.flush().await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
