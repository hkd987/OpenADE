# catalog-mcp — tool schema draft

**Status:** Draft, matches the implementation in `crates/catalog-mcp`.
**Transport:** MCP stdio (newline-delimited JSON-RPC 2.0), protocol revision
`2025-06-18`. Streamable HTTP is planned for the shared/remote scenarios (P1+).
**Server identity:** `catalog-mcp`, capability `tools` only (no resources or
prompts in v0).

## Design rules

1. **Backend-neutral surface.** Tool names, argument shapes, and error texts
   never mention Backstage. The same six tools must be implementable by a
   Port/Cortex/Git-YAML provider (`CatalogProvider` trait) without breaking
   agent-facing behavior (PRD G4).
2. **Entity refs are strings** in `kind:namespace/name` form; `namespace`
   defaults to `default`. This matches Backstage's own compact ref format and
   costs nothing for other backends to adopt.
3. **Failures are tool results, not protocol errors.** A missing entity or bad
   argument returns `isError: true` with a readable message, so the agent can
   self-correct (retry with a searched-for ref) instead of the client
   surfacing a protocol fault. Protocol errors are reserved for malformed
   JSON-RPC and unknown methods.
4. **Read-only.** The write path (knowledge artifacts) goes through Git PRs,
   not through this server (PRD §7.3).

## Tools

All results are returned as a single `text` content block containing
pretty-printed JSON (shapes below).

### `get_entity`
Fetch one entity: metadata, spec, relations.

Input:
```json
{ "entity_ref": "component:default/payments-api" }
```
Result: the entity object (Backstage-shaped envelope: `apiVersion`, `kind`,
`metadata{name,namespace,title,description,tags,annotations,links}`, `spec`,
`relations[{type,targetRef}]`).

### `get_owner`
Resolve ownership: `ownedBy` relation targets, with owner entities fetched
when resolvable (unresolvable refs are kept as bare strings — partial
answers beat failures).

Input: `{ "entity_ref": ... }`
Result:
```json
{ "owned_by": ["group:default/payments-team"], "owners": [ { ...entity } ] }
```

### `get_dependencies`
`dependsOn` edges from the relations graph.

Input: `{ "entity_ref": ... }`
Result:
```json
{ "dependencies": [ { "relation": "dependsOn", "target_ref": "component:default/ledger", "entity": { ... } | null } ] }
```

### `get_apis_for_entity`
`providesApi` / `consumesApi` edges, same shape as dependencies under an
`apis` key.

### `search_catalog`
Full-text search.

Input:
```json
{ "query": "payments", "limit": 10 }
```
`limit` is clamped to `[1, 50]`, default 10. Result: `{ "items": [ ...entities ] }`.

### `get_techdocs_page`
Fetch one docs page (ADR, runbook, index) for an entity.

Input:
```json
{ "entity_ref": "component:default/payments-api", "path": "adrs/adr-001.md" }
```
Result: `{ "page": "<markdown or html as published>" }`.

## Auth & configuration

- v0: `BACKSTAGE_BASE_URL` (required) + `BACKSTAGE_TOKEN` (optional static
  service-to-service bearer token) from the environment; the daemon injects
  these when spawning the server per session. OS-keychain storage for the
  token is a Phase 2 item (PRD §7.5); env-var passing is the transport either
  way.
- OAuth device/browser flows: documented-not-implemented for v0; orgs that
  disallow static tokens front Backstage with a token-issuing proxy.

## Implementation notes & open questions

- **Thin server vs. SDK.** The stdio server is ~150 lines of hand-rolled
  JSON-RPC (`src/mcp.rs`) with the wire behavior pinned by our own tests.
  Rationale: the required surface (initialize / tools/list / tools/call /
  ping) is tiny, and avoiding an SDK keeps the context layer dependency-light
  while MCP SDKs are still moving fast. **Revisit trigger:** the moment we
  need resources, prompts, notifications, or streamable HTTP, migrate to the
  official Rust SDK instead of growing this by hand.
- **PRD Q2 (Backstage first-party MCP backend).** Building on Backstage's own
  MCP/actions backend would tie tool availability to the org's Backstage
  release cadence and plugin rollout — precisely the coupling `CatalogProvider`
  exists to avoid. Current position: keep our thin server as the default
  path (works against any Backstage new enough to serve the REST APIs), and
  add a "passthrough provider" targeting the first-party MCP backend as a
  second `CatalogProvider` implementation once it stabilizes. Decide finally
  during Phase 2 with S0.6 data.
- **Search backend.** v0 uses catalog `by-query` full-text filtering rather
  than the Search API plugin: no extra backend plugins required, full
  entities returned. Revisit if orgs want ranked TechDocs-content search.
- **Context bundles** (`src/bundle.rs`) are assembled by the daemon at session
  launch — bundle injection is *not* an MCP tool in v0 (PRD Q3: fixed
  injection vs. tool-driven retrieval; testing per harness in the Phase 0
  spike). A `get_context_bundle` tool is the likely addition if Q3 lands on
  tool-driven.
