//! Concrete `sqlx`-backed repository implementations, one module per
//! aggregate. Each adapter is a thin wrapper around `PgPool` so the same pool
//! can be shared across many repositories within a service.

pub mod groups;
pub mod knockout_phases;
mod mappers;
pub mod matches;
pub mod predictions;
pub mod scoring_rules;
pub mod teams;
pub mod tournament_groups;
pub mod tournaments;
pub mod users;

pub use groups::PgGroupRepository;
pub use knockout_phases::PgKnockoutPhaseRepository;
pub use matches::PgMatchRepository;
pub use predictions::PgPredictionRepository;
pub use scoring_rules::PgScoringRuleRepository;
pub use teams::PgTeamRepository;
pub use tournament_groups::PgTournamentGroupRepository;
pub use tournaments::PgTournamentRepository;
pub use users::PgUserRepository;
