# Stage 1 — Plan 2: Live RabbitMQ ingestion (design spec)

- **Status:** approved design, pre-implementation
- **Date:** 2026-06-18
- **Builds on:** Plan 1 (the ingestion foundation — `src/ingest/` tokio seam, reducer,
  dynamic nuclei; branch `stage1-ingestion-foundation`), the Stage 1 design spec
  (`2026-06-17-stage1-ingestion-design.md`, esp. §5.1), and the §9
  source-availability verification (`2026-06-17-stage1-source-verification.md`).
- **Next step:** implementation plan via the writing-plans skill.

## 1. Purpose

Plan 1 built the ingestion architecture and proved it on synthetic data. Plan 2
attaches the **first real, live source**: the homelab `claude-events` RabbitMQ
exchange. After Plan 2, running `orrery` visualizes **real Claude Code sessions
across the homelab** — each session a nucleus that spawns on start, pulses on
activity, colors by model, and fades on idle/stop — with the synthetic generator
retained as an explicit `--synthetic` demo/dev mode.

The new source tasks plug into the **existing** `IngestHandle.tx` mpsc channel.
The reducer, the `triple_buffer` seam, and the entire render side are **unchanged**
— this is the extension point Plan 1 was designed around.

## 2. Scope decisions (agreed)

1. **Live by default.** Bare `orrery` connects to RabbitMQ and shows real
   sessions; `--synthetic` forces the synthetic field. (Matches the spec §2.3
   product intent.)
2. **Two live signals, two independent consumers** (Approach A):
   - `hook.#` → identity, liveness, pulses, host, workspace (the backbone).
   - `transcript.message` → the assistant **model**, so live nuclei get correct
     per-model colors (model is absent from hook events — §9 finding).
3. **Mimir dropped; REST no-op.** §9 proved `claude_code_*` metrics do not exist
   and the REST surface has no live active-session feed. Neither is built.
4. **Show every homelab session** (spec §2.2) — no exclude-filter. This includes
   automated agents (slack bot, systems-engineer, …) and orrery's own dev
   session. One nucleus per `session_id`.
5. **Deferred to a later plan** (NOT in Plan 2): per-source health overlay,
   all-sources-quiet → synthetic auto-fallback, `max_agents` cap *enforcement*
   (still plumbed; real traffic is well under the default 64), `agent_id`/subagent
   topology, Mimir, REST.

## 3. Architecture

```
            ┌───────────── tokio runtime (Plan 1, unchanged) ─────────────┐
 RabbitMQ ──│ rabbitmq_task  (queue orrery.hook ← hook.#) ───┐            │
 (claude-   │ transcript_task(queue orrery.transcript ←      ├─▶ mpsc ─▶ reducer ─┐
  events)   │                 transcript.message) ───────────┘   (AgentUpdate)   │
 [--synthetic] synth_task (Plan 1) ─────────────────────────┘                    │
            └─────────────────────────────────────────────────────────────────┼─┘
                                                                                ▼
                                              triple_buffer  →  Bevy render (UNCHANGED)
```

New module tree: `src/ingest/sources/` with `mod.rs`, `rabbitmq.rs`,
`transcript.rs`. `spawn_ingest` gains source toggles and spawns the live tasks
(default on) alongside (or instead of) the synthetic task.

Each source task: connect → consume → parse+normalize into `AgentUpdate` →
`tx.send(..).await`. A source task **never** touches Bevy world state and never
writes the `triple_buffer` directly (the reducer is the sole writer). On error it
logs and self-restarts with capped backoff; it never panics.

## 4. RabbitMQ hook source (`rabbitmq.rs`) — backbone

### 4.1 Connection
- `lapin`, URL from `RABBITMQ_URL` (env; OpenBao-injected on bto-storm,
  `~/.env` fallback — secret never hardcoded). `ConnectionProperties::default()
  .with_executor(tokio)` + `enable_auto_recover` (topology replays on reconnect).
- Exchange `claude-events` (topic, durable) — name overridable via
  `CLAUDE_EVENTS_EXCHANGE`.
- Declare orrery's **own durable** queue `orrery.hook`, bound to `hook.#`. QoS
  prefetch (e.g. 64). **Ack after successful parse** (parse failure → log + nack
  without requeue-storm, or ack-and-drop a single malformed message; do not block
  the stream).

### 4.2 Envelope → `AgentUpdate` (envelope verified in §9)
Body is a JSON `HookRelayMessage`. Key fields: `hookEvent`, `sessionId`, `cwd`,
`toolName`, `createdAt`, plus AMQP headers `session-id`, `hook-event`,
`tool-name`, `account`.

