//! Periodic ingestion driver.
//!
//! `run` is the long-lived entry point invoked from `services/events/main`.
//! It bootstraps the tournament-wide state (tournament row + global rounds)
//! once, then loops forever calling `poll::poll_once`. The cadence between
//! ticks adapts based on the previous tick's `TickReport`:
//!
//! - At least one match is currently live → 60 s between ticks (fast).
//! - Otherwise → 300 s between ticks (slow).
//!
//! The "is anything live?" signal is read off the report `poll_once`
//! already produced — no extra DB query — keeping the dependency graph
//! simple and the steady-state cost predictable.

pub mod bootstrap;
pub mod football_data;
pub mod poll;

use std::time::Duration;

use anyhow::Context;
use messaging::Publisher;
use persistence::repositories::{
    PgMatchRepository, PgRoundRepository, PgTeamRepository, PgTournamentRepository,
};
use shared::Config;
use sports_client::Client as SportsClient;
use sqlx::PgPool;
use tracing::{error, info};

/// Sleep duration after a tick that observed at least one live match. Short
/// enough that score updates inside the DB visibly trail the live action by
/// less than a minute.
const TICK_FAST: Duration = Duration::from_secs(60);

/// Default sleep duration. Used when no match is live and as a back-off
/// when a tick fails — short enough that recovery does not feel sluggish,
/// long enough that a flaky upstream is not hammered. Mirrors the cadence
/// originally specified in the ROADMAP.
const TICK_SLOW: Duration = Duration::from_secs(300);

/// Run the periodic ingestion loop. Polls the upstream provider, upserts
/// matches, and emits `MatchLive` / `MatchFinished` for state changes.
///
/// The function never returns `Ok(())` under normal conditions — it loops
/// forever. It returns `Err` only if the one-shot bootstrap (tournament
/// row, global rounds) fails, since without those the loop has no anchors
/// to write against. Per-tick failures inside the loop are logged and the
/// driver moves on to the next tick.
pub async fn run(config: Config, pool: PgPool, nats: async_nats::Client) -> anyhow::Result<()> {
    let tournament_repo = PgTournamentRepository::new(pool.clone());
    let round_repo = PgRoundRepository::new(pool.clone());
    let match_repo = PgMatchRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());
    let publisher = Publisher::new(nats);
    let client = SportsClient::new(config.football_api_key.clone())
        .map_err(shared::report_to_anyhow)
        .context("constructing sports-data client")?;

    // Bootstrap is part of `run`'s contract: a successful `run` always
    // implies the tournament + global rounds exist. Bubbling errors here
    // up to the caller is the right thing — without these, every tick
    // would fail anyway.
    let tournament = bootstrap::ensure_tournament(&tournament_repo).await?;
    let rounds = bootstrap::ensure_global_rounds(&client, &round_repo, &tournament).await?;

    info!(
        tournament_id = %tournament.id,
        group_round_id = %rounds.group_stage.id,
        knockout_round_id = %rounds.knockouts.id,
        "ingester ready, starting periodic loop"
    );

    // First tick fires immediately so a fresh boot does not idle for the
    // whole TICK_SLOW window before doing anything useful. Subsequent
    // sleeps are decided from the report each tick produces.
    loop {
        let wait = match poll::poll_once(
            &client,
            &match_repo,
            &team_repo,
            &publisher,
            &tournament,
            &rounds,
        )
        .await
        {
            Ok(report) if report.any_live() => TICK_FAST,
            Ok(_) => TICK_SLOW,
            Err(err) => {
                // Hard error on a tick: log and back off the slow interval.
                // Backing off (instead of retrying immediately) avoids a
                // tight loop hammering an upstream that is already flaky.
                error!(error = ?err, "ingestion tick failed; backing off");
                TICK_SLOW
            }
        };
        tokio::time::sleep(wait).await;
    }
}
