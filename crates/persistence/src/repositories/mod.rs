//! Concrete `sqlx`-backed repository implementations, one module per
//! aggregate. Each adapter is a thin wrapper around `PgPool` so the same pool
//! can be shared across many repositories within a service.

pub mod groups;
pub mod matches;
pub mod predictions;
pub mod rounds;
pub mod scoring_rules;
pub mod teams;
pub mod tournament_groups;
pub mod users;

pub use groups::PgGroupRepository;
pub use matches::PgMatchRepository;
pub use predictions::PgPredictionRepository;
pub use rounds::PgRoundRepository;
pub use scoring_rules::PgScoringRuleRepository;
pub use teams::PgTeamRepository;
pub use tournament_groups::PgTournamentGroupRepository;
pub use users::PgUserRepository;
