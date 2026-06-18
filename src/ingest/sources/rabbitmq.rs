//! The hook backbone: consume `hook.#` and map each `HookRelayMessage`
//! envelope to an `AgentUpdate`. Identity, liveness, pulses, host, workspace.

#![allow(dead_code)]

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
        "Stop" | "SessionEnd" | "SubagentStop" => {
            Some(AgentUpdate::SessionStopped { session, at_ms })
        }
        _ => None,
    }
}

/// Wall-clock ms for the `at_ms` fallback (the reducer itself stays clock-free).
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    let conn = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = conn.create_channel().await?;
    channel
        .basic_qos(64, BasicQosOptions::default())
        .await?;
    channel
        .queue_declare(
            "orrery.hook".into(),
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_bind(
            "orrery.hook".into(),
            exchange.into(),
            "hook.#".into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    let mut consumer = channel
        .basic_consume(
            "orrery.hook".into(),
            "orrery-hook".into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;

    // Consumer implements Stream; each item is Result<Delivery, lapin::Error>.
    while let Some(delivery_result) = consumer.next().await {
        let delivery = match delivery_result {
            Ok(d) => d,
            Err(err) => return Err(err.into()),
        };

        // Read the optional `account` AMQP header as a UTF-8 string.
        let account: Option<String> = delivery
            .properties
            .headers()
            .as_ref()
            .and_then(|h| h.inner().get("account"))
            .and_then(|v| v.as_long_string())
            .map(|ls| ls.to_string());

        if let Ok(msg) = parse_hook_body(&delivery.data)
            && let Some(update) = hook_to_update(&msg, account.as_deref(), now_ms())
            && tx.send(update).await.is_err()
        {
            // ack before returning so the broker doesn't redeliver
            let _ = delivery.ack(BasicAckOptions::default()).await;
            return Ok(()); // reducer gone — shut down cleanly
        }
        // Malformed bodies are acked-and-dropped; don't block the stream.
        delivery.ack(BasicAckOptions::default()).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::model::{ActivityKind, AgentUpdate, AttentionLevel};

    #[test]
    fn session_start_maps_to_session_started() {
        let msg =
            parse_hook_body(include_bytes!("fixtures/hook_sessionstart.json")).unwrap();
        let u = hook_to_update(&msg, Some("bto-storm"), 9_999).unwrap();
        match u {
            AgentUpdate::SessionStarted {
                session,
                host,
                workspace,
                model,
                ..
            } => {
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
        let msg =
            parse_hook_body(include_bytes!("fixtures/hook_pretooluse.json")).unwrap();
        match hook_to_update(&msg, None, 1).unwrap() {
            AgentUpdate::Activity {
                kind: ActivityKind::ToolUse,
                ..
            } => {}
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
            Some(AgentUpdate::Activity {
                kind: ActivityKind::UserPrompt,
                at_ms: 42,
                ..
            })
        ));
        assert!(matches!(
            hook_to_update(&mk("Notification"), None, 0),
            Some(AgentUpdate::Attention {
                level: AttentionLevel::Info,
                ..
            })
        ));
        assert!(matches!(
            hook_to_update(&mk("SessionEnd"), None, 0),
            Some(AgentUpdate::SessionStopped { .. })
        ));
        assert!(hook_to_update(&mk("SomeUnknownEvent"), None, 0).is_none());
    }

    #[test]
    fn host_falls_back_to_unknown_without_account() {
        let msg =
            parse_hook_body(include_bytes!("fixtures/hook_sessionstart.json")).unwrap();
        if let Some(AgentUpdate::SessionStarted { host, .. }) = hook_to_update(&msg, None, 0) {
            assert_eq!(host, "unknown");
        } else {
            panic!("expected SessionStarted");
        }
    }
}
