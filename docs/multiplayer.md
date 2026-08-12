# Multiplayer: self-hosted team workspaces

OpenADE's multiplayer mode is a **self-hostable workspace server**
(`openade-server`, its own binary in this repo and in every release
tarball). Your team runs one instance; each member's daemon connects to
it. Nothing about a session leaves a member's machine until they press
**Share** — then the session's harness-neutral record (title, harness,
entity, branch, summary, knowledge-artifact markdown, and the transcript
events) is uploaded to a team workspace where everyone can browse it,
learn from it, and **pick it up**.

Picking up is the headline: any shared session can be resumed by anyone,
anytime, in *any* harness. A session shared from Claude Code can be picked
up in Copilot CLI. That works because the server only ever stores
OpenADE's harness-neutral representation — never a vendor CLI's native
session file — and the picking-up daemon renders it for the chosen
harness: context through that harness's own rules file (`CLAUDE.md` /
`AGENTS.md` / `GEMINI.md`), the conversation history as a
`.openade/pickup.md` takeover document (summary, full record, the original
prompts), and the resume instruction through the adapter's own prompt
convention. The new session runs in the picker's own clone, in a fresh
worktree (or the main checkout), under their own credentials.

Shared history also feeds forward automatically: when a workspace is
configured, launching a session on an entity pulls the workspace's most
recent shared sessions for that entity into the context bundle — what any
teammate shared, everyone's next session knows.

> Prefer not to run a server? A hosted version is planned. Self-hosting is
> and will remain fully supported and open source.

## Run the server

```sh
OPENADE_SERVER_ADMIN_TOKEN=$(openssl rand -hex 24) \
openade-server
# listens on 0.0.0.0:7500, state in ~/.openade-server/server.db (SQLite)
```

Environment knobs:

| Variable | Default | What |
|---|---|---|
| `OPENADE_SERVER_ADMIN_TOKEN` | *(empty — admin API disabled)* | Bootstrap admin bearer token. Required to mint member tokens; keep it secret. An empty value never authenticates. |
| `OPENADE_SERVER_PORT` | `7500` | Listen port |
| `OPENADE_SERVER_BIND` | `0.0.0.0` | Bind address |
| `OPENADE_SERVER_DATA_DIR` | `~/.openade-server` | Directory for `server.db` |

The server speaks plain HTTP; put it behind your usual TLS-terminating
reverse proxy for anything beyond a trusted network. All state is one
SQLite file — back it up by copying it.

## Set up the team (admin, once)

```sh
S=http://your-server:7500
A="Authorization: Bearer $OPENADE_SERVER_ADMIN_TOKEN"

# One workspace for the team's shared history
curl -s -X POST $S/workspaces -H "$A" -H 'content-type: application/json' \
  -d '{"title":"Acme Eng","description":"Shared agent sessions"}'
# → {"id":1,...}

# One member token per person (revocable)
curl -s -X POST $S/tokens -H "$A" -H 'content-type: application/json' \
  -d '{"name":"casey"}'
# → {"id":1,"name":"casey","token":"oadk_…"}   ← give this to casey
```

Revoke a leaver's token with `POST /tokens/{id}/revoke`; list them with
`GET /tokens`. Member management is deliberately API-only in v1.

## Connect each member (30 seconds)

In the desktop app: **⚙ Settings → Multiplayer** — paste the server URL,
your member token, and the workspace id. Applies immediately, no restart.
The token is stored by the local daemon and never reaches the browser;
the UI's Team view is proxied through the daemon.

Environment variables work too (and always win over saved settings):
`OPENADE_SERVER_URL`, `OPENADE_SERVER_TOKEN`, `OPENADE_SERVER_WORKSPACE`.

## Use it

- **Share** — open a session, press **Share**. Uploads are manual and
  per-session: nothing is shared implicitly, ever.
- **Browse** — the **Team** view lists the workspace's shared sessions
  (who shared, which harness, which entity, when, one-line summary); click
  one for the read-only artifact + transcript.
- **Pick up** — from a shared session, choose any harness and a local
  clone of the repo, press **Pick up**. A new local session starts with
  the shared context rendered for that harness and the agent instructed to
  read `.openade/pickup.md` and continue.

Degradation is graceful everywhere: an unreachable server or bad token
turns into a clear banner (naming the fix), never a broken daemon — and
context bundles simply skip workspace history while it's unavailable.

## API

All endpoints except `/health` require `Authorization: Bearer <token>`.
Admin = the `OPENADE_SERVER_ADMIN_TOKEN` value; members = minted `oadk_…`
tokens.

| Endpoint | Auth | What |
|---|---|---|
| `GET /health` | — | Liveness + version |
| `GET /whoami` | member | Token introspection (`org`, `member`) |
| `POST /tokens` `{name}` | admin | Mint a member token |
| `GET /tokens` | admin | List tokens (no secrets) |
| `POST /tokens/{id}/revoke` | admin | Revoke a token |
| `POST /workspaces` `{title, description}` | member | Create a workspace |
| `GET /workspaces` | member | List workspaces |
| `GET /workspaces/{id}` | member | One workspace |
| `POST /workspaces/{id}/sessions` | member | Upload a session record (`title`, `harness`, `entity_ref?`, `branch?`, `summary`, `markdown`, `events`) |
| `GET /workspaces/{id}/sessions[?entity=ref]` | member | Shared sessions, newest first, optionally entity-filtered |
| `GET /workspaces/{id}/sessions/{sid}` | member | Full record (meta + markdown + events) |

Every row is org-scoped internally (self-host runs a single default org),
so the same binary can serve a future multi-tenant hosted deployment
without a schema change.

## Scope (v1)

Matches what the workspace model is for: asynchronous session sharing and
pickup. **Not** included, deliberately: live session observation or
co-driving, presence, and a member-management UI. See
[CONTRIBUTING](../CONTRIBUTING.md) for the contribution boundaries.
