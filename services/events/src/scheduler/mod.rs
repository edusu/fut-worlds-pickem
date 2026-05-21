//! Scheduler loops for the events service.
//!
//! v1 has a single periodic job: pre-deadline reminder DMs (feature
//! #8). Future deferred jobs (`SubmissionDeadlineApproaching`,
//! `SubmissionWindowClosed`, etc. — currently scaffolded but inert)
//! will register additional tickers here.

pub mod reminders;

use std::sync::Arc;

use persistence::PgPredictionReminderRepository;
use rust_utils::network::nats::NatsClient;
use shared::Config;
use sqlx::PgPool;
use telegram_client::{FrankensteinClient, TelegramClient};
use tokio::time::interval;
use tracing::{error, info};

use reminders::ReminderDeps;

/// Drive the scheduler. Builds the dependencies once and runs the
/// reminder tick on the configured cadence forever. NATS is taken even
/// though no current job needs it — keeps the signature aligned with
/// the other top-level event loops so a future job emitting events
/// does not need to refactor `main.rs`.
pub async fn run(config: Config, pool: PgPool, _nats: NatsClient) -> anyhow::Result<()> {
    let telegram: Arc<dyn TelegramClient> =
        Arc::new(FrankensteinClient::new(config.telegram_bot_token.expose()));
    let reminders_repo = Arc::new(PgPredictionReminderRepository::new(pool));
    let (window_min, window_max) = config.reminder_window_hours();
    let deps = ReminderDeps {
        reminders: reminders_repo,
        telegram,
        window_min_hours: window_min,
        window_max_hours: window_max,
    };

    let tick_interval = config.reminder_tick_interval();
    let mut ticker = interval(tick_interval);
    // If a tick body runs longer than the interval, fall behind rather
    // than burst-catching up so a slow tick can't trigger a DM storm.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // `tokio::time::interval` resolves the first tick immediately;
    // consume it so startup waits one full interval before the first
    // reminder pass (gives the bot's update loop time to come online).
    ticker.tick().await;

    info!(
        tick_secs = tick_interval.as_secs(),
        window_min, window_max, "reminder scheduler running"
    );

    loop {
        ticker.tick().await;
        if let Err(e) = reminders::run_once(&deps).await {
            error!(error = ?e, "reminder tick failed; continuing");
        }
    }
}