| `hookEvent` | → `AgentUpdate` |
|---|---|
| `SessionStart` | `SessionStarted { session, host, workspace, model: None, at_ms }` |
| `PreToolUse` / `PostToolUse` | `Activity { kind: ToolUse, at_ms }` |
| `UserPromptSubmit` | `Activity { kind: UserPrompt, at_ms }` |
| assistant message (if surfaced) | `Activity { kind: AssistantMessage, at_ms }` |
| `Notification` | `Attention { level: Info, at_ms }` (Error level if the payload marks an error) |
| `Stop` / `SessionEnd` / `SubagentStop` | `SessionStopped { session, at_ms }` |

Field mapping: `session` ← `sessionId`; `workspace` ← `cwd`; **host** ← the
`account` AMQP header, falling back to a value derived from `cwd` when absent;
`at_ms` ← `createdAt` (fallback: ingest wall-clock `now_ms`). Model is **not**
set here (comes from the transcript source).

## 5. Transcript model source (`transcript.rs`) — colors

- Own durable queue `orrery.transcript` bound to `transcript.message`.
- Each message is one transcript JSONL line. Parse it; on an **assistant turn**,
  extract the `model` string; emit `Summary { session, model: Some(model), .. }`
  (the reducer's `Summary` arm already merges `model`).
- **Cost control — learn-model-once-per-session.** Keep a `HashSet<SessionId>` of
  sessions whose model is known; once learned, skip parsing further transcript
  lines for that session (still ack them). This avoids paying for the full
  high-volume transcript stream just to read a rarely-changing field. (If a
  session legitimately switches model mid-run, it's re-learned on the next
  `SessionStart`/eviction; acceptable for an ambient visual.)
- `--no-transcript` disables this task entirely; live nuclei then render with the
  `hue_for_model` fallback hue (neutral azure) until/unless model is known.

## 6. Configuration

- **Env:** `RABBITMQ_URL` (required for live; if unset/unreachable, log and the
  task retries — screen shows motes only until it connects), `CLAUDE_EVENTS_EXCHANGE`
  (default `claude-events`).
- **Flags / mode:** default = live (`rabbitmq` on, `transcript` on, `synthetic`
  off). `--synthetic` forces synthetic and turns the live sources off.
  `--no-rabbitmq` and `--no-transcript` independently disable a live source.
  Existing Plan-1 flags (`--idle-ms`/`--despawn-ms`/`--max-agents`/`--agents`/
  `--motes`/`--no-bloom`/`--no-vsync`/`--screenshot`) unchanged. Add
  `ORRERY_SYNTHETIC` (so the default can be flipped via env too).

## 7. Error handling / resilience (the table-stakes subset)

- `enable_auto_recover` reconnects + replays topology; on a drop, nuclei hold last
  state until idle-fade. Each source task wraps its consume loop in a
  self-restart-with-capped-backoff supervisor; **no panics** on any network path
  (no `unwrap`/`expect` — log and degrade).
- Bounded mpsc backpressure is already handled by Plan 1 (latest-state-wins).
- **NOT in Plan 2** (deferred): per-source health in the overlay, and the
  all-quiet → synthetic auto-fallback. With those deferred, a live-mode startup
  with RabbitMQ down shows an empty field (motes only) until reconnect — accepted.

## 8. Testing

- **Parser unit tests against real captured fixtures** (a plan prerequisite is to
  capture them from the live SQLite spool / RabbitMQ): one `hook.<event>` envelope
  per representative event type, and one `transcript.message` assistant line.
  Tests assert the envelope → `AgentUpdate` mapping and the model extraction +
  learn-once dedup, with no live connection.
- The reducer/seam/render are already covered by Plan 1 tests and need no change.
- End-to-end verified by running live (real sessions appear/pulse/color/fade) and
  via `--screenshot`, as in Stage 0/1. The §3 boundary means render correctness is
  unchanged from Plan 1.

## 9. Module / file plan (for the implementation plan)

- Create `src/ingest/sources/mod.rs`, `src/ingest/sources/rabbitmq.rs`,
  `src/ingest/sources/transcript.rs`.
- Add `lapin` (and any JSON helper already present — `serde`/`serde_json`) to
  `Cargo.toml`.
- Extend `spawn_ingest` + `Config` for the live toggles and the live-default mode.
- `tests/fixtures/` (or inline `#[cfg(test)]` consts) for the captured payloads.
- No change to `model.rs` (the `AgentUpdate`/`AgentState` surface already covers
  this), `reducer.rs`, the seam, `visuals.rs`, or `diagnostics.rs`. The
  `#![allow(dead_code)]` shrinks (Attention/Summary/ActivityKind variants and
  host/workspace fields become live) — narrow or remove it as the final step.

## 10. Out of scope (later plans)

Health overlay; synthetic auto-fallback; `max_agents` cap enforcement; Mimir; REST;
`agent_id`/subagent topology; Rapier physics; orbital bodies / inter-agent arcs;
GNOME idle/D-Bus integration; GPU-timestamp instrumentation.
