//! Versioned event payloads.
//!
//! Every event enum has a `V1` variant from day one so adding `V2` later is a
//! backwards-compatible change: subscribers can match on the variant they
//! understand and ignore newer ones. Never delete or repurpose a variant —
//! add a new one and migrate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum MatchFinished {
    V1(MatchFinishedV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchFinishedV1 {
    pub match_id: Uuid,
    pub round_id: Uuid,
    pub external_id: String,
    pub home_score: i32,
    pub away_score: i32,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum MatchLive {
    V1(MatchLiveV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchLiveV1 {
    pub match_id: Uuid,
    pub round_id: Uuid,
    pub kicked_off_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum RoundDeadlineApproaching {
    V1(RoundDeadlineApproachingV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundDeadlineApproachingV1 {
    pub round_id: Uuid,
    pub group_id: Uuid,
    pub deadline_at: DateTime<Utc>,
    pub minutes_remaining: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum RoundClosed {
    V1(RoundClosedV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundClosedV1 {
    pub round_id: Uuid,
    pub group_id: Uuid,
    pub closed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum RoundScored {
    V1(RoundScoredV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundScoredV1 {
    pub round_id: Uuid,
    pub group_id: Uuid,
    pub scored_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum PredictionsSubmitted {
    V1(PredictionsSubmittedV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionsSubmittedV1 {
    pub user_id: i64,
    pub round_id: Uuid,
    pub group_id: Uuid,
    pub submitted_at: DateTime<Utc>,
    pub prediction_count: usize,
}

/// Outbound notifications. The `bot` service is the sole consumer; any other
/// service can produce one to ask the bot to deliver a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum NotificationRequested {
    V1(NotificationRequestedV1),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRequestedV1 {
    /// Telegram chat id (group or DM). Negative for groups.
    pub chat_id: i64,
    pub template: NotificationTemplate,
    pub requested_at: DateTime<Utc>,
}

/// Body templates the bot knows how to render.
///
/// New notification kinds extend this enum — the bot must learn how to render
/// each variant before producers start emitting it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationTemplate {
    RoundDeadlineSoon {
        round_id: Uuid,
        group_id: Uuid,
        deadline_at: DateTime<Utc>,
        minutes_remaining: i64,
    },
    RoundClosed {
        round_id: Uuid,
        group_id: Uuid,
    },
    MatchResult {
        match_id: Uuid,
        home_team: String,
        away_team: String,
        home_score: i32,
        away_score: i32,
    },
    RankingUpdate {
        group_id: Uuid,
        round_id: Uuid,
    },
}
