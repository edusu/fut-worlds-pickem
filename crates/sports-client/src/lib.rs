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
