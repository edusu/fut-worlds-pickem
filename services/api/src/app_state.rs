//! Shared runtime state injected into every Axum handler.
//!
//! Mirrors `services/bot/src/app_state.rs` — every aggregate is exposed
//! through the domain trait behind `Arc<dyn ...>`, so handlers depend on
//! ports rather than concrete `Pg*Repository` types.

use std::sync::Arc;

use domain::repository::{
    BestThirdsPredictionRepository, GroupRepository, KnockoutPhaseRepository, MatchRepository,
    PredictionRepository, StandingsPredictionRepository, TeamRepository, TournamentGroupRepository,
    TournamentRepository,
};
use persistence::{
    PgBestThirdsPredictionRepository, PgGroupRepository, PgKnockoutPhaseRepository,
    PgMatchRepository, PgPredictionRepository, PgStandingsPredictionRepository, PgTeamRepository,
    PgTournamentGroupRepository, PgTournamentRepository,
};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub groups: Arc<dyn GroupRepository>,
    pub tournaments: Arc<dyn TournamentRepository>,
    pub tournament_groups: Arc<dyn TournamentGroupRepository>,
    pub knockout_phases: Arc<dyn KnockoutPhaseRepository>,
    pub matches: Arc<dyn MatchRepository>,
    pub teams: Arc<dyn TeamRepository>,
    pub predictions: Arc<dyn PredictionRepository>,
    pub standings: Arc<dyn StandingsPredictionRepository>,
    pub best_thirds: Arc<dyn BestThirdsPredictionRepository>,
}

impl AppState {
    /// Build the production state from a live Postgres pool. `PgPool` is
    /// internally `Arc`-shared, so cloning it into each repository does not
    /// multiply real connections.
    pub fn new(pool: PgPool) -> Self {
        Self {
            groups: Arc::new(PgGroupRepository::new(pool.clone())),
            tournaments: Arc::new(PgTournamentRepository::new(pool.clone())),
            tournament_groups: Arc::new(PgTournamentGroupRepository::new(pool.clone())),
            knockout_phases: Arc::new(PgKnockoutPhaseRepository::new(pool.clone())),
            matches: Arc::new(PgMatchRepository::new(pool.clone())),
            teams: Arc::new(PgTeamRepository::new(pool.clone())),
            predictions: Arc::new(PgPredictionRepository::new(pool.clone())),
            standings: Arc::new(PgStandingsPredictionRepository::new(pool.clone())),
            best_thirds: Arc::new(PgBestThirdsPredictionRepository::new(pool)),
        }
    }
}
