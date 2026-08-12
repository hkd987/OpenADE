# Signals & the Inbox: outside world in, outcomes out

OpenADE's Inbox turns errors, tickets, and feedback into triaged agent
sessions — and remembers what reality decided about each one. The design
(schema, fingerprinting, dismissal taxonomy, escalation, outcome memory)
is ported from [Merge0](https://github.com/hkd987/Merge0) (MIT — see
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)).

## Where signals go

- **Team mode** (multiplayer configured): post to your self-hosted
  `openade-server` with any member token. The whole team triages one
  queue in their own local apps — the server has no UI and no login.
- **Solo mode** (no server): post to your local daemon
  (`http://127.0.0.1:7433/signals`, no auth — loopback only). Same
  schema, same inbox, zero setup.

```sh
# team: any tool with a member token (Sentry webhook, CI job, script)
curl -X POST http://your-server:7500/signals \
  -H "Authorization: Bearer oadk_…" -H 'content-type: application/json' \
  -d '{
    "source": "sentry",
    "source_ref": "E-42",
    "kind": "exception",
    "severity": "critical",
    "title": "NPE in checkout poller",
    "body": "TypeError: cannot read x of undefined",
    "evidence": [{ "kind": "stack_trace", "label": "sentry trace",
                   "url": "https://sentry.example/e/42" }],
    "join_keys": { "release": "v2.3.0" },
    "affected_count": 128
  }'

# solo: same body, straight at the daemon
curl -X POST http://127.0.0.1:7433/signals -H 'content-type: application/json' -d '{...}'
```

Both endpoints accept a single object or an array, and answer
`{received, inserted, updated, escalated}`.

## The normalized schema

Senders may not invent fields — unknown fields are rejected, so the
contract stays honest. Extensions go through this schema, never through
ad-hoc additions in a sender.

| Field | Required | Meaning |
|---|---|---|
| `source` | ✓ | Where it came from (`"sentry"`, `"ci"`, `"zendesk"`, …) — free-form, non-empty |
| `source_ref` | | Vendor-native object id |
| `kind` | ✓ | `exception` \| `ux_friction` \| `ticket` \| `regression` \| `custom` |
| `severity` | ✓ | `low` \| `medium` \| `high` \| `critical` |
| `title` | ✓ | One line, non-empty — becomes the inbox item title |
| `body` | | Longer description / stack text |
| `evidence[]` | | Deep links: `{kind: replay\|stack_trace\|ticket\|issue\|other, label, url}` |
| `fingerprint` | | Stable dedup key; computed from `(source, source_ref, kind, title)` when absent |
| `join_keys` | | Correlation keys: `release`, `stack_hash`, `account_id`, `url_path` (absent keys omitted, never `null`) |
| `affected_count` | | Users/accounts impacted, when the source knows |
| `raw` | | Original vendor payload, kept verbatim for audit |

## Dedup, recurrence, escalation

The **fingerprint** (`<source>:<hex16 of sha256 over length-prefixed
parts>`) is the identity of a problem. Reposting the same fingerprint
never duplicates an inbox item — it bumps `last_seen`, refreshes
severity/title, and updates the impact.

Dismissing an item records a structured reason in **outcome memory**
(`intended_behavior` / `wont_fix` / `duplicate` / `bad_evidence`) and
snapshots the impact known at decision time. If the same fingerprint
returns with **≥3× the affected count**, the dismissal is considered
stale and the item escalates back into the queue with a note — quiet
inbox, but never quietly wrong.

## Triage → session → outcome

- **Accept & start session** launches a triage session in your chosen
  harness: the signal's evidence, join keys, impact, and the
  fingerprint's full outcome history land in `.openade/inbox-item.md`,
  and the agent is prompted to investigate and fix. The item moves to
  "In progress" with your name on it — teammates see it within seconds.
- **Investigate with agent** does the same without deciding: the item
  stays in the queue.
- When the session finishes, OpenADE checks the task branch's PR fate
  through **your own `gh` CLI** (never a stored credential) and records
  `merged` / `closed` into outcome memory, idempotently. Prior outcomes
  are rendered into every future triage doc and context bundle with age
  annotations — entries older than 90 days are marked `STALE`: they
  inform, they never veto a retry.

## API summary

On `openade-server` (member token) and mirrored on the local daemon
(no auth, loopback):

| Endpoint | What |
|---|---|
| `POST /signals` | Ingest one signal or an array |
| `GET /inbox?status=new\|accepted\|dismissed` | The queue, newest activity first |
| `GET /inbox/{id}` | Item + signals + fingerprint-anchored outcome history |
| `POST /inbox/{id}/accept` | Take the work (actor = your token / `$USER`) |
| `POST /inbox/{id}/dismiss {reason}` | Structured dismissal into outcome memory |
| `POST /inbox/{id}/outcomes {kind, pr_url?}` | Record what reality decided (idempotent) |

Daemon-only:

| Endpoint | What |
|---|---|
| `POST /sessions/from-inbox` | Launch a triage session from an item (`investigate: true` to look without deciding) |
| `POST /sessions/{id}/inbox-outcome` | Check the session branch's PR fate via gh and record it |

## Scope notes

v1 ships the generic webhook only — vendor adapters (Sentry, PostHog,
Zendesk, …) can be built by anything that can POST this schema. There is
no LLM triage gate and never will be one inside OpenADE (no model access
in our code); agentic triage happens through *your* harness via
"Investigate with agent".
