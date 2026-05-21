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
use frankenstein::AsyncTelegramApi;
use rust_utils::network::nats::{NatsClient, NatsConnectOptions};
use shared::Config;
use telegram_client::{FrankensteinClient, TelegramClient, ThrottledTelegramClient};
use tracing::{debug, info, warn};

/// Cadence for sweeping idle per-chat buckets out of the rate limiter.
///
/// Our buckets replenish in 1–3 seconds (private / group), so almost any
/// chat that has not been written to in a few seconds is already
/// eligible for eviction. A 5-minute sweep keeps the limiter's HashMap
/// trimmed without doing meaningful work in the steady state.
const RATE_LIMIT_CLEANUP_INTERVAL: Duration = Duration::from_secs(300);

/// Canonical slash-command menu pushed to Telegram at startup. Must
/// stay in sync with `services/bot/src/commands/` and the table in
/// `CLAUDE.md`; `set_my_commands` is idempotent so re-running this list
/// every boot is safe.
const BOT_COMMAND_MENU: &[(&str, &str)] = &[
    ("start", "Show welcome message and quick help"),
    ("new_pickem", "Create a pickem in this group (admin only)"),
    ("join", "Join the pickem in this group"),
    ("play", "Open the Mini App to enter your predictions"),
    ("ranking", "Show the current standings in this group"),
    ("help", "Show available commands and how to play"),
];

/// Label for the DM menu button that launches the Mini App. Shown left
/// of the input field in every private chat with the bot.
const MENU_BUTTON_LABEL: &str = "Predict";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "bot",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;

    info!("bot service starting");

    let nats = NatsClient::connect(NatsConnectOptions::new(config.nats_url.as_str()))
        .await
        .map_err(shared::report_to_anyhow)?;
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

    // Resolve our own username once. The update loop uses it to gate
    // `/cmd@OtherBot` traffic.
    let bot_username = frankenstein
        .bot()
        .get_me()
        .await?
        .result
        .username
        .ok_or_else(|| anyhow::anyhow!("getMe returned a bot without a username"))?;
    let bot_username = Arc::new(bot_username);

    // Sync the slash menu and DM menu button. Failures are non-fatal —
    // commands typed by hand still work, and `/new_pickem`'s inline
    // button is an independent entry point into the Mini App.
    let telegram: Arc<dyn TelegramClient> = throttled.clone() as Arc<dyn TelegramClient>;
    if let Err(e) = telegram.set_my_commands(BOT_COMMAND_MENU).await {
        warn!(error = ?e, "set_my_commands failed; slash menu may be out of date");
    }
    if let Err(e) = telegram
        .set_chat_menu_button(MENU_BUTTON_LABEL, &config.miniapp_url)
        .await
    {
        warn!(error = ?e, "set_chat_menu_button failed; DM menu button may be missing");
    }

    let miniapp_url = Arc::new(config.miniapp_url.clone());
    let state = AppState::new(pool, telegram, miniapp_url);

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

    let updates = update_loop::run(config.clone(), state.clone(), frankenstein, bot_username);
    let notifier = event_consumers::notify::run(config.clone(), nats.clone());

    tokio::try_join!(updates, notifier)?;
    Ok(())
}
