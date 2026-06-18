# Stage 1 Ingestion Foundation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the synthetic producer's ad-hoc thread with the real Stage-1 ingestion architecture — a tokio runtime feeding a single reducer that owns the merged per-session model and writes the existing `triple_buffer` — and make the render side spawn/despawn nuclei dynamically per session, all proven end-to-end on synthetic data.

**Architecture:** A dedicated tokio runtime on its own OS thread runs source tasks that each normalize input into an `AgentUpdate` and send it over a bounded mpsc to a single reducer task. The reducer owns `HashMap<SessionId, AgentState>`, applies lifecycle on a tick, and is the only writer to `triple_buffer::Input`. Bevy reads the latest snapshot each frame (unchanged seam) and reconciles a dynamic set of nuclei keyed by `session_id`. In this plan the only source is the synthetic generator; live sources arrive in Plan 2.

**Tech Stack:** Rust 2024, Bevy 0.18.1, `tokio` (multi-thread runtime, mpsc, time), `triple_buffer` 9, `fastrand`.

## Global Constraints

- Bevy pinned at `0.18.1`; `triple_buffer = "9"`; Rust edition 2024 (toolchain 1.96, mise-managed).
- The `triple_buffer` boundary is the ONLY coupling between ingestion and the render world — preserve it. Render systems read `LatestSnapshot`; they never touch tokio.
- No `unwrap()`/`expect()` on fallible startup or network paths that could panic silently — return `Result`, log and degrade. No panics inside async source/reducer tasks.
- The reducer reads NO wall clock: all time enters via timestamps on `AgentUpdate` (incl. `Tick { now_ms }`), so its logic is deterministic and unit-testable.
- `cargo clippy --all-targets` must stay clean; `cargo test` green.
- Secrets/connection params come from env, never hardcoded (relevant in Plan 2; keep the config plumbing ready).
- Commits: do this work on a feature branch (not `main`). Per the user's CLAUDE.md, run the session-closeout skill before any real commit and never push without explicit instruction — so the per-task "Commit" steps below may be batched at checkpoints rather than executed literally per step. Keep the cadence; honor the policy.

## File structure

- Create `src/ingest/mod.rs` — runtime bootstrap (`spawn_ingest`), channel + tick wiring, `SnapshotReceiver`/`LatestSnapshot` (moved here from `sync.rs`).
- Create `src/ingest/model.rs` — `SessionId`, `Status`, `ActivityKind`, `AttentionLevel`, `AgentUpdate`, `AgentState`, `hue_for_model`.
- Create `src/ingest/reducer.rs` — `Reducer` (owns the model; `apply`, `snapshot`).
- Create `src/ingest/synthetic.rs` — `SyntheticGen` (pure update generator) + `run_synthetic` (async wrapper).
- Delete `src/agent.rs` and `src/sync.rs` — their content is superseded by `src/ingest/*` (model + producer → `model.rs`/`synthetic.rs`; triple_buffer seam → `mod.rs`).
- Modify `src/visuals.rs` — `Nucleus` keyed by `session_id` with fade state; dynamic reconcile/spawn/despawn systems; `home_for_session` hash layout; consume `AgentState` from `ingest`.
- Modify `src/diagnostics.rs` — overlay reads session count; import path updates.
- Modify `src/main.rs` — call `ingest::spawn_ingest`; add config (timeouts, max-agents cap, `--synthetic`, source toggles for Plan 2); module wiring.
- Modify `Cargo.toml` — add `tokio`.

---

### Task 1: Internal model types

**Files:**
- Create: `src/ingest/model.rs`
- Modify: `src/main.rs` (add `mod ingest;` and `pub mod` lines — minimal, just enough to compile the module)

**Interfaces:**
- Produces:
  - `pub type SessionId = String;`
  - `pub enum Status { Idle, Active, Error }` (Clone, Copy, PartialEq, Eq, Debug)
  - `pub enum ActivityKind { ToolUse, UserPrompt, AssistantMessage, Other }` (Copy)
  - `pub enum AttentionLevel { Info, Error }` (Copy)
  - `pub struct AgentState { session_id: SessionId, host: String, workspace: Option<String>, model: String, status: Status, activity_level: f32, token_rate: f32, pulse_count: u32, last_activity_ms: u64, stopped: bool }` (Clone, Debug, PartialEq)
  - `pub enum AgentUpdate { SessionStarted{session,host,workspace,model,at_ms}, Activity{session,kind,at_ms}, Attention{session,level,at_ms}, SessionStopped{session,at_ms}, Summary{session,status,workspace,model}, Metrics{session,token_rate,model,at_ms}, Tick{now_ms} }`
  - `pub fn hue_for_model(model: &str) -> f32`

- [ ] **Step 1: Create the module and add the failing test**

Create `src/ingest/model.rs`:

