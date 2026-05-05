use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A World Cup group (e.g. "Group A"). Distinct from the `Group` aggregate,
/// which represents a pickem bound to a Telegram chat. The naming
/// disambiguation matters: `Group` = pickem; `TournamentGroup` = the 4-team
/// stage block in the actual competition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentGroup {
    pub id: Uuid,
    pub name: String,
}

/// Many-to-many association between a tournament group and the teams that
/// belong to it. The schema enforces single-group membership for a team
/// (unique on `team_id`), but the type stays generic to leave room for
/// future tournaments where this constraint might not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentGroupAssignment {
    pub tournament_group_id: Uuid,
    pub team_id: Uuid,
}
