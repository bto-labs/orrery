# Stage 1 — Real Data Ingestion (design spec)

- **Status:** ✓ implemented (v0.2.0) — foundation in Plan 1, live sources in Plan 2
- **Date:** 2026-06-17
- **Builds on:** the Stage-0 POC (`POC_RESULTS.md`) and `PLAN.md` (source of truth)
- **Next step:** implementation plan via the writing-plans skill

## 1. Purpose

Replace the Stage-0 synthetic producer with a real, decoupled ingestion layer
that feeds live Claude Code agent activity into the render loop through the
existing lock-free `triple_buffer` seam — without ever letting I/O touch the
render thread. After Stage 1, orrery visualizes **real homelab agent sessions**
instead of synthetic data.

## 2. Scope decisions (agreed)

1. **Full three-source merge** — RabbitMQ + REST + Mimir/PromQL, merged per
   session (matches PLAN.md's full ingestion design). Built in that order of
   priority (RabbitMQ is the backbone).
2. **One nucleus per active Claude Code session**, keyed by `session_id`, across
   **all** homelab machines. Sessions spawn on start, fade on idle, despawn on
   stop.
3. **Synthetic source is retained**: a selectable `--synthetic` dev mode, plus an
   automatic fallback to a synthetic/demo field when no live data has arrived for
   a while. Live sources reconnect transparently and take over.

## 3. Architecture

A dedicated **tokio runtime** runs on its own OS thread (Bevy stays synchronous).
Each source is an independent async task that *only* parses + normalizes its
input into an `AgentUpdate` and sends it over a bounded `tokio::mpsc`. A single
**reducer task** owns the merged model and is the only writer to
`triple_buffer::Input`. The Bevy read system and the `sync.rs` seam are
unchanged — this is the boundary the POC proved.

```
                  ┌───────────── tokio runtime (own OS thread) ─────────────┐
 RabbitMQ ─lapin─▶ rabbit_task ─┐                                            │
 REST ───reqwest─▶ rest_task  ──┼─▶ bounded mpsc ─▶ reducer task ─ writes ─┐ │
 Mimir ──reqwest─▶ mimir_task ──┘    (AgentUpdate)   owns HashMap<         │ │
 [--synthetic] ─▶ synth_task ───┘                     SessionId,AgentState>│ │
                  └───────────────────────────────────────────────────────┼─┘
                                                                           ▼
                                              triple_buffer::Input  (UNCHANGED)
                                                                           │ Vec<AgentState>
                                                                           ▼
                                              Bevy render world (reads latest each frame)
```

New module: `src/ingest/` (runtime bootstrap, the three source tasks, the
synthetic task, the reducer, the internal types). `src/agent.rs`'s `Producer`
is refactored to emit `AgentUpdate`s through the same channel so `--synthetic`
and live data share one code path.

## 4. Internal model

### 4.1 `AgentState` (the triple_buffer payload)

| field | source of truth | drives |
|---|---|---|
| `session_id` (UUID string) | RabbitMQ | nucleus identity (match / spawn / despawn) |
| `host` | RabbitMQ / REST | label, layout seed, filtering |
| `workspace` (repo / cwd) | RabbitMQ / REST | label |
| `model` (string tag) | Mimir / RabbitMQ | hue via `hue_for_model()` (opus/sonnet/haiku/fable + fallback) |
| `status` (Idle / Active / Error) | derived | animation mode, tint |
| `activity_level` (0..1) | derived from recent event rate | size + brightness |
| `token_rate` | Mimir | brightness/enrichment |
| `pulse_count` (u32, monotonic) | RabbitMQ (per tool/message) | flares |
| `last_activity` (timestamp) | all | idle detection + despawn TTL |

The POC's fixed `Model` enum is replaced by a `model: String` tag plus
`hue_for_model(&str) -> f32` that maps model families to hues with a fallback for
unknown models.

### 4.2 `AgentUpdate` (source → reducer)

```rust
enum AgentUpdate {
    SessionStarted { session: SessionId, host: String, workspace: Option<String>,
                     model: Option<String>, at: Timestamp },
    Activity      { session: SessionId, kind: ActivityKind, at: Timestamp }, // pulse + bump + liveness
    Attention     { session: SessionId, level: AttentionLevel, at: Timestamp }, // Notification / error
    SessionStopped{ session: SessionId, at: Timestamp },
    Summary       { session: SessionId, status: Option<Status>,
                    workspace: Option<String>, model: Option<String> }, // REST enrichment
    Metrics       { session: SessionId, token_rate: f32, model: Option<String>, at: Timestamp }, // Mimir
    Tick          { now: Timestamp }, // lifecycle heartbeat (~1 Hz)
}
```

`ActivityKind` ∈ { ToolUse, UserPrompt, AssistantMessage, … }.

### 4.3 Reducer logic

Owns `HashMap<SessionId, AgentState>`. Timestamps are **passed in** (injectable
clock) so the logic is deterministic and unit-testable — no wall-clock reads
inside the reducer.

- `SessionStarted` → insert/refresh; `status = Active`; set host/workspace/model.
- `Activity` → bump `activity_level` target, `pulse_count += 1`, `status = Active`,
  refresh `last_activity`.
- `Attention(error)` → `status = Error`; `Attention(info)` → transient flag.
- `Metrics` → set `token_rate`, authoritative `model`, cost (if used).
- `Summary` → set coarse status / workspace / model metadata.
- `SessionStopped` → mark stopped (begins fade); removed after `DESPAWN_TTL`.
- `Tick` → for each session: no activity > `IDLE_TIMEOUT` (default ~30 s) ⇒
  `Idle`; stopped and past `DESPAWN_TTL` (default ~120 s, so a stopped session
  lingers long enough to fade gracefully) ⇒ remove from map. Both are
  configurable (§8).

After applying updates the reducer publishes a `Vec<AgentState>` snapshot to the
triple buffer; the buffer naturally coalesces for the single-frame reader.

## 5. Per-source mapping

PLAN.md's rule: **discrete animations from events, continuous state from metrics.**

### 5.1 RabbitMQ (`lapin`, primary, real-time)
- Durable queue bound to the claude-events exchange; QoS prefetch;
  `ConnectionProperties::default().enable_auto_recover()`; **ack after parse**.
- Maps hook events: `SessionStart` → `SessionStarted`;
  `PreToolUse`/`PostToolUse`/`UserPromptSubmit`/assistant-message → `Activity`;
  `Notification` → `Attention`; `Stop`/`SessionEnd` → `SessionStopped`.
- Provides identity, liveness, pulses, host/workspace, and model when present.

### 5.2 Mimir / PromQL (`reqwest`, ~5–15 s poll)
- Instant queries, e.g.
  `sum by (session_id, model)(rate(claude_code_token_usage_tokens_total[1m]))`,
  with the `X-Scope-OrgID` tenant header → `Metrics`.
- Subject to PLAN.md's gotchas: `opentelemetry.usePrometheusNaming` (dot vs
  underscore names) and **cumulative** temporality (`OTEL_EXPORTER_OTLP_METRICS_
  TEMPORALITY_PREFERENCE=cumulative`) or short sessions vanish before scrape.
