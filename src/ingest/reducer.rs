//! The single owner of the merged per-session model. Pure and deterministic:
//! all time arrives via update timestamps, so behavior is fully unit-testable.

use std::collections::HashMap;

use crate::ingest::model::{
    AgentState, AgentUpdate, AttentionLevel, SessionId, Status,
};
#[cfg(test)]
use crate::ingest::model::ActivityKind;

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
