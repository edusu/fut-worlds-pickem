use error_stack::ResultExt;
use rust_utils::network::nats::NatsClient;
use serde::Serialize;
use tracing::instrument;

use crate::{MessagingError, MessagingResult};

/// Thin publisher around a `NatsClient`. The underlying client is
/// reference-counted, so this struct is cheaply cloneable across tasks.
#[derive(Clone)]
pub struct Publisher {
    client: NatsClient,
}

impl Publisher {
    /// Wrap an existing `NatsClient`.
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }

    /// Serialize `event` as JSON and publish it to `topic`.
    ///
    /// Goes through `NatsClient::inner` for the raw bytes-level publish
    /// because the JSON encoding lives here (so the error taxonomy stays
    /// inside `MessagingError`). When JetStream lands the bytes-level
    /// access stays the same — only the underlying transport semantics
    /// change.
    #[instrument(skip(self, event), fields(topic = %topic))]
    pub async fn publish<T>(&self, topic: &str, event: &T) -> MessagingResult<()>
    where
        T: Serialize + ?Sized,
    {
        let payload = serde_json::to_vec(event)
            .change_context(MessagingError::Serde)
            .attach_with(|| format!("topic = {topic}"))?;

        self.client
            .inner()
            .publish(topic.to_string(), payload.into())
            .await
            .change_context(MessagingError::Nats)
            .attach_with(|| format!("topic = {topic}"))?;

        Ok(())
    }
}
