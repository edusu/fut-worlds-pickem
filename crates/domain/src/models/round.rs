use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::phase::Phase;

/// Lifecycle of a round of predictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoundState {
    Open,
    Closed,
    Scored,
}

/// A round groups one or more matches under a single submission deadline.
///
/// Rounds belong to a tournament, not to a Telegram pickem — every pickem
/// shares the same set of rounds for a given competition. Pickem-specific
/// state (members, scoring rule) lives on `Group`; tournament-wide state
/// (deadlines, phase progression) lives here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub id: Uuid,
    pub tournament_id: Uuid,
    pub name: String,
    pub deadline_at: DateTime<Utc>,
    pub state: RoundState,
    pub phase: Phase,
    pub created_at: DateTime<Utc>,
}
