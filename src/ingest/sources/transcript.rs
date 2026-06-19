//! The transcript model source: read `transcript.message` JSONL lines, learn
//! each session's model ONCE (to avoid paying for the full transcript volume),
//! and emit a `Summary { model }` so live nuclei colour correctly.

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
/// assistant turn carrying a model. Tolerant of any non-matching or garbage line.
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
/// learn each session's model once. Never panics: any error logs and the outer
/// supervisor retries with capped backoff. Returns only if the channel closes.
pub async fn run_transcript(tx: mpsc::Sender<AgentUpdate>, url: String, exchange: String) {
    let mut backoff_ms = 500u64;
    loop {
        match consume_once(&tx, &url, &exchange).await {
            Ok(()) => return, // tx closed -> shut down cleanly
            Err(err) => {
                eprintln!("orrery: transcript source error: {err}; retrying in {backoff_ms}ms");
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
    let uri = crate::ingest::sources::amqp_uri_with_default_vhost(url)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
    let conn = Connection::connect_uri(uri, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;
    channel
        .basic_qos(64, BasicQosOptions::default())
        .await?;
    channel
        .queue_declare(
            "orrery.transcript".into(),
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_bind(
            "orrery.transcript".into(),
            exchange.into(),
            "transcript.message".into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    let mut consumer = channel
        .basic_consume(
            "orrery.transcript".into(),
            "orrery-transcript".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    let mut learner = ModelLearner::new();
    // Consumer implements Stream; each item is Result<Delivery, lapin::Error>.
    while let Some(delivery_result) = consumer.next().await {
        let delivery = match delivery_result {
            Ok(d) => d,
            Err(err) => return Err(err.into()),
        };
        if let Some((session, model)) = extract_session_model(&delivery.data)
            && let Some(update) = learner.observe(session, model)
            && tx.send(update).await.is_err()
        {
            // ack before returning so the broker doesn't redeliver
            let _ = delivery.ack(BasicAckOptions::default()).await;
            return Ok(()); // reducer gone — shut down cleanly
        }
        // Malformed or already-learned lines are acked-and-dropped; don't block the stream.
        delivery.ack(BasicAckOptions::default()).await?;
    }
    Ok(())
}

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