```rust
//! Stage-1 internal data model: the per-session state that crosses the
//! triple_buffer, and the normalized update enum that every source emits.

pub type SessionId = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Active,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    ToolUse,
    UserPrompt,
    AssistantMessage,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttentionLevel {
    Info,
    Error,
}

/// One session's instantaneous state — the triple_buffer payload.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentState {
    pub session_id: SessionId,
    pub host: String,
    pub workspace: Option<String>,
    pub model: String,
    pub status: Status,
    pub activity_level: f32,
    pub token_rate: f32,
    pub pulse_count: u32,
    pub last_activity_ms: u64,
    pub stopped: bool,
}

impl AgentState {
    /// A freshly-seen session, active as of `now_ms`.
    pub fn new(session_id: SessionId, host: String, model: String, now_ms: u64) -> Self {
        Self {
            session_id,
            host,
            workspace: None,
            model,
            status: Status::Active,
            activity_level: 0.2,
            token_rate: 0.0,
            pulse_count: 0,
            last_activity_ms: now_ms,
            stopped: false,
        }
    }
}

/// Normalized update emitted by every source (RabbitMQ, REST, Mimir, synthetic)
/// and consumed only by the reducer. Timestamps are carried in-band so the
/// reducer never reads a wall clock.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentUpdate {
    SessionStarted {
        session: SessionId,
        host: String,
        workspace: Option<String>,
        model: Option<String>,
        at_ms: u64,
    },
    Activity {
        session: SessionId,
        kind: ActivityKind,
        at_ms: u64,
    },
    Attention {
        session: SessionId,
        level: AttentionLevel,
        at_ms: u64,
    },
    SessionStopped {
        session: SessionId,
        at_ms: u64,
    },
    Summary {
        session: SessionId,
        status: Option<Status>,
        workspace: Option<String>,
        model: Option<String>,
    },
    Metrics {
        session: SessionId,
        token_rate: f32,
        model: Option<String>,
        at_ms: u64,
    },
    Tick {
        now_ms: u64,
    },
}

/// Base hue (degrees) for a model string, by family, with a neutral fallback.
pub fn hue_for_model(model: &str) -> f32 {
    let m = model.to_ascii_lowercase();
    if m.contains("opus") {
        280.0
    } else if m.contains("sonnet") {
        205.0
    } else if m.contains("haiku") {
        140.0
    } else if m.contains("fable") {
        32.0
    } else {
        200.0 // unknown model — neutral azure
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_maps_model_families_with_fallback() {
        assert_eq!(hue_for_model("claude-opus-4-8"), 280.0);
        assert_eq!(hue_for_model("claude-sonnet-4-6"), 205.0);
        assert_eq!(hue_for_model("claude-haiku-4-5"), 140.0);
        assert_eq!(hue_for_model("claude-fable-5"), 32.0);
        assert_eq!(hue_for_model("some-unknown-model"), 200.0);
    }

    #[test]
    fn new_agent_is_active_at_now() {
        let a = AgentState::new("s1".into(), "host".into(), "claude-opus-4-8".into(), 1000);
        assert_eq!(a.status, Status::Active);
        assert_eq!(a.last_activity_ms, 1000);
        assert!(!a.stopped);
    }
}
```

Add to `src/main.rs` (top, with the other `mod` lines): `mod ingest;` and in `src/ingest/mod.rs` (create a stub now so the module resolves):

```rust
//! Stage-1 ingestion: tokio runtime, sources, reducer, and the triple_buffer seam.
pub mod model;
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test --lib ingest::model`
Expected: PASS (2 tests)

- [ ] **Step 3: Verify clippy clean**

Run: `cargo clippy --all-targets`
Expected: no warnings

- [ ] **Step 4: Commit**

```bash
git add src/ingest/model.rs src/ingest/mod.rs src/main.rs
git commit -m "feat(ingest): internal AgentState/AgentUpdate model + hue mapping"
```

---

### Task 2: Reducer (owns the merged model)

**Files:**
- Create: `src/ingest/reducer.rs`
- Modify: `src/ingest/mod.rs` (add `pub mod reducer;`)

**Interfaces:**
- Consumes: `model::{AgentState, AgentUpdate, Status, ActivityKind, AttentionLevel, SessionId}`
- Produces:
  - `pub struct Reducer` with `pub fn new(idle_timeout_ms: u64, despawn_ttl_ms: u64) -> Self`
  - `pub fn apply(&mut self, update: AgentUpdate)`
  - `pub fn snapshot(&self) -> Vec<AgentState>` (sorted by `session_id` for stable output)

- [ ] **Step 1: Write the failing tests**

Create `src/ingest/reducer.rs`:

