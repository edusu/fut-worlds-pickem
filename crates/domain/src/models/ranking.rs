use serde::{Deserialize, Serialize};

use crate::TelegramUserId;

/// One row of a pickem's leaderboard. Built by aggregating points across the
/// three point sources (match predictions, group standings, best thirds)
/// per user, then joining against `group_members` so every roster entry
/// shows up — members with zero scored predictions render with
/// `total_points = 0` instead of being filtered out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankingEntry {
    pub user_id: TelegramUserId,
    pub first_name: String,
    pub total_points: i64,
}
