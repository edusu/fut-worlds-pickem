//! Rate-limiting decorator over [`TelegramClient`].
//!
//! Wraps any inner [`TelegramClient`] implementation and paces outbound
//! sends so the bot stays inside Telegram's documented Bot API limits
//! (https://core.telegram.org/bots/faq#broadcasting-to-users):
//!
//! * **Global cap** — Telegram allows up to ~30 messages per second
//!   across all chats. We pace at 25/s by default to leave headroom for
//!   retries and one-off sends slipping through other paths.
//! * **Per-chat cap** — sustained throughput is limited to 1 message per
//!   second in private chats and ~20 messages per minute in group/channel
//!   chats. Group ids are negative in the Bot API by convention, so the
//!   wrapper picks the right bucket from the sign of `chat_id` alone.
//!
//! Stacking order is **per-chat first, global second**: the per-chat
//! bucket is the narrower constraint, so acquiring it first prevents the
//! global limiter from burning a cell on a call that is about to wait on
//! the per-chat bucket anyway. This matches the recommendation in
//! `rust_utils::concurrency::rate_limit`.
//!
//! What this layer does *not* do:
//! * **No retry on `429`** — when Telegram returns Too Many Requests with
//!   `parameters.retry_after`, surface the error as-is. Honouring
//!   `retry_after` belongs in a separate retry layer (see
//!   `crate::error::TelegramError::Api`) so it can be composed
//!   independently with other retry policies.
//! * **No throttling on `answer_callback_query` / `set_webhook`** —
//!   neither contributes to the broadcasting limits, and `setWebhook` is
//!   a one-shot startup call. They pass through to the inner client
//!   unchanged.

use std::num::NonZeroU32;
use std::time::Duration;

use async_trait::async_trait;
use rust_utils::concurrency::rate_limit::{KeyedRateLimiter, RateLimiter};
use rust_utils::error::UtilsResult;
use rust_utils::network::RateLimitWindow;

use crate::{Button, TelegramClient, TelegramResult};

/// Default cap on outbound messages across the whole bot. Telegram's
/// documented ceiling is 30/s; the 5-message margin absorbs jitter,
/// retries, and concurrency drift between the limiter and the wire.
const DEFAULT_GLOBAL_PER_SECOND: u32 = 25;

/// Default sustained throughput in private chats (1 message/second).
const DEFAULT_PRIVATE_PER_SECOND: u32 = 1;

/// Default replenishment period in group/channel chats: one cell every
/// three seconds, i.e. ~20 messages/minute, mirroring the documented
/// group limit.
const DEFAULT_GROUP_PERIOD: Duration = Duration::from_secs(3);

/// A [`TelegramClient`] decorator that paces sends to stay within
/// Telegram's published rate limits.
///
/// The intended use is to construct it once at startup and hand it out
/// as `Arc<dyn TelegramClient>`; the limiter buckets behind the scenes
/// already use their own `Arc`s, so concurrent senders share the same
/// pacing state.
pub struct ThrottledTelegramClient<C: TelegramClient> {
    inner: C,
    global: RateLimiter,
    private_chats: KeyedRateLimiter<i64>,
    group_chats: KeyedRateLimiter<i64>,
}

impl<C: TelegramClient> ThrottledTelegramClient<C> {
    /// Build a wrapper with the documented Telegram defaults
    /// (25 msg/s global, 1 msg/s per private chat, ~20 msg/min per group).
    ///
    /// # Errors
    /// Propagates any failure from the underlying limiters. The default
    /// values are valid by construction, so in practice this only fails
    /// if `governor`'s invariants change in a future bump.
    pub fn new(inner: C) -> UtilsResult<Self> {
        Self::with_limits(
            inner,
            NonZeroU32::new(DEFAULT_GLOBAL_PER_SECOND)
                .expect("DEFAULT_GLOBAL_PER_SECOND is non-zero"),
            NonZeroU32::new(DEFAULT_PRIVATE_PER_SECOND)
                .expect("DEFAULT_PRIVATE_PER_SECOND is non-zero"),
            DEFAULT_GROUP_PERIOD,
        )
    }

    /// Build a wrapper with custom limits. Useful for tests that want
    /// fast paces, or for tightening the bot's behaviour in deployments
    /// that share the same token with another service.
    ///
    /// # Arguments
    /// * `inner` — underlying [`TelegramClient`] implementation.
    /// * `global_per_second` — global cap shared by every chat.
    /// * `private_per_second` — per-chat cap for private chats
    ///   (positive `chat_id`).
    /// * `group_period` — replenishment period for group chats
    ///   (negative `chat_id`); one cell becomes available every
    ///   `group_period`.
    pub fn with_limits(
        inner: C,
        global_per_second: NonZeroU32,
        private_per_second: NonZeroU32,
        group_period: Duration,
    ) -> UtilsResult<Self> {
        let global = RateLimiter::new(RateLimitWindow::PerSecond(global_per_second), None)?;
        let private_chats =
            KeyedRateLimiter::<i64>::new(RateLimitWindow::PerSecond(private_per_second), None)?;
        let group_chats = KeyedRateLimiter::<i64>::new(
            RateLimitWindow::Custom {
                period: group_period,
            },
            None,
        )?;
        Ok(Self {
            inner,
            global,
            private_chats,
            group_chats,
        })
    }

