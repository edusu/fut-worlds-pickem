use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stage of the knockout bracket. Mirrors the upstream `stage` strings
/// emitted by football-data.org for World Cup 2026 fixtures (Round of 32
/// through Final). Persisted to the DB as the SCREAMING_SNAKE_CASE form so
/// the column value matches the upstream code one-to-one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KnockoutStage {
    Last32,
    Last16,
    QuarterFinals,
    SemiFinals,
    ThirdPlace,
    Final,
}

impl KnockoutStage {
    /// Bracket position used to drive `ORDER BY position` in queries. Stable
    /// across tournaments: 1 = Round of 32 ... 6 = Final, with Third Place
    /// slotted just before the Final to match the natural narrative order.
    pub fn position(self) -> i16 {
        match self {
            Self::Last32 => 1,
            Self::Last16 => 2,
            Self::QuarterFinals => 3,
            Self::SemiFinals => 4,
            Self::ThirdPlace => 5,
            Self::Final => 6,
        }
    }

    /// Spanish display name used by the Mini App. Matches the v1 product
    /// language; localisation to other languages is a future task that
    /// would extend `knockout_phases.display_name` per-row.
    pub fn display_name_es(self) -> &'static str {
        match self {
            Self::Last32 => "Octavos de 32",
            Self::Last16 => "Octavos",
            Self::QuarterFinals => "Cuartos",
            Self::SemiFinals => "Semis",
            Self::ThirdPlace => "Tercer puesto",
            Self::Final => "Final",
        }
    }

    /// Stable upstream code used in the DB `stage` column.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Last32 => "LAST_32",
            Self::Last16 => "LAST_16",
            Self::QuarterFinals => "QUARTER_FINALS",
            Self::SemiFinals => "SEMI_FINALS",
            Self::ThirdPlace => "THIRD_PLACE",
            Self::Final => "FINAL",
        }
    }

    /// Parse the upstream / DB form back into the enum. Returns `None` for
    /// any string outside the closed set so callers can warn-and-skip on
    /// unrecognised stages instead of panicking.
    pub fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "LAST_32" => Self::Last32,
            "LAST_16" => Self::Last16,
            "QUARTER_FINALS" => Self::QuarterFinals,
            "SEMI_FINALS" => Self::SemiFinals,
            "THIRD_PLACE" => Self::ThirdPlace,
            "FINAL" => Self::Final,
            _ => return None,
        })
    }
}

/// Lifecycle of a knockout phase: open for predictions, closed at deadline,
/// scored once every match in the phase has a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnockoutPhaseState {
    Open,
    Closed,
    Scored,
}

/// A knockout-stage container with its own submission deadline and
/// lifecycle. Replaces the previous "single knockouts round" — every
/// match in the knockout bracket points at one row of this table via
/// `matches.knockout_phase_id`.
///
/// `position` is denormalised for cheap bracket rendering. The seed
/// guarantees `position == stage.position()` so callers can rely on the
/// column for `ORDER BY` without a CASE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnockoutPhase {
    pub id: Uuid,
    pub tournament_id: Uuid,
    pub stage: KnockoutStage,
    pub position: i16,
    pub display_name: String,
    pub deadline_at: DateTime<Utc>,
    pub state: KnockoutPhaseState,
    pub created_at: DateTime<Utc>,
}