```rust
//! The single owner of the merged per-session model. Pure and deterministic:
//! all time arrives via update timestamps, so behavior is fully unit-testable.

use std::collections::HashMap;

use crate::ingest::model::{
    ActivityKind, AgentState, AgentUpdate, AttentionLevel, SessionId, Status,
};

/// How much one Activity bumps activity_level (toward 1.0).
const ACTIVITY_BUMP: f32 = 0.34;
/// Per-Tick multiplicative decay of activity_level (≈1 Hz ticks).
const ACTIVITY_DECAY: f32 = 0.9;

pub struct Reducer {
    agents: HashMap<SessionId, AgentState>,
    idle_timeout_ms: u64,
    despawn_ttl_ms: u64,
}

impl Reducer {
    pub fn new(idle_timeout_ms: u64, despawn_ttl_ms: u64) -> Self {
        Self {
            agents: HashMap::new(),
            idle_timeout_ms,
            despawn_ttl_ms,
        }
    }

    /// Get-or-create a minimal agent, so out-of-order Metrics/Summary for a
    /// not-yet-started session still surface.
    fn entry(&mut self, session: &SessionId, now_ms: u64) -> &mut AgentState {
        self.agents.entry(session.clone()).or_insert_with(|| {
            AgentState::new(session.clone(), String::new(), "unknown".into(), now_ms)
        })
    }

    pub fn apply(&mut self, update: AgentUpdate) {
        match update {
            AgentUpdate::SessionStarted {
                session,
                host,
                workspace,
                model,
                at_ms,
            } => {
                let a = self.entry(&session, at_ms);
                a.host = host;
                a.workspace = workspace.or(a.workspace.take());
                if let Some(m) = model {
                    a.model = m;
                }
                a.status = Status::Active;
                a.stopped = false;
                a.last_activity_ms = at_ms;
            }
            AgentUpdate::Activity {
                session,
                kind: _kind,
                at_ms,
            } => {
                let a = self.entry(&session, at_ms);
                a.activity_level = (a.activity_level + ACTIVITY_BUMP).min(1.0);
                a.pulse_count = a.pulse_count.wrapping_add(1);
                a.status = Status::Active;
                a.last_activity_ms = at_ms;
            }
            AgentUpdate::Attention {
                session,
                level,
                at_ms,
            } => {
                let a = self.entry(&session, at_ms);
                if level == AttentionLevel::Error {
                    a.status = Status::Error;
                }
                a.last_activity_ms = at_ms;
            }
            AgentUpdate::SessionStopped { session, at_ms } => {
                if let Some(a) = self.agents.get_mut(&session) {
                    a.stopped = true;
                    a.last_activity_ms = at_ms;
                }
            }
            AgentUpdate::Summary {
                session,
                status,
                workspace,
                model,
            } => {
                let a = self.entry(&session, 0);
                if let Some(s) = status {
                    a.status = s;
                }
                if workspace.is_some() {
                    a.workspace = workspace;
                }
                if let Some(m) = model {
                    a.model = m;
                }
            }
            AgentUpdate::Metrics {
                session,
                token_rate,
                model,
                at_ms,
            } => {
                let a = self.entry(&session, at_ms);
                a.token_rate = token_rate;
                if let Some(m) = model {
                    a.model = m;
                }
            }
            AgentUpdate::Tick { now_ms } => self.tick(now_ms),
        }
    }

    fn tick(&mut self, now_ms: u64) {
        let idle = self.idle_timeout_ms;
        let ttl = self.despawn_ttl_ms;
        for a in self.agents.values_mut() {
            a.activity_level *= ACTIVITY_DECAY;
            let since = now_ms.saturating_sub(a.last_activity_ms);
            if a.status != Status::Error && since > idle {
                a.status = Status::Idle;
            }
        }
        self.agents
            .retain(|_, a| !(a.stopped && now_ms.saturating_sub(a.last_activity_ms) > ttl));
    }

    pub fn snapshot(&self) -> Vec<AgentState> {
        let mut v: Vec<AgentState> = self.agents.values().cloned().collect();
        v.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(id: &str, at: u64) -> AgentUpdate {
        AgentUpdate::SessionStarted {
            session: id.into(),
            host: "h".into(),
            workspace: None,
            model: Some("claude-sonnet-4-6".into()),
            at_ms: at,
        }
    }

    #[test]
    fn session_started_spawns_active() {
        let mut r = Reducer::new(30_000, 120_000);
        r.apply(started("s1", 0));
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status, Status::Active);
        assert_eq!(snap[0].model, "claude-sonnet-4-6");
    }

    #[test]
    fn activity_bumps_level_and_pulse() {
        let mut r = Reducer::new(30_000, 120_000);
        r.apply(started("s1", 0));
        let before = r.snapshot()[0].clone();
        r.apply(AgentUpdate::Activity {
            session: "s1".into(),
            kind: ActivityKind::ToolUse,
            at_ms: 100,
        });
        let after = r.snapshot()[0].clone();
        assert_eq!(after.pulse_count, before.pulse_count + 1);
        assert!(after.activity_level > before.activity_level);
        assert_eq!(after.status, Status::Active);
    }

    #[test]
    fn goes_idle_after_timeout() {
        let mut r = Reducer::new(30_000, 120_000);
        r.apply(started("s1", 0));
        r.apply(AgentUpdate::Tick { now_ms: 31_000 });
        assert_eq!(r.snapshot()[0].status, Status::Idle);
    }

    #[test]
    fn despawns_after_stop_ttl() {
        let mut r = Reducer::new(30_000, 120_000);
        r.apply(started("s1", 0));
        r.apply(AgentUpdate::SessionStopped {
            session: "s1".into(),
            at_ms: 1_000,
        });
        r.apply(AgentUpdate::Tick { now_ms: 1_000 + 120_001 });
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn metrics_merge_and_create_if_absent() {
        let mut r = Reducer::new(30_000, 120_000);
        r.apply(AgentUpdate::Metrics {
            session: "s9".into(),
            token_rate: 1234.0,
            model: Some("claude-opus-4-8".into()),
            at_ms: 50,
        });
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].token_rate, 1234.0);
        assert_eq!(snap[0].model, "claude-opus-4-8");
    }
}
```

