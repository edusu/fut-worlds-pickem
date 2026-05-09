//! Periodic ingestion driver.
//!
//! `run` is the long-lived entry point invoked from `services/events/main`.
//! It bootstraps the tournament-wide state (tournament row + tournament
//! groups + knockout phases) once, then loops forever calling
//! `poll::poll_once`. The cadence between ticks adapts based on the
//! previous tick's `TickReport`:
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
use domain::repository::TeamRepository;
use messaging::Publisher;
use persistence::repositories::{
    PgKnockoutPhaseRepository, PgMatchRepository, PgTeamRepository, PgTournamentGroupRepository,
    PgTournamentRepository,
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
    let group_repo = PgTournamentGroupRepository::new(pool.clone());
    let phase_repo = PgKnockoutPhaseRepository::new(pool.clone());
    let match_repo = PgMatchRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());
    let publisher = Publisher::new(nats);
    let client = SportsClient::new(config.football_api_key.clone())
        .map_err(shared::report_to_anyhow)
        .context("constructing sports-data client")?;

    let tournament = bootstrap::ensure_tournament(&tournament_repo).await?;
    let structure =
        bootstrap::ensure_tournament_structure(&client, &group_repo, &phase_repo, &tournament)
            .await?;

    // Load teams once after bootstrap. The set is stable for the whole run
    // (the seed CLI populates `teams` before the service starts and does
    // not append later); a service restart picks up any future changes.
    let teams = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading teams for resolver index")?;
    let team_index = poll::build_team_index(&teams);

    let ctx = poll::IngestContext {
        tournament,
        group_index: structure.group_index,
        knockout_index: structure.knockout_index,
        team_index,
    };

    info!(
        tournament_id = %ctx.tournament.id,
        group_count = ctx.group_index.len(),
        knockout_count = ctx.knockout_index.len(),
        team_count = ctx.team_index.len(),
        "ingester ready, starting periodic loop"
    );

    // First tick fires immediately so a fresh boot does not idle for the
    // whole TICK_SLOW window before doing anything useful.
    loop {
        let wait = match poll::poll_once(&client, &match_repo, &publisher, &ctx).await {
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
