//! Live ingestion sources: each parses one claude-events stream into
//! `AgentUpdate`s and sends them on the shared mpsc. No source touches the
//! Bevy world or the triple_buffer — the reducer remains the only writer.

pub mod rabbitmq;
pub mod transcript;

/// Parse an AMQP URL, defaulting an empty vhost to "/" (RabbitMQ's default
/// vhost, which a bare `amqp://host/` URL leaves empty — lapin, unlike
/// amqplib, does not coerce it). Returns a String error on parse failure.
pub(crate) fn amqp_uri_with_default_vhost(
    url: &str,
) -> Result<lapin::uri::AMQPUri, String> {
    let mut uri: lapin::uri::AMQPUri = url.parse()?;
    if uri.vhost.is_empty() {
        uri.vhost = "/".to_string();
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vhost_defaults_to_root() {
        let uri = amqp_uri_with_default_vhost("amqp://u:p@h:5672/").unwrap();
        assert_eq!(uri.vhost, "/");
    }

    #[test]
    fn explicit_vhost_is_preserved() {
        let uri = amqp_uri_with_default_vhost("amqp://u:p@h:5672/myvhost").unwrap();
        assert_eq!(uri.vhost, "myvhost");
    }
}
