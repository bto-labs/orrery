//! Live ingestion sources: each parses one claude-events stream into
//! `AgentUpdate`s and sends them on the shared mpsc. No source touches the
//! Bevy world or the triple_buffer — the reducer remains the only writer.

pub mod rabbitmq;
pub mod transcript;