Add `pub mod reducer;` to `src/ingest/mod.rs`.

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib ingest::reducer`
Expected: PASS (5 tests)

- [ ] **Step 3: Clippy**

Run: `cargo clippy --all-targets`
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/ingest/reducer.rs src/ingest/mod.rs
git commit -m "feat(ingest): reducer owning merged per-session model with lifecycle"
```

---

### Task 3: tokio runtime + channel + triple_buffer seam

**Files:**
- Modify: `src/ingest/mod.rs`
- Modify: `Cargo.toml` (add tokio)
- Delete: `src/sync.rs` (its `SnapshotReceiver`/`LatestSnapshot`/read system move here)
- Modify: `src/main.rs` (drop `mod sync;`)

**Interfaces:**
- Consumes: `model::{AgentState, AgentUpdate}`, `reducer::Reducer`
- Produces:
  - `pub struct SnapshotReceiver(triple_buffer::Output<Vec<AgentState>>)` (Bevy `Resource`)
  - `pub struct LatestSnapshot(pub Vec<AgentState>)` (Bevy `Resource`, `Default`)
  - `pub fn read_latest_snapshot(receiver: ResMut<SnapshotReceiver>, latest: ResMut<LatestSnapshot>)`
  - `pub struct IngestHandle { pub tx: tokio::sync::mpsc::Sender<AgentUpdate> }` (so sources can be added in Plan 2)
  - `pub async fn reducer_loop(rx, input, idle_ms, ttl_ms)`
  - `pub fn spawn_ingest(idle_ms: u64, ttl_ms: u64) -> std::io::Result<(SnapshotReceiver, IngestHandle)>`

- [ ] **Step 1: Add tokio dependency**

Run:
```bash
cargo add tokio --features rt-multi-thread,macros,sync,time
```
Expected: `tokio` added to `Cargo.toml`.

- [ ] **Step 2: Write the failing async test (reducer loop publishes snapshots)**

Append to `src/ingest/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::model::AgentUpdate;

    #[tokio::test]
    async fn reducer_loop_publishes_snapshots() {
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        let initial: Vec<crate::ingest::model::AgentState> = Vec::new();
        let (input, mut output) = triple_buffer::triple_buffer(&initial);

        tokio::spawn(reducer_loop(rx, input, 30_000, 120_000));

        tx.send(AgentUpdate::SessionStarted {
            session: "s1".into(),
            host: "h".into(),
            workspace: None,
            model: None,
            at_ms: 0,
        })
        .await
        .unwrap();

        // Let the loop drain + publish.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(output.read().len(), 1);
    }
}
```

- [ ] **Step 3: Implement the seam + runtime in `src/ingest/mod.rs`**

Replace the stub `mod.rs` body with:

```rust
//! Stage-1 ingestion: tokio runtime, sources, reducer, and the triple_buffer seam.

pub mod model;
pub mod reducer;
pub mod synthetic;

use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use tokio::sync::mpsc;
use triple_buffer::{Output, triple_buffer};

use crate::ingest::model::{AgentState, AgentUpdate};
use crate::ingest::reducer::Reducer;

/// Bounded channel capacity from sources to the reducer.
const CHANNEL_CAP: usize = 1024;
/// Lifecycle tick cadence.
const TICK_MS: u64 = 1000;

/// Consumer end of the triple buffer, read once per frame by the render world.
#[derive(Resource)]
pub struct SnapshotReceiver(Output<Vec<AgentState>>);

/// The most recent snapshot, copied out each frame for parallel readers.
#[derive(Resource, Default)]
pub struct LatestSnapshot(pub Vec<AgentState>);

/// Handle the rest of the app uses to add sources (Plan 2) — a cloneable sender.
#[derive(Resource, Clone)]
pub struct IngestHandle {
    pub tx: mpsc::Sender<AgentUpdate>,
}

/// Drain updates, fold them through the reducer, publish each new snapshot.
pub async fn reducer_loop(
    mut rx: mpsc::Receiver<AgentUpdate>,
    mut input: triple_buffer::Input<Vec<AgentState>>,
    idle_ms: u64,
    ttl_ms: u64,
) {
    let mut reducer = Reducer::new(idle_ms, ttl_ms);
    while let Some(update) = rx.recv().await {
        reducer.apply(update);
        input.write(reducer.snapshot());
    }
}

/// Monotonic-ish millisecond clock for in-band timestamps on updates. Uses
/// SystemTime here (production); the reducer's *logic* is clock-free and tested
/// with injected times.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Start the tokio runtime on its own OS thread, wire the reducer loop and the
/// lifecycle tick, and return the render-side receiver plus a handle for sources.
pub fn spawn_ingest(
    idle_ms: u64,
    ttl_ms: u64,
) -> std::io::Result<(SnapshotReceiver, IngestHandle)> {
    let initial: Vec<AgentState> = Vec::new();
    let (input, output) = triple_buffer(&initial);
    let (tx, rx) = mpsc::channel::<AgentUpdate>(CHANNEL_CAP);

    let tick_tx = tx.clone();
    thread::Builder::new()
        .name("orrery-ingest".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    eprintln!("orrery: failed to build tokio runtime: {err}");
                    return;
                }
            };
            runtime.block_on(async move {
                // Lifecycle tick.
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_millis(TICK_MS));
                    loop {
                        interval.tick().await;
                        if tick_tx
                            .send(AgentUpdate::Tick { now_ms: now_ms() })
                            .await
                            .is_err()
                        {
                            break; // reducer gone
                        }
                    }
                });
                // (Plan 2: spawn rabbit/rest/mimir source tasks here, each holding
                // a clone of the sender. Synthetic is wired in Task 4 / main.)
                reducer_loop(rx, input, idle_ms, ttl_ms).await;
            });
        })?;

    Ok((SnapshotReceiver(output), IngestHandle { tx }))
}

/// Copy the latest published snapshot into [`LatestSnapshot`] (lock-free).
pub fn read_latest_snapshot(
    mut receiver: ResMut<SnapshotReceiver>,
    mut latest: ResMut<LatestSnapshot>,
) {
    let snapshot = receiver.0.read();
    latest.0.clone_from(snapshot);
}
```

