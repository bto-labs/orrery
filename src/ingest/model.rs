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

/// Normalized update emitted by every source (RabbitMQ hook, transcript, synthetic)
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
