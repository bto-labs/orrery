//! Synthetic agent-state model and the deliberately *bursty* producer engine.
//!
//! This module is pure data + logic: no Bevy, no `triple_buffer`, no threads.
//! `sync.rs` owns the dedicated thread and the lock-free hand-off; keeping the
//! simulation engine here makes the interesting parts (clamped random walks,
//! status flips, discrete pulse accounting) unit-testable in isolation. This is
//! also exactly the seam where the real tokio ingestion layer (RabbitMQ / REST
//! / Mimir) will replace the synthetic producer in a later stage.

/// Coarse lifecycle status of an agent. Drives the animation mode and tint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Active,
    Error,
}

/// The set of models we visualise. Hue is keyed off this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    Opus,
    Sonnet,
    Haiku,
}

impl Model {
    /// Base hue in degrees (0..360) used to colour an agent's nucleus.
    pub fn hue(self) -> f32 {
        match self {
            Model::Opus => 280.0,   // violet
            Model::Sonnet => 205.0, // azure
            Model::Haiku => 140.0,  // green
        }
    }

    /// Deterministically assign a model to agent index `i`.
    pub fn from_index(i: usize) -> Model {
        match i % 3 {
            0 => Model::Opus,
            1 => Model::Sonnet,
            _ => Model::Haiku,
        }
    }
}

/// One agent's instantaneous state — the payload that crosses the
/// `triple_buffer` seam on every publish. Plain `Copy` data, cheap to snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentState {
    pub id: u32,
    pub status: AgentStatus,
    /// `0..=1` random-walked "how busy". Drives nucleus size + brightness.
    pub activity_level: f32,
    /// Synthetic tokens/sec-ish enrichment value.
    pub token_rate: f32,
    pub model: Model,
    /// Monotonic count of discrete "pulse" events. The render side diffs this
    /// against the last value it saw, so pulses survive the lossy latest-only
    /// read across the triple buffer — we never miss one even if snapshots are
    /// dropped, and we never double-count one that lingers across frames.
    pub pulse_count: u32,
}

/// A single clamped random-walk step, kept in `0..=1`.
pub fn random_walk(current: f32, delta: f32) -> f32 {
    (current + delta).clamp(0.0, 1.0)
}

/// Owns the evolving set of agents and advances them with intentional
/// burstiness. Held entirely inside the producer thread.
pub struct Producer {
    agents: Vec<AgentState>,
    rng: fastrand::Rng,
}

impl Producer {
    /// Create `count` agents seeded deterministically from `seed`.
    pub fn new(count: usize, seed: u64) -> Self {
        let mut rng = fastrand::Rng::with_seed(seed);
        let agents = (0..count)
            .map(|i| AgentState {
                id: i as u32,
                status: AgentStatus::Idle,
                activity_level: rng.f32() * 0.3,
                token_rate: 0.0,
                model: Model::from_index(i),
                pulse_count: 0,
            })
            .collect();
        Self { agents, rng }
    }

    /// Current agents (for inspection / tests).
    #[allow(dead_code)] // used by unit tests; part of the producer's surface
    pub fn agents(&self) -> &[AgentState] {
        &self.agents
    }

    /// Clone the current state into a fresh snapshot for publishing.
    pub fn snapshot(&self) -> Vec<AgentState> {
        self.agents.clone()
    }

    /// Advance the simulation by one **bursty** tick. Most agents nudge a
    /// little; occasionally an agent makes a large jump, flips status, or fires
    /// a pulse, and sometimes a whole "storm" hits many agents at once. That
    /// burstiness is the entire point — it is what the render-side exponential
    /// smoothing has to turn back into fluid motion.
    pub fn step(&mut self) {
        let storm = self.rng.f32() < 0.15;
        let count = self.agents.len();
        for idx in 0..count {
            // Draw every random decision *before* taking the &mut to the agent,
            // so there is no overlap between borrowing `self.rng` and the agent.
            let big = storm || self.rng.f32() < 0.10;
            let step_mag = if big { 0.6 } else { 0.05 };
            let delta = (self.rng.f32() * 2.0 - 1.0) * step_mag;

            let flip_chance = if storm { 0.25 } else { 0.03 };
            let new_status = if self.rng.f32() < flip_chance {
                let r = self.rng.f32();
                Some(if r < 0.6 {
                    AgentStatus::Active
                } else if r < 0.9 {
                    AgentStatus::Idle
                } else {
                    AgentStatus::Error
                })
            } else {
                None
            };

            let pulse_chance = if big { 0.4 } else { 0.02 };
            let pulse = self.rng.f32() < pulse_chance;
            let rate_noise = 0.7 + self.rng.f32() * 0.6;

            let agent = &mut self.agents[idx];
            agent.activity_level = random_walk(agent.activity_level, delta);
            if let Some(status) = new_status {
                agent.status = status;
            }
            agent.token_rate = agent.activity_level * 1200.0 * rate_noise;
            if pulse {
                agent.pulse_count = agent.pulse_count.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_walk_stays_in_unit_range() {
        assert_eq!(random_walk(0.9, 0.5), 1.0);
        assert_eq!(random_walk(0.1, -0.5), 0.0);
        assert_eq!(random_walk(0.5, 0.25), 0.75);
    }

    #[test]
    fn new_creates_requested_count_with_sequential_ids() {
        let p = Producer::new(16, 1);
        assert_eq!(p.agents().len(), 16);
        for (i, a) in p.agents().iter().enumerate() {
            assert_eq!(a.id, i as u32);
        }
    }

    #[test]
    fn activity_never_leaves_unit_range_under_bursts() {
        let mut p = Producer::new(32, 42);
        for _ in 0..5_000 {
            p.step();
            for a in p.agents() {
                assert!(
                    (0.0..=1.0).contains(&a.activity_level),
                    "activity out of range: {}",
                    a.activity_level
                );
            }
        }
    }

    #[test]
    fn pulse_count_is_monotonic() {
        let mut p = Producer::new(8, 7);
        let mut last = [0u32; 8];
        for _ in 0..2_000 {
            p.step();
            for (i, a) in p.agents().iter().enumerate() {
                assert!(a.pulse_count >= last[i], "pulse_count went backwards");
                last[i] = a.pulse_count;
            }
        }
        // Over 2000 bursty ticks, at least one pulse should have fired.
        assert!(last.iter().any(|&c| c > 0), "no pulses ever fired");
    }

    #[test]
    fn same_seed_is_deterministic() {
        let mut a = Producer::new(10, 99);
        let mut b = Producer::new(10, 99);
        for _ in 0..100 {
            a.step();
            b.step();
        }
        assert_eq!(a.snapshot(), b.snapshot());
    }
}
