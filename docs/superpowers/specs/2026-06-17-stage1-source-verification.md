# Stage 1 Source-Availability Verification

**Date:** 2026-06-17  
**Machine:** bto-storm  
**Author:** verify-sources subagent  
**Purpose:** Gate check for Plan 2 (real data ingestion from RabbitMQ, Mimir, REST)

---

## 1. RabbitMQ — VERIFIED (LIVE)

### Connection

**Status:** VERIFIED — exchange confirmed live, publishing at ~3.8 msg/s in, ~11.4 msg/s out.

- **AMQP URL env var:** `RABBITMQ_URL` (this is the real name; `RABBITMQ_AMQP_URL` is the legacy alias — `rabbit.ts` prefers `RABBITMQ_URL` first, falls back to `RABBITMQ_AMQP_URL`).
- **Credential source:** OpenBao path `services/claude-events/rabbitmq`, field `url`. Injected at PM2 startup by `scripts/bao-env-wrapper.ts`. Also present in `~/.env` as a fallback.
- **URL form:** `amqp://admin:<REDACTED>@rabbitmq.svc.bto.bar:5672/` (in `$RABBITMQ_URL` on bto-storm). Primary host is `rabbitmq.svc.bto.bar:5672` (on-LAN Caddy-L4 proxy); tailnet fallback is ClusterIP `10.43.30.178:5672` (auto-derived by `buildAmqpUrls()`).
- **Management API:** `https://rabbitmq.bto.bar/api` — reachable, requires HTTP Basic auth.

### Exchange

| Property | Value |
|----------|-------|
| Name | `claude-events` |
| Type | **topic** |
| Durable | true |
| Vhost | `/` |
| Env override | `CLAUDE_EVENTS_EXCHANGE` (default: `claude-events`) |

### Routing Keys

| Routing Key | Description |
|-------------|-------------|
| `hook.<event_lowercase>` | One per hook event type (e.g. `hook.stop`, `hook.pretooluse`, `hook.posttooluse`, `hook.notification`) |
| `transcript.message` | One per JSONL line published from transcript delta |
| `transcript.archived` | Fired when a full transcript JSONL is archived to MinIO/SeaweedFS |

**Wildcard for all hook events:** `hook.#`

### Known Queues (VERIFIED live, all durable, 0 backlog)

| Queue | Routing Key | Consumers |
|-------|-------------|-----------|
| `claude-events-consumer.hook` | `hook.#` | 1 |
| `claude-events-consumer.transcript-message` | `transcript.message` | 1 |
| `claude-events-consumer.transcript-archived` | `transcript.archived` | 1 |
| `bto-coder-hooks` | `hook.#` | (not checked) |
| `bto-coder-transcripts` | `transcript.message` | (not checked) |
| `sessions-central.hook` | `hook.#` | (not checked) |
| `sessions-central.transcript-message` | `transcript.message` | (not checked) |

Orrery should declare its own durable queue (e.g. `orrery.hook`) and bind to `hook.#`.

### Message Envelope — VERIFIED (from source + SQLite samples)

The message body published to `hook.<event>` is a **JSON object** (`HookRelayMessage`):

```json
{
  "id": 30911,
  "hookEvent": "Stop",
  "sessionId": "37c1bda5-b5c5-4920-b471-7ef3e38fbd7c",
  "cwd": "/home/jay/dev/mesh-six",
  "transcriptPath": "/home/jay/.claude/projects/-home-jay-dev-mesh-six/37c1bda5-b5c5-4920-b471-7ef3e38fbd7c.jsonl",
  "createdAt": 1750200000000,
  "lastAssistantMessage": "<last assistant turn text — may be long>",
  "notificationType": null,
  "toolName": "Bash",
  "message": null,
  "rawPayload": "<JSON string: original hook payload>"
}
```

AMQP headers also set per message:
- `session-id` — same as `msg.sessionId`
- `hook-event` — same as `msg.hookEvent`
- `tool-name` — same as `msg.toolName` (null for non-tool events)
- `account` — derived from transcript path (workspace account label)
- `x-tmux-session` — tmux session name if available
- workspace resolution headers (git remote, branch, etc.)

### JSON paths to key fields

| Field | Path in envelope |
|-------|-----------------|
| Session ID | `msg.sessionId` (top-level) OR `JSON.parse(msg.rawPayload).session_id` |
| Host/machine | NOT in envelope directly — must derive from `msg.cwd` or AMQP header `account` |
| Workspace/cwd | `msg.cwd` (top-level) |
| Model | NOT in hook events — model info is not in the hook payload schema |
| Hook event type | `msg.hookEvent` (e.g. "Stop", "PreToolUse", "PostToolUse", "Notification") |
| Tool name | `msg.toolName` (top-level, null for non-tool events) |
| Agent ID | `JSON.parse(msg.rawPayload).agent_id` (in rawPayload string) |

**Note on model:** The hook event payload does not include model name. Model would need to be inferred from transcript content or a separate metadata source.

### Hook Event Types (from SQLite schema + source)

Confirmed in DB: `PreToolUse`, `PostToolUse`, `Stop`, `Notification`  
From source code: also `SessionStart`, `SubagentStop`, `UserPromptSubmit`, and others in the hook framework.

---

## 2. Mimir / PromQL — VERIFIED (BLOCKED for `claude_code_*`)

**Status:** Mimir is reachable at `https://mimir.bto.bar/prometheus/` (no auth required, no tenant header required). However, **`claude_code_*` series do not exist**.