    /// Pick the right per-chat bucket from the sign of `chat_id`.
    /// Negative ids are groups/channels in the Bot API; positive ids are
    /// users (private chats). A `chat_id` of `0` is not a valid Telegram
    /// id but is routed to private as a safe default.
    fn per_chat(&self, chat_id: i64) -> &KeyedRateLimiter<i64> {
        if chat_id < 0 {
            &self.group_chats
        } else {
            &self.private_chats
        }
    }

    /// Drop idle per-chat buckets and shrink the underlying state.
    ///
    /// Keyed limiters never automatically forget keys, so a long-running
    /// bot accumulating chat ids should call this periodically. Cheap to
    /// run; safe under concurrent use.
    pub fn cleanup_idle_keys(&self) {
        self.private_chats.retain_recent();
        self.private_chats.shrink_to_fit();
        self.group_chats.retain_recent();
        self.group_chats.shrink_to_fit();
    }

    /// Borrow the wrapped client. Useful for adapters that need
    /// implementation-specific surface (e.g. `FrankensteinClient::bot`)
    /// without giving up the throttled trait view.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

#[async_trait]
impl<C: TelegramClient> TelegramClient for ThrottledTelegramClient<C> {
    async fn send_text(&self, chat_id: i64, text: &str) -> TelegramResult<()> {
        // Per-chat first, global second — see module docs for rationale.
        self.per_chat(chat_id)
            .run(&chat_id, || async {
                self.global
                    .run(|| self.inner.send_text(chat_id, text))
                    .await
            })
            .await
    }

    async fn send_text_with_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[Vec<Button>],
    ) -> TelegramResult<()> {
        self.per_chat(chat_id)
            .run(&chat_id, || async {
                self.global
                    .run(|| self.inner.send_text_with_buttons(chat_id, text, buttons))
                    .await
            })
            .await
    }

    /// Pass-through: callback answers do not count against Telegram's
    /// broadcasting limits, so there is nothing to pace here.
    async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> TelegramResult<()> {
        self.inner
            .answer_callback_query(callback_query_id, text)
            .await
    }

    /// Pass-through: `setWebhook` is a one-shot configuration call.
    async fn set_webhook(&self, url: &str) -> TelegramResult<()> {
        self.inner.set_webhook(url).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    /// Minimal test double that records every `send_text` call without
    /// actually hitting any network. The counter sits behind an `Arc` so
    /// the test can clone the client into the wrapper and still observe
    /// call counts through its original handle.
    #[derive(Debug, Default, Clone)]
    struct CountingClient {
        calls: Arc<AtomicUsize>,
    }

    impl CountingClient {
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TelegramClient for CountingClient {
        async fn send_text(&self, _chat_id: i64, _text: &str) -> TelegramResult<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn send_text_with_buttons(
            &self,
            _chat_id: i64,
            _text: &str,
            _buttons: &[Vec<Button>],
        ) -> TelegramResult<()> {
            Ok(())
        }

        async fn answer_callback_query(
            &self,
            _callback_query_id: &str,
            _text: Option<&str>,
        ) -> TelegramResult<()> {
            Ok(())
        }

        async fn set_webhook(&self, _url: &str) -> TelegramResult<()> {
            Ok(())
        }
    }

    /// Hammering the same private chat must serialise at the per-chat
    /// rate. With a 2 msg/s per-private-chat cap, the third call should
    /// land at least ~500ms after the first two have drained the burst.
    #[tokio::test]
    async fn private_chat_paces_same_chat() {
        let inner = CountingClient::default();
        let observer = inner.clone();
        let throttled = ThrottledTelegramClient::with_limits(
            inner,
            NonZeroU32::new(100).unwrap(), // global: effectively unbounded for the test
            NonZeroU32::new(2).unwrap(),
            Duration::from_secs(60),
        )
        .expect("limits are valid");

        // Drain the burst.
        throttled.send_text(42, "first").await.unwrap();
        throttled.send_text(42, "second").await.unwrap();

        let start = Instant::now();
        throttled.send_text(42, "third").await.unwrap();
        assert!(
            start.elapsed() >= Duration::from_millis(400),
            "third call should have waited for the per-chat bucket"
        );
        assert_eq!(observer.calls(), 3);
    }

    /// Two distinct private chats must not block each other: draining
    /// chat A's bucket should leave chat B's first call free to fire
    /// immediately.
    #[tokio::test]
    async fn private_chats_are_independent() {
        let inner = CountingClient::default();
        let throttled = ThrottledTelegramClient::with_limits(
            inner,
            NonZeroU32::new(100).unwrap(),
            NonZeroU32::new(1).unwrap(),
            Duration::from_secs(60),
        )
        .unwrap();

        throttled.send_text(1, "drain").await.unwrap();

        let start = Instant::now();
        throttled.send_text(2, "free").await.unwrap();
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "different chats must not contend on the per-chat limiter"
        );
    }

    /// Group ids (negative `chat_id`) take the slower bucket. With a
    /// 100ms group period, the second call to the same group must wait
    /// ~100ms even though the per-private and global caps are slack.
    #[tokio::test]
    async fn group_chat_uses_slower_bucket() {
        let inner = CountingClient::default();
        let throttled = ThrottledTelegramClient::with_limits(
            inner,
            NonZeroU32::new(100).unwrap(),
            NonZeroU32::new(100).unwrap(),
            Duration::from_millis(100),
        )
        .unwrap();

        throttled.send_text(-1001, "first").await.unwrap();

        let start = Instant::now();
        throttled.send_text(-1001, "second").await.unwrap();
        assert!(
            start.elapsed() >= Duration::from_millis(80),
            "group chats should be paced by the group period"
        );
    }
}
