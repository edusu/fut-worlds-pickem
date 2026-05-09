use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lifecycle of a tournament group: open for predictions, closed at
/// deadline, scored once every match in the group has a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TournamentGroupState {
    Open,
    Closed,
    Scored,
}

/// A World Cup group (e.g. "Group A"). Distinct from the `Group` aggregate,
/// which represents a pickem bound to a Telegram chat. The naming
/// disambiguation matters: `Group` = pickem; `TournamentGroup` = the 4-team
/// stage block in the actual competition.
///
/// Carries its own submission deadline and lifecycle: every match in this
/// group points at this row via `matches.tournament_group_id`. In v1 all
/// 12 groups share the same `deadline_at` value (the first group-stage
/// kickoff), but the schema is shaped so per-group deadlines can be set
/// later without migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentGroup {
    pub id: Uuid,
    pub tournament_id: Uuid,
    pub name: String,
    pub deadline_at: DateTime<Utc>,
    pub state: TournamentGroupState,
    pub created_at: DateTime<Utc>,
}

/// Many-to-many association between a tournament group and the teams that
/// belong to it. The schema enforces "exactly one group per team per
/// tournament" via the composite PK `(tournament_id, team_id)`; this type
/// carries all three identifiers so callers cannot construct an
/// inconsistent assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TournamentGroupAssignment {
    pub tournament_id: Uuid,
    pub tournament_group_id: Uuid,
    pub team_id: Uuid,
}