- **Largest unverified assumption** (see §9): that `claude_code_*` series exist
  with a `session.id` label.

### 5.3 REST (`reqwest`, ~1–5 s poll)
- Finding: the claude-events REST surface is **search-oriented** (`/search/*`),
  not an obvious live "active sessions" feed. In Stage 1 REST's role is therefore
  **enrichment + reconciliation**: session metadata/summary, and re-seeding the
  model after a RabbitMQ reconnect gap — not a primary live signal.
- If no suitable endpoint exists, REST degrades to a periodic reconcile/no-op and
  this is logged (no pretending the source is live).

## 6. Dynamic nuclei (render-side changes in `visuals.rs`)

The POC spawns a fixed 16 nuclei by index; Stage 1 makes the set dynamic, keyed
by `session_id`:
- **Reconcile system** diffs each snapshot against live `Nucleus` entities: new
  session ⇒ spawn (fade in from brightness 0); session gone from snapshot ⇒ fade
  out then despawn. `Nucleus` carries `session_id` instead of `agent_id`.
- **Home layout:** deterministic position from a hash of `session_id` into the
  screen rect, so a session keeps its spot and others don't reshuffle when one
  leaves. Existing spring + drift + noise supplies the organic motion; inter-agent
  repulsion is deferred to Stage 2.