Delete `src/sync.rs`: `git rm src/sync.rs`. Remove `mod sync;` from `src/main.rs` (Task 6 finishes main wiring; for now just make it compile — temporarily reference `ingest` types).

- [ ] **Step 4: Run the async test**

Run: `cargo test --lib ingest::tests::reducer_loop_publishes_snapshots`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/ingest/mod.rs src/main.rs
git rm src/sync.rs
git commit -m "feat(ingest): tokio runtime, mpsc->reducer loop, tick, triple_buffer seam"
```

---

### Task 4: Synthetic source as an `AgentUpdate` emitter

**Files:**
- Create: `src/ingest/synthetic.rs`
- Delete: `src/agent.rs`
- Modify: `src/ingest/mod.rs` (already declares `pub mod synthetic;`)

**Interfaces:**
- Consumes: `model::{AgentUpdate, ActivityKind, SessionId}`, `IngestHandle`
- Produces:
  - `pub struct SyntheticGen` with `pub fn new(count: usize, seed: u64) -> Self`
  - `pub fn initial(&self, now_ms: u64) -> Vec<AgentUpdate>` (one `SessionStarted` per fake session)
  - `pub fn step(&mut self, now_ms: u64) -> Vec<AgentUpdate>` (bursty Activity/Metrics; occasional Stop + replacement Start)
  - `pub async fn run_synthetic(tx: tokio::sync::mpsc::Sender<AgentUpdate>, count: usize, seed: u64)`

- [ ] **Step 1: Write the failing tests**

Create `src/ingest/synthetic.rs`:

```rust
//! Synthetic source: emits the same `AgentUpdate`s the live sources will, so
//! `--synthetic` and live data share one code path. Pure generator + thin async
//! wrapper; the generator is unit-tested.

use crate::ingest::model::{ActivityKind, AgentUpdate, SessionId};

const MODELS: [&str; 4] = [
    "claude-opus-4-8",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
    "claude-fable-5",
];

pub struct SyntheticGen {
    sessions: Vec<SessionId>,
    rng: fastrand::Rng,
    next_id: usize,
}

impl SyntheticGen {
    pub fn new(count: usize, seed: u64) -> Self {
        let rng = fastrand::Rng::with_seed(seed);
        let sessions = (0..count.max(1)).map(|i| format!("synthetic-{i:03}")).collect();
        Self {
            sessions,
            rng,
            next_id: count.max(1),
        }
    }

    fn model_for(&self, idx: usize) -> String {
        MODELS[idx % MODELS.len()].to_string()
    }

    /// One `SessionStarted` per current session.
    pub fn initial(&self, now_ms: u64) -> Vec<AgentUpdate> {
        self.sessions
            .iter()
            .enumerate()
            .map(|(i, s)| AgentUpdate::SessionStarted {
                session: s.clone(),
                host: "bto-storm".into(),
                workspace: Some("orrery".into()),
                model: Some(self.model_for(i)),
                at_ms: now_ms,
            })
            .collect()
    }

    /// A bursty batch of updates: usually a few Activities, sometimes Metrics,
    /// occasionally retire a session and start a fresh one (exercises dynamic
    /// spawn/despawn downstream).
    pub fn step(&mut self, now_ms: u64) -> Vec<AgentUpdate> {
        let mut out = Vec::new();
        let storm = self.rng.f32() < 0.15;
        let n = if storm { self.sessions.len() } else { 1 + self.rng.usize(0..3) };
        for _ in 0..n {
            if self.sessions.is_empty() {
                break;
            }
            let i = self.rng.usize(0..self.sessions.len());
            let session = self.sessions[i].clone();
            out.push(AgentUpdate::Activity {
                session: session.clone(),
                kind: ActivityKind::ToolUse,
                at_ms: now_ms,
            });
            if self.rng.f32() < 0.5 {
                out.push(AgentUpdate::Metrics {
                    session: session.clone(),
                    token_rate: self.rng.f32() * 2000.0,
                    model: None,
                    at_ms: now_ms,
                });
            }
        }
        // Occasionally retire + replace a session.
        if self.rng.f32() < 0.05 && !self.sessions.is_empty() {
            let i = self.rng.usize(0..self.sessions.len());
            let old = self.sessions.remove(i);
            out.push(AgentUpdate::SessionStopped {
                session: old,
                at_ms: now_ms,
            });
            let fresh = format!("synthetic-{:03}", self.next_id);
            self.next_id += 1;
            out.push(AgentUpdate::SessionStarted {
                session: fresh.clone(),
                host: "bto-storm".into(),
                workspace: Some("orrery".into()),
                model: Some(self.model_for(self.next_id)),
                at_ms: now_ms,
            });
            self.sessions.push(fresh);
        }
        out
    }
}

