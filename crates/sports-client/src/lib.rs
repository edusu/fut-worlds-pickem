//! Thin wrapper around football-data.org (or any equivalent) used by the
//! `events` service to ingest fixtures and results.
//!
//! Keeping the upstream API behind a typed client lets us swap providers
//! later without touching ingestion logic. The crate is laid out as:
//!
//! - `client` — the rate-limited, retrying HTTP client and its public methods.
//! - `dto` — wire-format types organised one submodule per upstream endpoint,
//!   with literal-JSON tests next to the structs they exercise.
//! - `error` — typed errors and the `error-stack` report alias.

pub mod client;
pub mod dto;
pub mod error;

// Flat re-exports so callers can keep using `sports_client::MatchDto`, etc.
pub use client::Client;
pub use dto::{
    AreaDto, CompetitionMatches, CompetitionStandings, CompetitionTeams, HomeAwayDto, MatchDto,
    MatchStatusDto, ScoreDto, ScoreDuration, StandingDto, StandingsRowDto, TeamDto, TeamRefDto,
    WinnerDto,
};
pub use error::{SportsClientError, SportsClientReport, SportsResult};

/// Trait carved out of [`Client`] for the operations the events
/// scorer needs at runtime. Exists to let integration tests inject a
/// fake without spinning up an HTTP mock; the real `Client` carries
/// the production `reqwest` + retry + rate-limit stack.
///
/// Only the methods the scorer touches today are surfaced. If a
/// future scorer responsibility needs another endpoint, add it here
/// alongside the implementation on `Client`.
#[async_trait::async_trait]
pub trait SportsClientLike: Send + Sync {
    /// `/competitions/{code}/standings` — used by
    /// `dispatch_group_standings` and `dispatch_best_thirds`.
    async fn get_competition_standings(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionStandings>;
}

#[async_trait::async_trait]
impl SportsClientLike for Client {
    async fn get_competition_standings(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionStandings> {
        Client::get_competition_standings(self, competition_code).await
    }
}
