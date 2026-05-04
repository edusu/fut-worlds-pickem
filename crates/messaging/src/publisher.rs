use error_stack::ResultExt;
use serde::Serialize;
use tracing::instrument;

use crate::{MessagingError, MessagingResult};

/// Thin publisher around a NATS client. Holds the client by value so it can
/// be cloned cheaply across tasks (the client itself is a handle).
#[derive(Clone)]
pub struct Publisher {
    client: async_nats::Client,
}

impl Publisher {
    /// Wrap an existing NATS client.
    pub fn new(client: async_nats::Client) -> Self {
        Self { client }
    }

    /// Serialize `event` as JSON and publish it to `topic`.
    ///
    /// JSON is used (not protobuf) for ergonomics and observability — payloads
    /// stay human-readable in the NATS CLI and Jaeger spans. If volume becomes
    /// a concern, swap encoders here without touching call sites.
    ///
    /// Errors carry the originating cause attached to the report chain plus
    /// the topic name as a printable attachment.
    #[instrument(skip(self, event), fields(topic = %topic))]
    pub async fn publish<T>(&self, topic: &str, event: &T) -> MessagingResult<()>
    where
        T: Serialize + ?Sized,
    {
        let payload = serde_json::to_vec(event)
            .change_context(MessagingError::Serde)
            .attach_with(|| format!("topic = {topic}"))?;

        self.client
            .publish(topic.to_string(), payload.into())
            .await
            .change_context(MessagingError::Nats)
            .attach_with(|| format!("topic = {topic}"))?;

        Ok(())
    }
}