/// Async wrapper: emit the initial sessions, then bursty steps forever.
pub async fn run_synthetic(tx: tokio::sync::mpsc::Sender<AgentUpdate>, count: usize, seed: u64) {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let mut gen = SyntheticGen::new(count, seed);
    for u in gen.initial(now()) {
        if tx.send(u).await.is_err() {
            return;
        }
    }
    let mut cadence = fastrand::Rng::with_seed(seed ^ 0x5DEE_CE66);
    loop {
        for u in gen.step(now()) {
            if tx.send(u).await.is_err() {
                return;
            }
        }
        let ms = match cadence.u32(0..100) {
            0..=9 => cadence.u64(250..600),
            10..=24 => cadence.u64(8..20),
            _ => cadence.u64(40..90),
        };
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_emits_one_start_per_session() {
        let gen = SyntheticGen::new(16, 1);
        let inits = gen.initial(0);
        assert_eq!(inits.len(), 16);
        assert!(inits.iter().all(|u| matches!(u, AgentUpdate::SessionStarted { .. })));
    }

    #[test]
    fn step_emits_activity() {
        let mut gen = SyntheticGen::new(4, 7);
        let mut saw_activity = false;
        for _ in 0..50 {
            if gen
                .step(0)
                .iter()
                .any(|u| matches!(u, AgentUpdate::Activity { .. }))
            {
                saw_activity = true;
                break;
            }
        }
        assert!(saw_activity);
    }
}
```

Delete `src/agent.rs`: `git rm src/agent.rs`, and remove `mod agent;` from `src/main.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ingest::synthetic`
Expected: PASS (2 tests)

- [ ] **Step 3: Spawn synthetic from the runtime**

In `src/ingest/mod.rs` `spawn_ingest`, add a parameter `synthetic: Option<(usize, u64)>` (count, seed) and, inside `block_on`, before `reducer_loop`:

```rust
if let Some((count, seed)) = synthetic {
    let synth_tx = tx_for_sources.clone();
    tokio::spawn(crate::ingest::synthetic::run_synthetic(synth_tx, count, seed));
}
```

(Clone the sender into `tx_for_sources` before moving `tx` into the returned handle. Update the `spawn_ingest` signature and the Task 3 test call site to pass `None` or `Some((1, 1))`.)

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: all green

- [ ] **Step 5: Commit**

```bash
git add src/ingest/synthetic.rs src/ingest/mod.rs src/main.rs
git rm src/agent.rs
git commit -m "feat(ingest): synthetic source emitting AgentUpdates through the reducer"
```

---

### Task 5: Dynamic nuclei (render side)

**Files:**
- Modify: `src/visuals.rs`

**Interfaces:**
- Consumes: `ingest::model::{AgentState, Status, hue_for_model}`, `ingest::LatestSnapshot`
- Produces (within `visuals`):
  - `Nucleus { session_id: SessionId, home_norm: Vec2, velocity: Vec2, phase: f32, wobble_seed: f32, displayed_activity: f32, target_activity: f32, status: Status, model: String, last_pulse_count: u32, flare: f32, fade: f32, despawning: bool }`
  - `pub fn home_for_session(session_id: &str) -> Vec2` (deterministic normalized home in [-0.45,0.45]²)
  - `pub fn reconcile_nuclei(...)`, updated `apply_targets`, `animate_nuclei` (fade + despawn)

- [ ] **Step 1: Write failing tests for the pure layout helper**

Add to `src/visuals.rs` (tests module):

```rust
#[test]
fn home_for_session_is_deterministic_and_in_range() {
    let a = home_for_session("session-abc");
    let b = home_for_session("session-abc");
    assert_eq!(a, b); // stable per session
    assert!(a.x.abs() <= 0.45 && a.y.abs() <= 0.45);
    assert_ne!(home_for_session("session-abc"), home_for_session("session-xyz"));
}
```

- [ ] **Step 2: Implement `home_for_session`**

Add to `src/visuals.rs`:

```rust
use std::hash::{Hash, Hasher};

/// Deterministic home position (normalized, [-0.45, 0.45]²) from a session id,
/// so a session keeps its spot regardless of spawn order.
pub fn home_for_session(session_id: &str) -> Vec2 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    session_id.hash(&mut h);
    let v = h.finish();
    let x = ((v & 0xFFFF) as f32 / 65535.0) - 0.5;
    let y = (((v >> 16) & 0xFFFF) as f32 / 65535.0) - 0.5;
    Vec2::new(x * 0.9, y * 0.9)
}
```

Run: `cargo test --lib visuals::tests::home_for_session_is_deterministic_and_in_range`
Expected: PASS

- [ ] **Step 3: Rework `Nucleus` + add reconcile/fade (implementation)**

Replace the `Nucleus` struct and the fixed-spawn logic. Key changes (full code):

```rust
#[derive(Component)]
pub struct Nucleus {
    pub session_id: SessionId,
    pub home_norm: Vec2,
    pub velocity: Vec2,
    pub phase: f32,
    pub wobble_seed: f32,
    pub displayed_activity: f32,
    pub target_activity: f32,
    pub status: Status,
    pub model: String,
    pub last_pulse_count: u32,
    pub flare: f32,
    pub fade: f32,        // 0 = invisible, 1 = full
    pub despawning: bool, // true => fade toward 0 then despawn
}

