//! Wire-format DTOs that mirror what football-data.org actually returns.
//!
//! Conventions:
//!
//! - All structs are `#[serde(rename_all = "camelCase")]` so Rust snake_case
//!   fields map onto the upstream's camelCase keys (`utcDate`, `fullTime`...).
//! - Anything the ingester does not strictly require is `Option<T>` — even
//!   fields that "always" exist according to the docs — because the upstream
//!   occasionally ships partial documents during reschedules.
//! - Enums for `status` and `score.duration` are typed (not raw strings) so
//!   the adapter can pattern-match without re-doing the validation.
//!
//! One submodule per upstream endpoint, plus `common` for fragments shared
//! across endpoints. Tests embed literal JSON payloads alongside the DTOs they
//! exercise so a renamed key breaks the test next to the affected struct.

pub mod common;
pub mod match_data;
pub mod standings;
pub mod teams;

pub use common::TeamRefDto;
pub use match_data::{
    CompetitionMatches, HomeAwayDto, MatchDto, MatchStatusDto, ScoreDto, ScoreDuration, WinnerDto,
};
pub use standings::{CompetitionStandings, StandingDto, StandingsRowDto};
pub use teams::{AreaDto, CompetitionTeams, TeamDto};
