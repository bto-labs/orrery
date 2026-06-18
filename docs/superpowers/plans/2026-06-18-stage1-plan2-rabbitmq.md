# Stage 1 — Plan 2: Live RabbitMQ ingestion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Attach the live homelab `claude-events` RabbitMQ exchange as orrery's first real data source — `hook.#` for session identity/liveness/pulses and `transcript.message` for per-session model — so running `orrery` visualizes real Claude Code sessions, with synthetic retained as `--synthetic`.

**Architecture:** Two new independent async source tasks in `src/ingest/sources/` each parse+normalize their stream into the existing `AgentUpdate` enum and send on the existing bounded mpsc to the unchanged reducer → `triple_buffer` → render. The hook source maps `HookRelayMessage` envelopes to lifecycle/activity updates; the transcript source learns each session's model once and emits a `Summary`. A small supervisor reconnects each source with capped backoff. `spawn_ingest` gains a config struct that selects which sources run; the app defaults to live.

**Tech Stack:** Rust 2024, Bevy 0.18.1, tokio (already present), `lapin` (AMQP) with tokio executor/reactor adapters, `serde`/`serde_json`, `futures-lite` (Stream). Builds on Plan 1's `src/ingest/` foundation (branch `stage1-ingestion-foundation`).

## Global Constraints

- Bevy pinned `0.18.1`; Rust edition 2024 (toolchain 1.96, mise-managed — run `cargo`).
- The `triple_buffer` boundary is the ONLY coupling between ingestion and the render world. Source tasks send `AgentUpdate` on the mpsc and NEVER touch Bevy world state or write the `triple_buffer`; the reducer remains the sole writer. The render side and `reducer.rs`/`model.rs` are NOT modified by this plan.
- The reducer reads NO wall clock (unchanged). Source tasks may read the clock to stamp `at_ms` only as a *fallback* when the envelope lacks a timestamp.
- No `unwrap()`/`expect()` on fallible startup or network paths; NO panics inside async tasks — log and degrade, reconnect with backoff. (Test code may use `.unwrap()`.)
- Secrets/connection params come from env, never hardcoded: `RABBITMQ_URL`, `CLAUDE_EVENTS_EXCHANGE`. Never write the AMQP password into source, fixtures, logs, or commits.
- `cargo clippy --all-targets` stays clean; `cargo test` green.
- Default run mode is LIVE (rabbitmq + transcript on, synthetic off); `--synthetic` forces synthetic and disables the live sources.
- Exchange `claude-events` is **topic**, durable. Routing keys (verified §9): `hook.<event_lowercase>` (wildcard `hook.#`), `transcript.message`. Declare orrery's OWN durable queues; bind; ack after parse.

---

### Task 1: Dependencies + captured real fixtures

**Files:**
- Modify: `Cargo.toml` (add deps)
- Create: `src/ingest/sources/fixtures/hook_sessionstart.json`
- Create: `src/ingest/sources/fixtures/hook_pretooluse.json`
- Create: `src/ingest/sources/fixtures/hook_stop.json`
- Create: `src/ingest/sources/fixtures/transcript_assistant.jsonl`
- Create: `src/ingest/sources/fixtures/SCHEMA.md` (one-paragraph note recording the confirmed `transcript.message` body shape)

**Interfaces:**
- Produces: the dependency set and the on-disk fixtures that Tasks 2 and 3 `include_str!`. No Rust code yet.

- [ ] **Step 1: Add dependencies**

Run:
```bash
cargo add lapin
cargo add tokio-executor-trait tokio-reactor-trait
cargo add futures-lite
cargo add serde --features derive
cargo add serde_json
```
Expected: all added to `Cargo.toml`. (`lapin` needs the tokio executor + reactor adapter crates to run on our tokio runtime; `futures-lite` provides `StreamExt` for the consumer stream.)

- [ ] **Step 2: Capture real hook fixtures from the local spool**

