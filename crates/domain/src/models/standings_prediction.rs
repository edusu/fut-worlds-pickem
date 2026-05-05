use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::user::TelegramUserId;

/// A user's prediction of the final ordering (1st..4th) of a single
/// World Cup group within a specific pickem.
///
/// The four `pos*_team_id` fields are guaranteed distinct at the database
/// level via a CHECK constraint. `points_awarded` stays `None` until the
/// scorer runs at end of group stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupStandingsPrediction {
    pub id: Uuid,
    pub user_id: TelegramUserId,
    pub pickem_group_id: Uuid,
    pub tournament_group_id: Uuid,
    pub pos1_team_id: Uuid,
    pub pos2_team_id: Uuid,
    pub pos3_team_id: Uuid,
    pub pos4_team_id: Uuid,
    pub points_awarded: Option<i32>,
    pub submitted_at: DateTime<Utc>,
}

/// A user's prediction of which 8 teams will be the "best thirds"
/// advancing past the group stage. Set semantics: order doesn't matter,
/// duplicates aren't allowed (DB-enforced via PK on `team_id`).
///
/// At lock time exactly 8 picks are required. The api enforces this; the
/// scorer treats `< 8` as a partial submission and scores only the picks
/// that exist (each correct pick = `st_best_third_points`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BestThirdsPrediction {
    pub user_id: TelegramUserId,
    pub pickem_group_id: Uuid,
    pub team_ids: Vec<Uuid>,
}
