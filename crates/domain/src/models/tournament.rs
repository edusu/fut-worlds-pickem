use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level competition the ingester pulls fixtures for, e.g. the 2026
/// World Cup. Distinct from `TournamentGroup`, which models the 4-team
/// blocks *inside* a tournament (Group A, Group B, ...).
///
/// `external_id` is the upstream provider's competition code (football-data
/// uses `WC` for the World Cup). It is the natural key callers use to look
/// the row up at boot before they have a UUID in hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tournament {
    pub id: Uuid,
    pub name: String,
    pub external_id: String,
    pub created_at: DateTime<Utc>,
}