/// Spawn a nucleus for any session in the snapshot that lacks one; mark nuclei
/// whose session vanished as despawning.
pub fn reconcile_nuclei(
    mut commands: Commands,
    latest: Res<LatestSnapshot>,
    glow: Res<GlowTexture>,
    mut existing: Query<(Entity, &mut Nucleus)>,
) {
    use std::collections::HashSet;
    let live: HashSet<&str> = latest.0.iter().map(|a| a.session_id.as_str()).collect();
    let mut have: HashSet<String> = HashSet::new();

    for (_, mut nuc) in &mut existing {
        let present = live.contains(nuc.session_id.as_str());
        nuc.despawning = !present;
        have.insert(nuc.session_id.clone());
    }

    for agent in &latest.0 {
        if have.contains(&agent.session_id) {
            continue;
        }
        let home = home_for_session(&agent.session_id);
        commands.spawn((
            Sprite {
                image: glow.0.clone(),
                color: Color::WHITE,
                custom_size: Some(Vec2::splat(BASE_NUCLEUS_SIZE)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 1.0),
            Nucleus {
                session_id: agent.session_id.clone(),
                home_norm: home,
                velocity: Vec2::ZERO,
                phase: fastrand::f32() * TAU,
                wobble_seed: fastrand::f32() * TAU,
                displayed_activity: 0.0,
                target_activity: agent.activity_level,
                status: agent.status,
                model: agent.model.clone(),
                last_pulse_count: agent.pulse_count,
                flare: 0.0,
                fade: 0.0,
                despawning: false,
            },
        ));
    }
}
```

Update `apply_targets` to match by `session_id`:

```rust
pub fn apply_targets(latest: Res<LatestSnapshot>, mut nuclei: Query<&mut Nucleus>) {
    for mut nuc in &mut nuclei {
        if let Some(state) = latest.0.iter().find(|s| s.session_id == nuc.session_id) {
            nuc.target_activity = state.activity_level;
            nuc.status = state.status;
            nuc.model = state.model.clone();
            if state.pulse_count != nuc.last_pulse_count {
                nuc.flare = (nuc.flare + 1.0).min(1.6);
                nuc.last_pulse_count = state.pulse_count;
            }
        }
    }
}
```

In `animate_nuclei`, drive fade and despawn. Add near the top of the per-nucleus loop:

```rust
// Fade in to 1.0, or (if despawning) fade out to 0 then despawn.
let fade_target = if nuc.despawning { 0.0 } else { 1.0 };
nuc.fade += (fade_target - nuc.fade) * ease_factor(dt, 0.4);
if nuc.despawning && nuc.fade < 0.02 {
    commands.entity(entity).despawn();
    continue;
}
```

(`animate_nuclei` must take `mut commands: Commands` and include `Entity` in its query: `Query<(Entity, &mut Nucleus, &mut Transform, &mut Sprite)>`.) Multiply the final `brightness` and `size` contributions by `nuc.fade` so spawns/despawns glide:

```rust
let brightness = (1.0 + act * 4.0 + breathe * 2.0 + nuc.flare * 9.0) * nuc.fade;
let size = BASE_NUCLEUS_SIZE * (0.55 + act * 1.4 + breathe + nuc.flare * 0.9) * (0.4 + 0.6 * nuc.fade);
let hue = if nuc.status == Status::Error { 0.0 } else { hue_for_model(&nuc.model) };
```

Remove the fixed nuclei spawn loop from `setup_scene` (nuclei are now created by `reconcile_nuclei`). Keep camera, glow texture, and motes in `setup_scene`.

- [ ] **Step 4: Build (render systems verified by running, not unit tests)**

Run: `cargo build --release`
Expected: compiles. (Wire-up + visual check happen in Task 6.)

- [ ] **Step 5: Commit**

```bash
git add src/visuals.rs
git commit -m "feat(visuals): dynamic nuclei reconciled by session_id with fade in/out"
```

---

### Task 6: Wire `main`, config, and verify end-to-end on synthetic

**Files:**
- Modify: `src/main.rs`
- Modify: `src/diagnostics.rs` (overlay label: "sessions" instead of fixed "agents"; import fixes)

**Interfaces:**
- Consumes: `ingest::{spawn_ingest, LatestSnapshot, SnapshotReceiver, IngestHandle, read_latest_snapshot}`

- [ ] **Step 1: Extend `Config` + parse new flags**

In `src/main.rs`, extend `Config` with `idle_ms: u64`, `despawn_ms: u64`, `max_agents: usize`, and keep `synthetic: bool` semantics: default `synthetic = true` for this plan (live sources land in Plan 2). Parse `--idle-ms`, `--despawn-ms`, `--max-agents`, `--synthetic` (env `ORRERY_IDLE_MS`, `ORRERY_DESPAWN_MS`, `ORRERY_MAX_AGENTS`). Defaults: `idle_ms=30000`, `despawn_ms=120000`, `max_agents=64`.

- [ ] **Step 2: Replace the old source wiring**

In `main()`, replace `sync::spawn_synthetic_source(...)` with:

```rust
let synthetic = if config.synthetic { Some((config.agents, config.seed)) } else { None };
let (receiver, ingest_handle) = match ingest::spawn_ingest(config.idle_ms, config.despawn_ms, synthetic) {
    Ok(pair) => pair,
    Err(err) => {
        eprintln!("orrery: failed to start ingestion: {err}");
        std::process::exit(1);
    }
};
```

Insert resources: `.insert_resource(receiver)`, `.insert_resource(ingest_handle)`, `.init_resource::<ingest::LatestSnapshot>()`. Update the Update systems tuple to call `ingest::read_latest_snapshot` (replacing `sync::read_latest_snapshot`) and add `visuals::reconcile_nuclei` before `visuals::apply_targets` in the chain:

```rust
(
    ingest::read_latest_snapshot,
    visuals::reconcile_nuclei,
    visuals::apply_targets,
    visuals::animate_nuclei,
).chain(),
```

Remove `mod sync;` and `mod agent;`; ensure `mod ingest;` present.

- [ ] **Step 3: Fix diagnostics overlay**

In `src/diagnostics.rs`, change the `Nucleus` count label from "agents" to "sessions" and update imports (`crate::ingest::...`). The `update_overlay` query `Query<(), With<Nucleus>>` is unchanged.

- [ ] **Step 4: Build, test, clippy**

Run:
```bash
cargo build --release && cargo test && cargo clippy --all-targets
```
Expected: build OK, all tests pass, clippy clean.

- [ ] **Step 5: Verify end-to-end on synthetic (manual)**

Run:
```bash
XDG_RUNTIME_DIR=/run/user/$(id -u) WAYLAND_DISPLAY=wayland-0 timeout 20 ./target/release/orrery --synthetic --agents 12
```
Expected (watch the screen + logs): ~12 nuclei **fade in**, pulse and drift, occasionally one **fades out** and a new one **fades in** elsewhere (the synthetic retire/replace), the overlay shows a changing "sessions" count. Confirms reducer → triple_buffer → dynamic nuclei end to end.

Optionally capture proof: add `--screenshot /tmp/orrery_stage1.png` and view it.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/diagnostics.rs
git commit -m "feat(ingest): wire tokio ingestion + dynamic nuclei into the app (synthetic)"
```

---

## What this plan deliberately defers to Plan 2 (post-verification)

The three live sources are **not** in this plan because their exact wire formats are unverified (spec §9). Writing their parsers now would be guesswork. Plan 2 is authored after the §9 verification captures real payloads, and adds, each as its own task with real fixtures:

- **RabbitMQ source** (`src/ingest/sources/rabbitmq.rs`, `lapin`): consume the claude-events exchange, parse the envelope, emit `SessionStarted/Activity/Attention/SessionStopped`. Backbone.
- **Mimir source** (`src/ingest/sources/mimir.rs`, `reqwest`): poll PromQL instant queries, emit `Metrics`. Gated on confirming `claude_code_*` exists with a `session.id` label.
- **REST source** (`src/ingest/sources/rest.rs`, `reqwest`): enrichment/reconciliation, emit `Summary`. Gated on finding a useful endpoint.
- **Resilience polish**: per-source health in the overlay, all-sources-quiet → synthetic auto-fallback, reconnect backoff.

Each plugs into the existing `IngestHandle.tx` channel — no change to the reducer or render side.

## Self-review notes

- **Spec coverage:** §3 architecture (Tasks 3–4), §4 model + reducer (Tasks 1–2), §5 sources (deferred to Plan 2, explicitly), §6 dynamic nuclei (Task 5), §7 resilience (partial: bounded channel + clean degradation here; health/fallback in Plan 2), §8 config (Task 6; connection-param env vars land with their sources in Plan 2), §9 verification (precedes Plan 2), §10 testing (reducer/synthetic/layout unit tests across tasks), §11 sequencing (this plan = steps 1–2; Plan 2 = steps 3–6). The synthetic-fallback half of decision §2.3 is the auto-switch, completed in Plan 2; the `--synthetic` half is here.
- **Placeholders:** none — every step has concrete code or an exact command. Plan 2 work is scoped out, not stubbed.
- **Type consistency:** `AgentUpdate`/`AgentState` field names match across Tasks 1–4; `Nucleus.session_id: SessionId` matches the model; `spawn_ingest` signature gains the `synthetic` param in Task 4 and is called with it in Task 6 (note: the Task 3 test and any earlier call sites pass the 2-arg form until Task 4 adds the param — update the test call in Task 4 Step 3).
