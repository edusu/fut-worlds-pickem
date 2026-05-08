//! Events service.
//!
//! Three concurrent loops:
//!   1. `ingester` — every 5 minutes, pulls fixtures/results from the upstream
//!      sports API and upserts them; emits MatchLive / MatchFinished.
//!   2. `scheduler` — cron jobs that flip rounds to `closed` at deadline,
//!      and emit RoundDeadlineApproaching / RoundClosed.
//!   3. `scorer` — subscribes to `pickem.match.finished` and writes
//!      `points_awarded` for every prediction.

use events::{ingester, scheduler, scorer};
use shared::Config;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "events",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;

    let pool = persistence::init_pool(config.database_url.expose()).await?;
    let nats = async_nats::connect(&config.nats_url).await?;

    info!("events service starting");

    let ingest = ingester::run(config.clone(), pool.clone(), nats.clone());
    let sched = scheduler::run(config.clone(), pool.clone(), nats.clone());
    let score = scorer::run(config.clone(), pool.clone(), nats.clone());

    tokio::try_join!(ingest, sched, score)?;
    Ok(())
}