The spool SQLite DB at `~/.claude-events/claude-events.db` holds recent hook events (the same payloads published to `hook.<event>`). Inspect the schema, then extract one representative row per event type into the fixture files as pretty JSON. Example (adjust table/column names to the actual schema you find):
```bash
sqlite3 ~/.claude-events/claude-events.db '.schema'      # find the payload table/columns
# then, for each event type, write ONE real row's JSON body to the fixture file,
# e.g. a PreToolUse, a SessionStart, and a Stop:
sqlite3 -noheader ~/.claude-events/claude-events.db \
  "select body from <table> where <event_col>='PreToolUse' order by <id> desc limit 1;" \
  > src/ingest/sources/fixtures/hook_pretooluse.json
# repeat for SessionStart -> hook_sessionstart.json, Stop -> hook_stop.json
```
Each fixture must be a real `HookRelayMessage` JSON object with at least: `hookEvent`, `sessionId`, `cwd`, and (for tool events) `toolName`, plus `createdAt` if present. **REDACT** any sensitive free-text: blank out `lastAssistantMessage`/`message`/`rawPayload` string contents (keep the keys), and DO NOT include any secret. Confirm the field names match §9 (`sessionId`, `cwd`, `hookEvent`, `toolName`, `createdAt`).

- [ ] **Step 3: Capture a real `transcript.message` body + confirm its shape**

The `transcript.message` body shape was NOT fully verified in §9 — confirm it now. Pull one real transcript line (an **assistant** turn, which carries the model). Source it from the spool DB (look for a transcript table) or, if only hook rows are spooled, read one assistant line directly from a recent transcript JSONL under `~/.claude/projects/*/<session>.jsonl`:
```bash
# find an assistant line that contains a model field:
grep -l '"type":"assistant"' ~/.claude/projects/*/*.jsonl | head -1
# write ONE such line (redacting content text) to the fixture:
# src/ingest/sources/fixtures/transcript_assistant.jsonl  (a single JSONL line)
```
Record in `src/ingest/sources/fixtures/SCHEMA.md` the EXACT JSON path to the session id and the model (e.g. top-level `sessionId` and `message.model`, or whatever the real sample shows). REDACT `content` text; keep structural keys + the `model` value. This file is the source of truth for Task 3's serde paths.

- [ ] **Step 4: Verify it compiles + fixtures are valid JSON**

Run:
```bash
cargo build
python3 -c "import json,sys; [json.load(open(f)) for f in ['src/ingest/sources/fixtures/hook_sessionstart.json','src/ingest/sources/fixtures/hook_pretooluse.json','src/ingest/sources/fixtures/hook_stop.json']]" && echo OK
head -c 200 src/ingest/sources/fixtures/transcript_assistant.jsonl
```
Expected: build OK; the three hook fixtures parse as JSON; the transcript fixture is one assistant JSONL line.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/ingest/sources/fixtures/
git commit -m "chore(ingest): add lapin/serde deps + captured real claude-events fixtures"
```

---

### Task 2: RabbitMQ hook source (parser TDD + consumer)

**Files:**
- Create: `src/ingest/sources/mod.rs`
- Create: `src/ingest/sources/rabbitmq.rs`
- Modify: `src/ingest/mod.rs` (add `pub mod sources;`)

**Interfaces:**
- Consumes: `crate::ingest::model::{AgentUpdate, ActivityKind, AttentionLevel}`; `tokio::sync::mpsc::Sender<AgentUpdate>`.
- Produces:
  - `pub(crate) struct HookRelayMessage` (serde, `rename_all = "camelCase"`) with `hook_event: String`, `session_id: String`, `cwd: Option<String>`, `tool_name: Option<String>`, `created_at: Option<u64>`, `notification_type: Option<String>`.
  - `pub(crate) fn parse_hook_body(bytes: &[u8]) -> Result<HookRelayMessage, serde_json::Error>`
  - `pub(crate) fn hook_to_update(msg: &HookRelayMessage, account: Option<&str>, now_ms: u64) -> Option<AgentUpdate>`
  - `pub async fn run_rabbitmq(tx: tokio::sync::mpsc::Sender<AgentUpdate>, url: String, exchange: String)`

- [ ] **Step 1: Create the module + write the failing parser tests**

Create `src/ingest/sources/mod.rs`:
```rust
//! Live ingestion sources: each parses one claude-events stream into
//! `AgentUpdate`s and sends them on the shared mpsc. No source touches the
//! Bevy world or the triple_buffer — the reducer remains the only writer.

