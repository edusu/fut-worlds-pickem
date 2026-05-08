//! Telegram bot service.
//!
//! Two concurrent loops, joined with `tokio::try_join!`:
//!   1. `update_loop` — pulls Telegram updates (long polling) and dispatches
//!      to command handlers.
//!   2. `event_consumers::notify` — subscribes to `pickem.notification.requested`
//!      and translates each event into a Telegram message.

mod app_state;
mod commands;
mod event_consumers;
mod update_loop;

use std::sync::Arc;
use std::time::Duration;

use app_state::AppState;
use shared::Config;
use telegram_client::{FrankensteinClient, TelegramClient, ThrottledTelegramClient};
use tracing::{debug, info};

/// Cadence for sweeping idle per-chat buckets out of the rate limiter.
///
/// Our buckets replenish in 1–3 seconds (private / group), so almost any
/// chat that has not been written to in a few seconds is already
/// eligible for eviction. A 5-minute sweep keeps the limiter's HashMap
/// trimmed without doing meaningful work in the steady state.
const RATE_LIMIT_CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "bot",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;

    info!("bot service starting");

    let nats = async_nats::connect(&config.nats_url).await?;
    let pool = persistence::init_pool(config.database_url.expose()).await?;

    // Build one `FrankensteinClient` (cheap-to-clone — shares the same
    // reqwest connection pool internally) and hand it out twice:
    //
    //   - Wrapped in `ThrottledTelegramClient` for the trait surface in
    //     `AppState`. Every outbound `send_*` from a handler or event
    //     consumer is paced against Telegram's documented Bot API
    //     limits (25 msg/s global, 1/s per private chat, ~20/min per
    //     group). See `telegram_client::throttle` for the rationale.
    //   - Raw, in an `Arc`, for `update_loop`. It reaches `bot()` to
    //     call `get_updates` / `get_me`, which live outside the trait
    //     because those types are frankenstein-specific.
    let frankenstein = FrankensteinClient::new(config.telegram_bot_token.expose());
    // Keep a typed handle to the throttled client so we can call its
    // non-trait surface (`cleanup_idle_keys`) from the cleanup task.
    // `AppState` only sees it through `Arc<dyn TelegramClient>`.
    let throttled = Arc::new(
        ThrottledTelegramClient::new(frankenstein.clone()).map_err(shared::report_to_anyhow)?,
    );
    let frankenstein = Arc::new(frankenstein);
    let state = AppState::new(pool, throttled.clone() as Arc<dyn TelegramClient>);

    // Periodic sweep of idle per-chat buckets. The buckets themselves
    // are the source of truth for "last activity" (via their GCRA
    // theoretical-arrival-time), so this just asks the limiter to drop
    // entries whose state is back to fresh — no parallel TTL map
    // needed. Detaches from the runtime; the spawned task ends when
    // the process does.
    let cleanup_handle = Arc::clone(&throttled);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(RATE_LIMIT_CLEANUP_INTERVAL);
        // The first `tick()` resolves immediately; skip it so the first
        // sweep happens after one full interval, by which point any
        // startup-time keys are old enough to evict.
        tick.tick().await;
        loop {
            tick.tick().await;
            cleanup_handle.cleanup_idle_keys();
            debug!("rate limiter: idle bucket sweep done");
        }
    });

    let updates = update_loop::run(config.clone(), state.clone(), frankenstein);
    let notifier = event_consumers::notify::run(config.clone(), nats.clone());

    tokio::try_join!(updates, notifier)?;
    Ok(())
}
