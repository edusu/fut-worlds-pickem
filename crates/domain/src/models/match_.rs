use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::phase::Phase;

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
///
/// Score columns are split between regulation 90' (`home_score` /
/// `away_score`) and extra time (`et_home_score` / `et_away_score` — only
/// the goals scored *during* extra time, not aggregate). `pens_winner_team_id`
/// is set only when a penalty shootout was played.
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
    pub et_home_score: Option<i32>,
    pub et_away_score: Option<i32>,
    pub pens_winner_team_id: Option<Uuid>,
    pub status: MatchStatus,
    pub phase: Phase,
}

impl Match {
    /// Whether this match's actual result went to extra time. Treats both
    /// ET goal counters being NULL as "did not go to ET". Both NULL or both
    /// set is an invariant enforced by the schema.
    pub fn went_to_extra_time(&self) -> bool {
        self.et_home_score.is_some() && self.et_away_score.is_some()
    }

    /// Whether this match's actual result was decided on penalties.
    pub fn went_to_penalties(&self) -> bool {
        self.pens_winner_team_id.is_some()
    }
}
