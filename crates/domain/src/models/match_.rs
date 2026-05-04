use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lifecycle of a single match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchStatus {
    Scheduled,
    Live,
    Finished,
    Postponed,
    Cancelled,
}

/// A football match within a round.
///
/// `external_id` is the upstream provider identifier (e.g. football-data.org
/// match id) and is used for idempotent ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub id: Uuid,
    pub round_id: Uuid,
    pub external_id: String,
    pub home_team: String,
    pub away_team: String,
    pub home_flag: String,
    pub away_flag: String,
    pub kickoff_at: DateTime<Utc>,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub status: MatchStatus,
}
