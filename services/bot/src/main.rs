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

use app_state::AppState;
use shared::Config;
use telegram_client::FrankensteinClient;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::tracing::init("bot")?;
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;

    info!("bot service starting");

    let nats = async_nats::connect(&config.nats_url).await?;
    let pool = persistence::init_pool(config.database_url.expose()).await?;
    // The Telegram client is shared by:
    //   - `AppState` as `Arc<dyn TelegramClient>` for the trait surface
    //     (handlers and event consumers eventually call .send_text etc).
    //   - `update_loop` as `Arc<FrankensteinClient>` so it can reach
    //     `bot()` for `get_updates` / `get_me`, which sit outside the
    //     trait surface on purpose (those types are frankenstein-only).
    // The `Arc::clone` is a refcount bump — both handles point at the
    // same underlying HTTP client.
    let telegram = Arc::new(FrankensteinClient::new(config.telegram_bot_token.expose()));
    let state = AppState::new(pool, telegram.clone());

    let updates = update_loop::run(config.clone(), state.clone(), telegram);
    let notifier = event_consumers::notify::run(config.clone(), nats.clone());

    tokio::try_join!(updates, notifier)?;
    Ok(())
}
