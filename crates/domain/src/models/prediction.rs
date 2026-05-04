use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::user::TelegramUserId;

/// A user's prediction for a single match.
///
/// `points_awarded` is `None` until the match is scored, allowing us to tell
/// "not yet scored" apart from "zero points".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    pub id: Uuid,
    pub user_id: TelegramUserId,
    pub match_id: Uuid,
    pub predicted_home: i32,
    pub predicted_away: i32,
    pub points_awarded: Option<i32>,
    pub submitted_at: DateTime<Utc>,
}