### Datasource

- **Mimir HTTP base:** `https://mimir.bto.bar/prometheus/api/v1/`
- **Grafana MCP:** UNAUTHORIZED (returns HTTP 401 — same as prior sessions)
- **Tenant:** No `X-Scope-OrgID` header required; default anonymous tenant works.

### Metrics found matching `claude.*`

All 18 metrics found are from the **spool-worker** (pipeline telemetry), not from Claude Code itself:

```
claude_events_relay_published_total
claude_events_spool_backlog_segments
claude_events_spool_backlog_segments_ratio
claude_events_spool_batch_size_{bucket,count,sum}
claude_events_spool_event_lag_seconds_{bucket,count,sum}
claude_events_spool_events_inserted_total
claude_events_spool_oldest_segment_age_seconds
claude_events_spool_segments_deleted_total
claude_events_spool_tick_duration_seconds_{bucket,count,sum}
claude_events_spool_ticks_total
claude_events_transcript_bytes_archived_total
claude_events_transcript_messages_published_total
```

### `claude_code_*` query result

`match[]={__name__=~"claude_code.*"}` → **empty result set `[]`**

Claude Code does not export token-usage or session metrics to Mimir. The `claude_code_token_usage_tokens_total` series assumed in the spec does not exist.

### Session-ID label

Label `session_id` does **not** appear in Mimir labels. Label `session.id` also absent. The spool-worker metrics carry labels like `status`, `hook_event` — none are session-scoped.

### Sample query result

PromQL `sum by (session_id, model)(rate(claude_code_token_usage_tokens_total[1m]))` → would return empty (metric doesn't exist).

---

## 3. REST — VERIFIED (limited, no live session feed)

**Base URL:** `https://claude-events.bto.bar`

### Endpoints probed

| Path | Method | Status | Notes |
|------|--------|--------|-------|
| `/` | GET | 404 | No root handler |
| `/health` | GET | 404 | No health endpoint |
| `/api` | GET | 404 | |
| `/search` | GET | 404 | Not a direct path |
| `/search/sessions` | GET | 404 | |
| `/search/messages` | GET | 404 | |
| `/sessions` | GET | 400 | `{"error":"workspaceId required"}` — endpoint exists |
| `/sessions?workspaceId=<uuid>` | GET | 200 | Returns `{"sessions":[],"hasMore":false,"nextCursor":null}` |
| `/sessions?workspaceId=<invalid>` | GET | 500 | `{"error":"query failed"}` — UUID validation |
| `/mcp` | GET | 406 | MCP SSE endpoint (requires `Accept: text/event-stream`) |
| `/api/sessions` | GET | 404 | |

### Verdict

The `/sessions` endpoint requires a `workspaceId` (UUID) parameter and returns a paginated list of sessions for that workspace. It is **not** a live "currently active agents" feed — it queries historical session state from the backing store (likely the Postgres instance behind the `claude-events` MCP server). With a valid workspaceId it returns 200 with session records; with an unknown workspaceId it returns an empty list.

**REST verdict: reconcile/no-op only.** There is no endpoint that gives a real-time list of active sessions with model/state data. REST is useful for workspace-scoped session history lookups, not for driving live visualization. The `/mcp` endpoint is the correct surface for semantic search but requires SSE (not suitable for simple polling).

---

## 4. Impact on Plan 2

### Source status

| Source | Status | Notes |
|--------|--------|-------|
| **RabbitMQ** | **GO** | Exchange live, publishing actively, routing keys confirmed, envelope verified, creds in `$RABBITMQ_URL` |
| **Mimir** | **BLOCKED** | `claude_code_*` metrics do not exist; no session-id label; spool metrics only |
| **REST** | **SCOPED DOWN** | `/sessions` works but is workspace-scoped historical, not a live feed; no active-session endpoint |

### Schema surprises vs. spec assumptions

1. **Model is absent from hook events.** The spec (§5) assumed model could be derived from the hook payload or Mimir. It cannot from either source. Model would require parsing transcript JSONL lines (`transcript.message` on the exchange) for `model` field in assistant turns.

2. **No `claude_code_token_usage_tokens_total` in Mimir.** The spec's §5 Mimir path (token counts, session labels) is a dead end. Mimir carries only pipeline health metrics.

3. **Host/machine not in envelope.** Session-to-machine mapping must be inferred from `msg.cwd` path patterns or the `account` AMQP header (derived from transcript path).

4. **Agent IDs are in `rawPayload`.** The `agent_id` and `agent_type` fields (needed to distinguish subagents from orchestrators) are only in the `rawPayload` JSON string, not promoted to the top-level `HookRelayMessage`. Orrery's consumer must parse `JSON.parse(msg.rawPayload)` to get them.

5. **RabbitMQ is the definitive real-time source.** All three alternative sources (Mimir, REST) are either dead or non-real-time. Plan 2 should be RabbitMQ-only, with Mimir and REST marked as no-op stubs.

### Recommended Plan 2 adjustments

- Drop Mimir ingestion path entirely (no data).
- Scope REST to an optional workspace-id lookup (no-op if workspaceId unknown).
- Add `transcript.message` subscription alongside `hook.#` if model extraction is needed (parse transcript JSONL for `model` field in assistant turns).
- Parse `JSON.parse(msg.rawPayload)` to extract `agent_id`/`agent_type` for multi-agent topology.
