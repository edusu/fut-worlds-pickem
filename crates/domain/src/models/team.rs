use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A team participating in the tournament.
///
/// Teams are the unit of identity used by all prediction types: match
/// predictions reference team ids via `advancement_winner_team_id` and
/// `pens_winner_team_id`, group-standings predictions reference team ids in
/// `pos1..pos4`, and best-thirds predictions are sets of team ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub flag_emoji: String,
    pub country_code: String,
}
