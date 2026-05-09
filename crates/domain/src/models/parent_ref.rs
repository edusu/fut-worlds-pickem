use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifies the structural parent of a match: either a group-stage
/// `tournament_group` or a knockout `knockout_phase`. Mirrors the
/// `matches_parent_xor` CHECK in the schema — every match has exactly one.
///
/// Carried on the wire by `messaging::events::*` and on the API surface by
/// `services/api/src/routes/predictions.rs`. Keeping the canonical
/// definition here means producers and consumers agree on the JSON shape
/// without each crate redefining its own variant.
///
/// Serializes as `{"kind": "tournament_group", "id": "<uuid>"}` (struct
/// variants with `tag = "kind"`) for predictable JSON on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentRef {
    TournamentGroup { id: Uuid },
    KnockoutPhase { id: Uuid },
}

impl ParentRef {
    pub fn id(&self) -> Uuid {
        match self {
            Self::TournamentGroup { id } | Self::KnockoutPhase { id } => *id,
        }
    }
}
