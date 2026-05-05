use async_nats::Subscriber as NatsSubscriber;
use error_stack::ResultExt;
use rust_utils::network::validate_and_parse_json;
use serde::de::DeserializeOwned;
use tracing::warn;

use crate::{MessagingError, MessagingResult};

/// Maximum payload size accepted from NATS, in bytes. NATS itself defaults
/// to a 1 MiB server-side cap; we publish small JSON events (a
/// `MatchFinished` with team names + scoring rule fits in ~1 KiB), so 256
/// KiB is a generous ceiling that still rejects pathological inputs before
/// they reach `serde_json`.
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// Maximum JSON nesting depth accepted before deserialization. Our event
/// payloads are flat — at most a few struct fields with a small list inside.
/// 32 levels is well above any legitimate use and well below the recursion
/// limit that a stack-overflow attack would need.
const MAX_JSON_DEPTH: usize = 32;

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
            .change_context(MessagingError::Nats)
            .attach_with(|| format!("topic = {topic}"))?;
        Ok(Self {
            inner,
            _marker: std::marker::PhantomData,
        })
    }

    /// Pull the next decoded event. Returns `None` when the underlying NATS
    /// stream is closed.
    ///
    /// Each payload is validated against `MAX_PAYLOAD_BYTES` and
    /// `MAX_JSON_DEPTH` *before* `serde_json` sees the bytes, so an
    /// oversized or pathologically nested message cannot tie up the
    /// consumer task by exhausting the stack during decode. Validation
    /// failures are logged and the stream advances.
    pub async fn next(&mut self) -> Option<T> {
        loop {
            let msg = futures_next(&mut self.inner).await?;
            match validate_and_parse_json::<T>(&msg.payload, MAX_PAYLOAD_BYTES, MAX_JSON_DEPTH) {
                Ok(value) => return Some(value),
                Err(report) => {
                    warn!(
                        error = %report,
                        subject = %msg.subject,
                        payload_bytes = msg.payload.len(),
                        "failed to decode event; skipping",
                    );
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
