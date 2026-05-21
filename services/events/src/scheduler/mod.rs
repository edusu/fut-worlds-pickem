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

    let mut ticker = interval(config.reminder_tick_interval());
    // `Burst` (the default) would fire the first tick immediately. We
    // want a small delay on startup so the bot's update loop has time
    // to come online before the scheduler tries to send the first DM.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // immediate first tick — fine, the loop short-circuits when empty

    info!(
        tick_secs = config.reminder_tick_interval().as_secs(),
        window_min, window_max, "reminder scheduler running"
    );

    loop {
        ticker.tick().await;
        if let Err(e) = reminders::run_once(&deps).await {
            error!(error = ?e, "reminder tick failed; continuing");
        }
    }
}
