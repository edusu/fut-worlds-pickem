use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Configurable point award table for a pickem group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringRule {
    pub id: Uuid,
    pub name: String,
    pub exact_score_points: i32,
    pub correct_diff_points: i32,
    pub correct_sign_points: i32,
    pub miss_points: i32,
}