pub mod rabbitmq;
pub mod transcript;
```
Create `src/ingest/sources/rabbitmq.rs` with the serde struct, the two pure fns (stubs that `todo!()` for now is NOT allowed — write the real bodies in Step 3), and this test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::model::{ActivityKind, AgentUpdate, AttentionLevel};

    #[test]
    fn session_start_maps_to_session_started() {
        let msg = parse_hook_body(include_bytes!("fixtures/hook_sessionstart.json")).unwrap();
        let u = hook_to_update(&msg, Some("bto-storm"), 9_999).unwrap();
        match u {
            AgentUpdate::SessionStarted { session, host, workspace, model, .. } => {
                assert!(!session.is_empty());
                assert_eq!(host, "bto-storm");
                assert!(workspace.is_some());
                assert!(model.is_none()); // model comes from the transcript source
            }
            other => panic!("expected SessionStarted, got {other:?}"),
        }
    }

    #[test]
    fn pretooluse_maps_to_tooluse_activity() {
        let msg = parse_hook_body(include_bytes!("fixtures/hook_pretooluse.json")).unwrap();
        match hook_to_update(&msg, None, 1).unwrap() {
            AgentUpdate::Activity { kind: ActivityKind::ToolUse, .. } => {}
            other => panic!("expected Activity(ToolUse), got {other:?}"),
        }
    }

    #[test]
    fn stop_maps_to_session_stopped() {
        let msg = parse_hook_body(include_bytes!("fixtures/hook_stop.json")).unwrap();
        assert!(matches!(
            hook_to_update(&msg, None, 1),
            Some(AgentUpdate::SessionStopped { .. })
        ));
    }

    #[test]
    fn synthesized_events_map_by_name() {
        // Build messages by hand to cover the remaining arms deterministically.
        let mk = |ev: &str| HookRelayMessage {
            hook_event: ev.into(),
            session_id: "s1".into(),
            cwd: Some("/home/jay/dev/orrery".into()),
            tool_name: None,
            created_at: Some(42),
            notification_type: None,
        };
        assert!(matches!(
            hook_to_update(&mk("UserPromptSubmit"), None, 0),
            Some(AgentUpdate::Activity { kind: ActivityKind::UserPrompt, at_ms: 42, .. })
        ));
        assert!(matches!(
            hook_to_update(&mk("Notification"), None, 0),
            Some(AgentUpdate::Attention { level: AttentionLevel::Info, .. })
        ));
        assert!(matches!(hook_to_update(&mk("SessionEnd"), None, 0), Some(AgentUpdate::SessionStopped { .. })));
        assert!(hook_to_update(&mk("SomeUnknownEvent"), None, 0).is_none());
    }

    #[test]
    fn host_falls_back_to_unknown_without_account() {
        let msg = parse_hook_body(include_bytes!("fixtures/hook_sessionstart.json")).unwrap();
        if let Some(AgentUpdate::SessionStarted { host, .. }) = hook_to_update(&msg, None, 0) {
            assert_eq!(host, "unknown");
        } else {
            panic!("expected SessionStarted");
        }
    }
}
```
Add `pub mod sources;` to `src/ingest/mod.rs` (under the existing `pub mod synthetic;`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test ingest::sources::rabbitmq`
Expected: FAIL to compile / panic (the pure fns aren't implemented yet).

- [ ] **Step 3: Implement the serde struct + the two pure functions**

In `src/ingest/sources/rabbitmq.rs` (above the tests):
```rust
//! The hook backbone: consume `hook.#` and map each `HookRelayMessage`
//! envelope to an `AgentUpdate`. Identity, liveness, pulses, host, workspace.

use std::time::Duration;

use futures_lite::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicQosOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::ingest::model::{ActivityKind, AgentUpdate, AttentionLevel};

/// The body published to `hook.<event>` (verified §9). Unknown fields ignored.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HookRelayMessage {
    pub hook_event: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub tool_name: Option<String>,
    pub created_at: Option<u64>,
    pub notification_type: Option<String>,
}

