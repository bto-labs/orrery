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
    let mut source = SyntheticGen::new(count, seed);
    for u in source.initial(now()) {
        if tx.send(u).await.is_err() {
            return;
        }
    }
    let mut cadence = fastrand::Rng::with_seed(seed ^ 0x5DEE_CE66);
    loop {
        for u in source.step(now()) {
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
        let source = SyntheticGen::new(16, 1);
        let inits = source.initial(0);
        assert_eq!(inits.len(), 16);
        assert!(inits.iter().all(|u| matches!(u, AgentUpdate::SessionStarted { .. })));
    }

    #[test]
    fn step_emits_activity() {
        let mut source = SyntheticGen::new(4, 7);
        let mut saw_activity = false;
        for _ in 0..50 {
            if source
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
