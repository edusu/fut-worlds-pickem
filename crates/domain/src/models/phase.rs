use serde::{Deserialize, Serialize};

/// Stage of the tournament a round / match belongs to. Drives which scoring
/// function the events scorer dispatches to: `Group` → 5-bucket multiplicative
/// table; `Knockout` → reg-bucket + advancement + ET/PK flags + combo bonus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Group,
    Knockout,
}
