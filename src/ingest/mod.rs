//! Stage-1 ingestion: tokio runtime, sources, reducer, and the triple_buffer seam.

// Plan-2-only surface, unused while Stage 1 runs on synthetic data alone.
// Several of these are *matched* by the reducer but have no construction site
// yet (no source emits them), which is what makes them dead code:
//   - `IngestHandle::tx` (read by live sources in Plan 2 to send updates)
//   - `ActivityKind::{UserPrompt, AssistantMessage, Other}` (never built; synthetic emits only ToolUse)
//   - `AttentionLevel::Info` (never built yet)
//   - `AgentUpdate::{Attention, Summary}` (matched in the reducer, but no source constructs them yet)
// Remove this allow once those sources are wired in Plan 2.
#![allow(dead_code)]

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
    synthetic: Option<(usize, u64)>,
) -> std::io::Result<(SnapshotReceiver, IngestHandle)> {
    let initial: Vec<AgentState> = Vec::new();
    let (input, output) = triple_buffer(&initial);
    let (tx, rx) = mpsc::channel::<AgentUpdate>(CHANNEL_CAP);

    let tick_tx = tx.clone();
    let tx_for_sources = tx.clone();
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
                if let Some((count, seed)) = synthetic {
                    let synth_tx = tx_for_sources.clone();
                    tokio::spawn(crate::ingest::synthetic::run_synthetic(synth_tx, count, seed));
                }
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