pub(crate) fn parse_hook_body(bytes: &[u8]) -> Result<HookRelayMessage, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Map a hook envelope to a normalized update. `account` is the AMQP `account`
/// header (used as the host label); `now_ms` stamps `at_ms` only when the
/// envelope lacks `createdAt`. Returns None for events we don't visualize.
pub(crate) fn hook_to_update(
    msg: &HookRelayMessage,
    account: Option<&str>,
    now_ms: u64,
) -> Option<AgentUpdate> {
    let session = msg.session_id.clone();
    let at_ms = msg.created_at.unwrap_or(now_ms);
    match msg.hook_event.as_str() {
        "SessionStart" => Some(AgentUpdate::SessionStarted {
            session,
            host: account.unwrap_or("unknown").to_string(),
            workspace: msg.cwd.clone(),
            model: None,
            at_ms,
        }),
        "PreToolUse" | "PostToolUse" => Some(AgentUpdate::Activity {
            session,
            kind: ActivityKind::ToolUse,
            at_ms,
        }),
        "UserPromptSubmit" => Some(AgentUpdate::Activity {
            session,
            kind: ActivityKind::UserPrompt,
            at_ms,
        }),
        "Notification" => Some(AgentUpdate::Attention {
            session,
            level: AttentionLevel::Info,
            at_ms,
        }),
        "Stop" | "SessionEnd" | "SubagentStop" => Some(AgentUpdate::SessionStopped { session, at_ms }),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test ingest::sources::rabbitmq`
Expected: PASS (5 tests).

- [ ] **Step 5: Implement the consumer task + reconnect supervisor**

Append to `src/ingest/sources/rabbitmq.rs`:
```rust
/// Wall-clock ms for the `at_ms` fallback (the reducer itself stays clock-free).
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Connect, declare `orrery.hook` bound to `hook.#`, consume, and forward each
/// envelope as an `AgentUpdate`. Never panics: any error logs and the outer
/// supervisor retries with capped backoff. Returns only if the channel closes.
pub async fn run_rabbitmq(tx: mpsc::Sender<AgentUpdate>, url: String, exchange: String) {
    let mut backoff_ms = 500u64;
    loop {
        match consume_once(&tx, &url, &exchange).await {
            Ok(()) => return, // tx closed -> shut down cleanly
            Err(err) => {
                eprintln!("orrery: rabbitmq source error: {err}; retrying in {backoff_ms}ms");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(30_000);
            }
        }
    }
}

/// One connect+consume cycle. Ok(()) means the reducer is gone (stop);
/// Err means a connection/stream failure the supervisor should retry.
async fn consume_once(
    tx: &mpsc::Sender<AgentUpdate>,
    url: &str,
    exchange: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let props = ConnectionProperties::default()
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);
    let conn = Connection::connect(url, props).await?;
    let channel = conn.create_channel().await?;
    channel.basic_qos(64, BasicQosOptions::default()).await?;
    channel
        .queue_declare("orrery.hook", QueueDeclareOptions { durable: true, ..Default::default() }, FieldTable::default())
        .await?;
    channel
        .queue_bind("orrery.hook", exchange, "hook.#", QueueBindOptions::default(), FieldTable::default())
        .await?;
    let mut consumer = channel
        .basic_consume("orrery.hook", "orrery-hook", BasicConsumeOptions::default(), FieldTable::default())
        .await?;

    while let Some(delivery) = consumer.next().await {
        let delivery = delivery?;
        let account = delivery
            .properties
            .headers()
            .as_ref()
            .and_then(|h| h.inner().get("account"))
            .and_then(|v| v.as_long_string().map(|s| s.to_string()));
        if let Ok(msg) = parse_hook_body(&delivery.data) {
            if let Some(update) = hook_to_update(&msg, account.as_deref(), now_ms()) {
                if tx.send(update).await.is_err() {
                    return Ok(()); // reducer gone
                }
            }
        } // malformed bodies are acked-and-dropped below (don't block the stream)
        delivery.ack(BasicAckOptions::default()).await?;
    }
    Ok(())
}
```
(If the `account` header accessor differs in the installed lapin version, adjust to the equivalent — the intent is: read the `account` AMQP header as a string, `None` if absent. Verify with `cargo build`.)

- [ ] **Step 6: Build + clippy + full test**

Run:
```bash
cargo build
cargo test
cargo clippy --all-targets
```
Expected: builds; all tests pass (Plan-1 suite + 5 new); clippy clean. (`run_rabbitmq` is currently unused by non-test code → still covered by the module `#![allow(dead_code)]`; it gets wired in Task 4.)

- [ ] **Step 7: Commit**

```bash
git add src/ingest/sources/mod.rs src/ingest/sources/rabbitmq.rs src/ingest/mod.rs
git commit -m "feat(ingest): RabbitMQ hook source — envelope->AgentUpdate parser + consumer"
```

---

### Task 3: Transcript model source (parser TDD + consumer)

**Files:**
- Create: `src/ingest/sources/transcript.rs`
- Modify: `src/ingest/sources/mod.rs` (already declares `pub mod transcript;` from Task 2)

**Interfaces:**
- Consumes: `crate::ingest::model::{AgentUpdate, SessionId}`; the `transcript_assistant.jsonl` fixture; the JSON paths recorded in `fixtures/SCHEMA.md`.
- Produces:
  - `pub(crate) fn extract_session_model(line: &[u8]) -> Option<(SessionId, String)>` — `Some((session_id, model))` for an assistant turn carrying a model, else `None`.
  - `pub(crate) struct ModelLearner { learned: std::collections::HashSet<SessionId> }` with `pub(crate) fn new() -> Self` and `pub(crate) fn observe(&mut self, session: SessionId, model: String) -> Option<AgentUpdate>` (emits `Summary` once per session, then `None`).
  - `pub async fn run_transcript(tx: tokio::sync::mpsc::Sender<AgentUpdate>, url: String, exchange: String)`

- [ ] **Step 1: Write the failing tests**

Create `src/ingest/sources/transcript.rs` with the test module (implement the bodies in Step 3):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::model::AgentUpdate;

    #[test]
    fn extracts_session_and_model_from_assistant_line() {
        let got = extract_session_model(include_bytes!("fixtures/transcript_assistant.jsonl"));
        let (session, model) = got.expect("assistant line should yield session+model");
        assert!(!session.is_empty());
        assert!(model.contains("claude")); // real captured model id
    }

    #[test]
    fn non_assistant_line_yields_none() {
        let line = br#"{"type":"user","sessionId":"s1","message":{"role":"user"}}"#;
        assert!(extract_session_model(line).is_none());
    }

    #[test]
    fn garbage_line_yields_none() {
        assert!(extract_session_model(b"not json").is_none());
    }

    #[test]
    fn learner_emits_summary_once_per_session() {
        let mut l = ModelLearner::new();
        let first = l.observe("s1".into(), "claude-opus-4-8".into());
        assert!(matches!(
            first,
            Some(AgentUpdate::Summary { model: Some(ref m), .. }) if m == "claude-opus-4-8"
        ));
        // Same session again -> suppressed.
        assert!(l.observe("s1".into(), "claude-opus-4-8".into()).is_none());
        // Different session -> emits.
        assert!(l.observe("s2".into(), "claude-sonnet-4-6".into()).is_some());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test ingest::sources::transcript`
Expected: FAIL to compile (fns not implemented).

- [ ] **Step 3: Implement the parser + learner + consumer**

In `src/ingest/sources/transcript.rs` (above the tests). **Match the JSON paths to `fixtures/SCHEMA.md`** — the code below assumes a top-level `sessionId` and `message.model` with `type == "assistant"`; if Task 1's captured sample differs, adjust the field access (and the fixture) to the real shape:
```rust
//! The transcript model source: read `transcript.message` JSONL lines, learn
//! each session's model ONCE (to avoid paying for the full transcript volume),
//! and emit a `Summary { model }` so live nuclei color correctly.

use std::collections::HashSet;
use std::time::Duration;

use futures_lite::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicQosOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties};
use tokio::sync::mpsc;

use crate::ingest::model::{AgentUpdate, SessionId};

/// Pull `(session_id, model)` from one transcript JSONL line, if it is an
/// assistant turn carrying a model. Tolerant of any non-matching/garbage line.
pub(crate) fn extract_session_model(line: &[u8]) -> Option<(SessionId, String)> {
    let v: serde_json::Value = serde_json::from_slice(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let session = v.get("sessionId")?.as_str()?.to_string();
    let model = v.get("message")?.get("model")?.as_str()?.to_string();
    Some((session, model))
}

/// Emits one `Summary { model }` per session, then suppresses repeats.
pub(crate) struct ModelLearner {
    learned: HashSet<SessionId>,
}

impl ModelLearner {
    pub(crate) fn new() -> Self {
        Self { learned: HashSet::new() }
    }

    pub(crate) fn observe(&mut self, session: SessionId, model: String) -> Option<AgentUpdate> {
        if self.learned.insert(session.clone()) {
            Some(AgentUpdate::Summary {
                session,
                status: None,
                workspace: None,
                model: Some(model),
            })
        } else {
            None
        }
    }
}

/// Connect, declare `orrery.transcript` bound to `transcript.message`, consume,
/// learn each session's model once. Never panics; supervisor retries on error.
pub async fn run_transcript(tx: mpsc::Sender<AgentUpdate>, url: String, exchange: String) {
    let mut backoff_ms = 500u64;
    loop {
        match consume_once(&tx, &url, &exchange).await {
            Ok(()) => return,
            Err(err) => {
                eprintln!("orrery: transcript source error: {err}; retrying in {backoff_ms}ms");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(30_000);
            }
        }
    }
}

async fn consume_once(
    tx: &mpsc::Sender<AgentUpdate>,
    url: &str,
    exchange: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let props = ConnectionProperties::default()
        .with_executor(tokio_executor_trait::Tokio::current())
        .with_reactor(tokio_reactor_trait::Tokio);
    let conn = Connection::connect(url, props).await?;
    let channel = conn.create_channel().await?;
    channel.basic_qos(64, BasicQosOptions::default()).await?;
    channel
        .queue_declare("orrery.transcript", QueueDeclareOptions { durable: true, ..Default::default() }, FieldTable::default())
        .await?;
    channel
        .queue_bind("orrery.transcript", exchange, "transcript.message", QueueBindOptions::default(), FieldTable::default())
        .await?;
    let mut consumer = channel
        .basic_consume("orrery.transcript", "orrery-transcript", BasicConsumeOptions::default(), FieldTable::default())
        .await?;

    let mut learner = ModelLearner::new();
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery?;
        if let Some((session, model)) = extract_session_model(&delivery.data) {
            if let Some(update) = learner.observe(session, model) {
                if tx.send(update).await.is_err() {
                    return Ok(());
                }
            }
        }
        delivery.ack(BasicAckOptions::default()).await?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test ingest::sources::transcript`
Expected: PASS (4 tests).

- [ ] **Step 5: Build + clippy + full test**

Run: `cargo build && cargo test && cargo clippy --all-targets`
Expected: builds; all tests pass; clippy clean. (`run_transcript` still unused until Task 4 → covered by the allow.)

- [ ] **Step 6: Commit**

```bash
git add src/ingest/sources/transcript.rs
git commit -m "feat(ingest): transcript model source — learn-once model extraction + consumer"
```

---

### Task 4: Wire sources into spawn_ingest + config (live default) + verify

**Files:**
- Modify: `src/ingest/mod.rs` (`spawn_ingest` signature → config struct; spawn the live source tasks)
- Modify: `src/main.rs` (`Config` + `parse_config`: live-default mode + flags + env; build the ingest config; startup log)
- Modify: `src/ingest/mod.rs` (narrow the `#![allow(dead_code)]`)

**Interfaces:**
- Consumes: `crate::ingest::sources::rabbitmq::run_rabbitmq`, `crate::ingest::sources::transcript::run_transcript`, `crate::ingest::synthetic::run_synthetic`.
- Produces:
  - `pub struct RabbitmqConfig { pub url: String, pub exchange: String, pub transcript: bool }`
  - `pub struct IngestConfig { pub idle_ms: u64, pub despawn_ms: u64, pub synthetic: Option<(usize, u64)>, pub rabbitmq: Option<RabbitmqConfig> }`
  - `pub fn spawn_ingest(cfg: IngestConfig) -> std::io::Result<(SnapshotReceiver, IngestHandle)>`

- [ ] **Step 1: Refactor `spawn_ingest` to a config struct + spawn the live tasks**

In `src/ingest/mod.rs`, replace the `spawn_ingest(idle_ms, ttl_ms, synthetic)` signature with the config-struct form. Add near the top:
```rust
/// Live RabbitMQ source settings. `transcript` toggles the model-enrichment task.
pub struct RabbitmqConfig {
    pub url: String,
    pub exchange: String,
    pub transcript: bool,
}

/// Everything `spawn_ingest` needs to decide which sources run.
pub struct IngestConfig {
    pub idle_ms: u64,
    pub despawn_ms: u64,
    /// `Some((count, seed))` runs the synthetic generator.
    pub synthetic: Option<(usize, u64)>,
    /// `Some(..)` runs the live RabbitMQ hook source (+ transcript if enabled).
    pub rabbitmq: Option<RabbitmqConfig>,
}
```
Change `spawn_ingest` to `pub fn spawn_ingest(cfg: IngestConfig) -> std::io::Result<(SnapshotReceiver, IngestHandle)>`, use `cfg.idle_ms`/`cfg.despawn_ms` for the reducer, and inside `block_on`, before `reducer_loop`, spawn the selected sources (each gets a clone of the sources sender, taken before `tx` moves into `IngestHandle`):
```rust
            runtime.block_on(async move {
                // lifecycle tick (unchanged) ...

                if let Some((count, seed)) = cfg.synthetic {
                    tokio::spawn(crate::ingest::synthetic::run_synthetic(tx_for_sources.clone(), count, seed));
                }
                if let Some(rmq) = cfg.rabbitmq {
                    tokio::spawn(crate::ingest::sources::rabbitmq::run_rabbitmq(
                        tx_for_sources.clone(), rmq.url.clone(), rmq.exchange.clone(),
                    ));
                    if rmq.transcript {
                        tokio::spawn(crate::ingest::sources::transcript::run_transcript(
                            tx_for_sources.clone(), rmq.url, rmq.exchange,
                        ));
                    }
                }

                reducer_loop(rx, input, cfg.idle_ms, cfg.despawn_ms).await;
            });
```
(Keep the existing tick task; ensure `tx_for_sources` is cloned from `tx` before `IngestHandle { tx }` is constructed, as in Plan 1.)

- [ ] **Step 2: Update `Config` + `parse_config` in `src/main.rs` (live default)**

Extend `Config` with `rabbitmq: bool` and `transcript: bool`, and change the `synthetic` default to `false`. Parsing rules:
- default: `synthetic = false`, `rabbitmq = true`, `transcript = true`.
- `--synthetic` (or `ORRERY_SYNTHETIC=1`): `synthetic = true`, `rabbitmq = false` (synthetic forces demo, disables live).
- `--no-rabbitmq`: `rabbitmq = false`.
- `--no-transcript`: `transcript = false`.
Add to the arg loop (boolean flags, `i += 1`):
```rust
            "--synthetic" => { synthetic = true; rabbitmq = false; i += 1; }
            "--no-rabbitmq" => { rabbitmq = false; i += 1; }
            "--no-transcript" => { transcript = false; i += 1; }
```
and before the loop:
```rust
    let mut synthetic = matches!(std::env::var("ORRERY_SYNTHETIC").ok().as_deref(), Some("1") | Some("true"));
    let mut rabbitmq = !synthetic;
    let mut transcript = true;
```
Keep all other existing flags/fields. Carry `rabbitmq`/`transcript` into the returned `Config`.

- [ ] **Step 3: Build the `IngestConfig` in `main()` from `Config` + env**

Replace the current `ingest::spawn_ingest(config.idle_ms, config.despawn_ms, synthetic)` call with:
```rust
    let rabbitmq = if config.rabbitmq {
        match std::env::var("RABBITMQ_URL") {
            Ok(url) => Some(ingest::RabbitmqConfig {
                url,
                exchange: std::env::var("CLAUDE_EVENTS_EXCHANGE").unwrap_or_else(|_| "claude-events".into()),
                transcript: config.transcript,
            }),
            Err(_) => {
                eprintln!("orrery: RABBITMQ_URL unset — live source disabled (use --synthetic for the demo field)");
                None
            }
        }
    } else {
        None
    };
    let synthetic = if config.synthetic { Some((config.agents, config.seed)) } else { None };
    let ingest_cfg = ingest::IngestConfig { idle_ms: config.idle_ms, despawn_ms: config.despawn_ms, synthetic, rabbitmq };
    let (receiver, ingest_handle) = match ingest::spawn_ingest(ingest_cfg) {
        Ok(pair) => pair,
        Err(err) => { eprintln!("orrery: failed to start ingestion: {err}"); std::process::exit(1); }
    };
```
Update the startup `println!` to report the active mode (e.g. `mode: live (rabbitmq, transcript)` vs `mode: synthetic`).

- [ ] **Step 4: Narrow the `#![allow(dead_code)]`**

In `src/ingest/mod.rs`, temporarily remove the `#![allow(dead_code)]` and run `cargo clippy --all-targets`. The hook/transcript sources now construct `Attention`/`Summary`/`Activity` variants and read `host`/`workspace`, so most prior dead code is live. Keep the allow ONLY if items genuinely remain unused (e.g. `ActivityKind::AssistantMessage`, `AttentionLevel::Error`, `token_rate` — the Mimir/assistant paths not built here); update the comment to that reduced set. If nothing remains unused, remove the allow entirely.

- [ ] **Step 5: Build, test, clippy**

Run:
```bash
cargo build --release && cargo test && cargo clippy --all-targets
```
Expected: builds; all tests pass; clippy clean.

- [ ] **Step 6: Verify live end-to-end (on bto-storm, Wayland)**

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0
export RABBITMQ_URL="$RABBITMQ_URL"   # already in the bto-storm env / ~/.env
timeout 30 ./target/release/orrery --screenshot /tmp/orrery_plan2.png 2>&1 | tee /tmp/orrery_plan2.log
```
Expected: connects to RabbitMQ without error (no panic, no repeating backoff log); within a few seconds real sessions appear as nuclei (the homelab is actively publishing per §9), colored by model once a transcript line is seen; the overlay "sessions" count is > 0; `/tmp/orrery_plan2.png` shows live nuclei. Also confirm `--synthetic` still produces the synthetic field and `--no-transcript` runs with neutral-hued live nuclei.

- [ ] **Step 7: Commit**

```bash
git add src/ingest/mod.rs src/main.rs
git commit -m "feat(ingest): wire live RabbitMQ + transcript sources into spawn_ingest; live-default mode"
```

---

## What this plan deliberately defers (later plans)

Per-source health overlay (`RMQ ✓/✗`), all-sources-quiet → synthetic auto-fallback, reconnect backoff beyond the basic supervisor here, `max_agents` cap enforcement, Mimir, REST, `agent_id`/subagent topology. None are needed for "real colored sessions on screen."

## Self-review notes

- **Spec coverage:** §3 architecture (Tasks 2–4), §4 hook source + envelope mapping (Task 2), §5 transcript model + learn-once (Task 3), §6 config/live-default (Task 4), §7 resilience subset = supervisor backoff + no-panic (Tasks 2–3), §8 testing = fixture-based parser unit tests + live run (Tasks 1–4), §9 file plan (all). Deferred items (§10) explicitly out.
- **Schema risk:** the one unverified shape (`transcript.message` body) is pinned down in Task 1 and recorded in `fixtures/SCHEMA.md`; Task 3's serde paths are written to match it (the only "adjust to the captured shape" point, isolated to one function).
- **Type consistency:** `AgentUpdate`/`AgentState` come from Plan 1's `model.rs` unchanged; `hook_to_update` produces `SessionStarted/Activity/Attention/SessionStopped`, `ModelLearner` produces `Summary` — all existing variants. `spawn_ingest`'s new `IngestConfig` is the only signature change; its sole caller (`main`) is updated in the same task. The Plan-1 async test calls `reducer_loop` directly and is unaffected.
- **Boundary discipline:** source tasks only `tx.send(AgentUpdate)`; none touch Bevy or the triple_buffer. The reducer/render/`model.rs` are untouched.
- **No secrets:** `RABBITMQ_URL` read from env only; fixtures redact content and carry no password.