- **Fade in/out:** add a `spawn_age`/fade factor to `Nucleus` (entry/exit easing)
  so sessions glide in and out instead of popping.
- **Cap:** render up to a configurable max (default ~64) of the most-active
  sessions; overflow is **logged, never silently dropped** (first rung of
  PLAN.md's performance ladder).

## 7. Error handling / resilience (24/7 process)

- `lapin` auto-recover reconnects and replays topology; on a drop, nuclei hold
  last state until idle-fade; on reconnect, REST reconcile re-seeds.
- Bounded `mpsc`: when full, **coalesce/drop oldest per session** (latest state
  matters, not every event) and log the drop count — intentional lossy
  backpressure for an ambient visual.
- **Per-source health** (connected / last-success) surfaced in the overlay, e.g.
  `RMQ ✓  REST ✓  MIM ✗`.
- Any source may be absent without crashing; tasks self-restart with backoff; no
  panics on network paths. If **all** live sources are quiet/down past a
  threshold ⇒ auto-fallback to the synthetic/demo field, fading back to live when
  it returns.

## 8. Configuration

- Connection params via env, homelab defaults, overridable, **secrets from env
  not hardcoded**: `RABBITMQ_URL` (+ exchange / routing keys), claude-events REST
  base URL, Mimir base URL + `X-Scope-OrgID` tenant.
- New flags alongside the POC's: `--synthetic` (force), `--no-rabbitmq` /
  `--no-rest` / `--no-mimir`, idle/despawn timeouts, max-agents cap. Keeps the
  `ORRERY_*` env convention.

## 9. Source-availability verification (pre-implementation gate)

Confirm first; each source gets a synthetic-feedable stub so the build proceeds
even if a source isn't ready:
1. **RabbitMQ** — reachable URL + credentials from bto-storm; exact exchange
   name, routing-key pattern, and event envelope JSON (cross-check the existing
   mesh-six `claude-events-consumer` schema against a live message).
2. **Mimir** — that `claude_code_*` series exist (with `usePrometheusNaming` +
   cumulative temporality) carrying a `session.id` label; the query endpoint +
   tenant. (Grafana MCP was unauthorized during design — verify with creds.)
3. **REST** — identify an endpoint with useful per-session current state /
   metadata; otherwise scope REST to reconcile/no-op.

## 10. Testing

- **Reducer**: unit tests with an injectable clock — spawn-on-start, pulse
  increments, idle-after-`IDLE_TIMEOUT`, despawn-after-`DESPAWN_TTL`,
  model/token_rate merge, lossy coalescing.
- **Per-source parsers**: unit tests against captured sample payloads (one
  RabbitMQ envelope, one PromQL response, one REST response as fixtures).
- Render side verified by running, as in the POC.

## 11. Build sequencing (within Stage 1)

1. `ingest` module + tokio runtime + `AgentUpdate`/reducer + triple_buffer wiring,
   with the **synthetic** producer refactored to feed through the new path
   (no behavior change yet; the seam moves).
2. Dynamic nuclei in `visuals.rs` (spawn/despawn/fade by `session_id`), still on
   synthetic data.
3. RabbitMQ source (backbone) → real sessions appear.
4. Mimir source (enrichment) — gated on §9.2.
5. REST source (reconciliation/metadata) — gated on §9.3.
6. Resilience polish: health overlay, auto-fallback, backoff.

## 12. Out of scope (later stages)

Rapier physics, orbital bodies / inter-agent arcs, dashboard text panels,
GNOME idle/D-Bus screensaver integration, GPU-timestamp performance
instrumentation (noted in `POC_RESULTS.md`). Cost metrics beyond `token_rate`
are optional and may be deferred.
