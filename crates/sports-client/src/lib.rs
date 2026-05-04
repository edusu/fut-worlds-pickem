//! Thin wrapper around football-data.org (or any equivalent) used by the
//! `events` service to ingest fixtures and results.
//!
//! Keeping the upstream API behind a typed client lets us swap providers
//! later without touching ingestion logic.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SportsClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("upstream returned status {0}")]
    Upstream(u16),
}

pub type SportsResult<T> = Result<T, SportsClientError>;

/// Football-data.org API client.
///
/// The API key is sent on the `X-Auth-Token` header on every request. The
/// underlying `reqwest::Client` is shared and cheap to clone.
pub struct Client {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl Client {
    /// Build a new client. `base_url` defaults to the public football-data.org
    /// endpoint; tests can swap it for a local mock.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.football-data.org/v4".to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Fetch all fixtures for a given competition (e.g. "WC" for FIFA World Cup).
    pub async fn get_competition_matches(
        &self,
        _competition_code: &str,
    ) -> SportsResult<CompetitionMatches> {
        // TODO: GET {base_url}/competitions/{competition_code}/matches with auth header
        let _ = (&self.http, &self.api_key, &self.base_url);
        todo!("Client::get_competition_matches")
    }

    /// Fetch a single match by its upstream id.
    pub async fn get_match_by_id(&self, _match_id: &str) -> SportsResult<MatchDto> {
        // TODO: GET {base_url}/matches/{match_id}
        let _ = (&self.http, &self.api_key, &self.base_url);
        todo!("Client::get_match_by_id")
    }
}

/// Subset of upstream payload we care about. Extend as ingestion needs grow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitionMatches {
    pub matches: Vec<MatchDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchDto {
    pub id: i64,
    pub home_team: TeamDto,
    pub away_team: TeamDto,
    pub status: String,
    pub utc_date: String,
    pub score: Option<ScoreDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamDto {
    pub name: String,
    pub tla: Option<String>,
    pub crest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreDto {
    pub full_time: Option<FullTimeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTimeDto {
    pub home: Option<i32>,
    pub away: Option<i32>,
}
