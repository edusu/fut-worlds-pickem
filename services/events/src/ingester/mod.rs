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

//! Periodic ingestion driver. Bootstraps the tournament-wide state once,
//! then loops calling `poll::poll_once` with an adaptive cadence: 60 s
//! while any match is live (signal carried by the `TickReport`), 300 s
//! otherwise.

pub mod bootstrap;
pub mod football_data;
pub mod poll;

use std::time::Duration;

use anyhow::Context;
use domain::repository::TeamRepository;
use messaging::Publisher;
use persistence::repositories::{
    PgMatchRepository, PgRoundRepository, PgTeamRepository, PgTournamentRepository,
};
use shared::Config;
use sports_client::Client as SportsClient;
use sqlx::PgPool;
use tracing::{error, info};

const TICK_FAST: Duration = Duration::from_secs(60);
const TICK_SLOW: Duration = Duration::from_secs(300);

/// Run the periodic ingestion loop forever. Returns `Err` only if the
/// one-shot bootstrap fails; per-tick failures are logged and the loop
/// continues with the slow back-off interval.
pub async fn run(config: Config, pool: PgPool, nats: async_nats::Client) -> anyhow::Result<()> {
    let tournament_repo = PgTournamentRepository::new(pool.clone());
    let round_repo = PgRoundRepository::new(pool.clone());
    let match_repo = PgMatchRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());
    let publisher = Publisher::new(nats);
    let client = SportsClient::new(config.football_api_key.clone())
        .map_err(shared::report_to_anyhow)
        .context("constructing sports-data client")?;

    let tournament = bootstrap::ensure_tournament(
        &tournament_repo,
        bootstrap::V1_TOURNAMENT_EXTERNAL_ID,
        bootstrap::V1_TOURNAMENT_NAME,
    )
    .await?;
    let rounds = bootstrap::ensure_global_rounds(&client, &round_repo, &tournament).await?;

    // Load teams once after bootstrap. The set is stable for the whole run
    // (tournament-data ingester #9 seeds it before the loop starts and does
    // not append later); a service restart picks up any future changes.
    let teams = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading teams for resolver index")?;
    let team_index = poll::build_team_index(&teams);

    info!(
        tournament_id = %tournament.id,
        group_round_id = %rounds.group_stage.id,
        knockout_round_id = %rounds.knockouts.id,
        team_count = team_index.len(),
        "ingester ready, starting periodic loop"
    );

    // First tick fires immediately so a fresh boot does not idle for the
    // whole TICK_SLOW window before doing anything useful.
    loop {
        let wait = match poll::poll_once(
            &client,
            &match_repo,
            &team_index,
            &publisher,
            &tournament,
            &rounds,
        )
        .await
        {
            Ok(report) if report.any_live() => TICK_FAST,
            Ok(_) => TICK_SLOW,
            Err(err) => {
                error!(error = ?err, "ingestion tick failed; backing off");
                TICK_SLOW
            }
        };
        tokio::time::sleep(wait).await;
    }
}
