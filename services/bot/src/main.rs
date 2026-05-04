//! Telegram bot service.
//!
//! Two concurrent loops, joined with `tokio::try_join!`:
//!   1. `update_loop` — pulls Telegram updates (long polling) and dispatches
//!      to command handlers.
//!   2. `event_consumers::notify` — subscribes to `pickem.notification.requested`
//!      and translates each event into a Telegram message.

mod commands;
mod event_consumers;
mod update_loop;

use shared::Config;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::tracing::init("bot")?;
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;

    info!("bot service starting");

    let nats = async_nats::connect(&config.nats_url).await?;
    let pool = persistence::init_pool(&config.database_url).await?;

    let updates = update_loop::run(config.clone(), pool.clone());
    let notifier = event_consumers::notify::run(config.clone(), nats.clone());

    tokio::try_join!(updates, notifier)?;
    Ok(())
}
