use async_nats::Subscriber as NatsSubscriber;
use serde::de::DeserializeOwned;
use tracing::warn;

use crate::{MessagingError, MessagingResult};

/// Helper that wraps a NATS subscription and yields decoded events of type
/// `T`. Decode errors are logged and the stream advances — bad messages
/// should not stall the consumer loop.
pub struct Subscriber<T> {
    inner: NatsSubscriber,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Subscriber<T>
where
    T: DeserializeOwned,
{
    /// Subscribe to `topic` on the given client.
    pub async fn subscribe(client: &async_nats::Client, topic: &str) -> MessagingResult<Self> {
        let inner = client
            .subscribe(topic.to_string())
            .await
            .map_err(|e| MessagingError::Nats(e.to_string()))?;
        Ok(Self {
            inner,
            _marker: std::marker::PhantomData,
        })
    }

    /// Pull the next decoded event. Returns `None` when the underlying NATS
    /// stream is closed.
    pub async fn next(&mut self) -> Option<T> {
        loop {
            let msg = futures_next(&mut self.inner).await?;
            match serde_json::from_slice::<T>(&msg.payload) {
                Ok(value) => return Some(value),
                Err(err) => {
                    warn!(error = %err, subject = %msg.subject, "failed to decode event; skipping");
                }
            }
        }
    }
}

/// Internal helper that adapts `async_nats::Subscriber`'s stream API to a
/// simple `async fn`. Kept private so call sites do not need to import
/// `futures::StreamExt`.
async fn futures_next(sub: &mut NatsSubscriber) -> Option<async_nats::Message> {
    use tokio_stream::StreamExt;
    sub.next().await
}
