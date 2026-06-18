//! The lock-free seam between the synthetic producer thread and the render
//! world.
//!
//! A `triple_buffer` is the *only* coupling: the producer thread writes a fresh
//! `Vec<AgentState>` snapshot whenever it likes, and a single Bevy system reads
//! the most recently published snapshot once per frame. Neither side ever
//! blocks on the other. This is precisely where the real tokio ingestion layer
//! will plug in later — it would own the `Input` end instead of this synthetic
//! thread, and nothing on the render side would change.

use std::thread;
use std::time::Duration;

use bevy::prelude::*;
use triple_buffer::{Output, triple_buffer};

use crate::agent::{AgentState, Producer};

/// Consumer end of the triple buffer, held by the ECS. `Output` is `Send +
/// Sync`, so it lives as an ordinary resource.
#[derive(Resource)]
pub struct SnapshotReceiver(Output<Vec<AgentState>>);

/// The most recent snapshot, copied out of the triple buffer once per frame so
/// that any number of parallel systems can read it without touching the buffer.
#[derive(Resource, Default)]
pub struct LatestSnapshot(pub Vec<AgentState>);

/// Spawn the synthetic producer on a dedicated `std::thread` (deliberately
/// *not* tokio yet) and return the consumer resource. Returns an error rather
/// than panicking if the OS refuses to start the thread, so startup can fail
/// cleanly.
pub fn spawn_synthetic_source(count: usize, seed: u64) -> std::io::Result<SnapshotReceiver> {
    let initial = Producer::new(count, seed).snapshot();
    let (mut input, output) = triple_buffer(&initial);

    thread::Builder::new()
        .name("orrery-synthetic-source".into())
        .spawn(move || {
            let mut producer = Producer::new(count, seed);
            // A second RNG, decorrelated from the producer's, drives the bursty
            // *cadence* (separate from the bursty *content*).
            let mut rng = fastrand::Rng::with_seed(seed ^ 0x9E37_79B9_7F4A_7C15);
            loop {
                producer.step();
                input.write(producer.snapshot());

                // Bursty cadence: usually ~60 ms, occasionally a long stall or a
                // rapid flurry. The render side must glide across all of it.
                let ms = match rng.u32(0..100) {
                    0..=9 => rng.u64(250..600), // occasional stall
                    10..=24 => rng.u64(8..20),  // rapid flurry
                    _ => rng.u64(40..90),       // normal
                };
                thread::sleep(Duration::from_millis(ms));
            }
        })?;

    Ok(SnapshotReceiver(output))
}

/// Copy the latest published snapshot into [`LatestSnapshot`]. Reading the
/// triple buffer is lock-free and never blocks on the producer.
pub fn read_latest_snapshot(
    mut receiver: ResMut<SnapshotReceiver>,
    mut latest: ResMut<LatestSnapshot>,
) {
    let snapshot = receiver.0.read();
    latest.0.clone_from(snapshot);
}
