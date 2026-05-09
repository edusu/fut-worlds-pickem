use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::phase::Phase;

/// Lifecycle of a single match. Mirrors the upstream `MatchStatusDto` set
/// 1:1 so the ingester can persist whatever the provider sends without
/// collapsing intermediate states (Timed / Paused / etc.) into broader
/// buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Scheduled,
    Timed,
    InPlay,
    Paused,
    Finished,
    Suspended,
    Postponed,
    Cancelled,
    Awarded,
}

/// A football match. Belongs to exactly one structural parent: either a
/// `tournament_group` (group-stage) or a `knockout_phase` (knockout). The
/// `matches_parent_xor` SQL CHECK enforces this invariant at the DB level;
/// callers in Rust can rely on `phase()` to read it back.
///
/// Team references are nullable because the upstream feed ships knockout
/// fixtures before participants are determined. The ingester populates the
/// FKs when the bracket resolves.
///
/// `external_id` is the upstream provider identifier (e.g.
/// football-data.org match id) and is globally unique across competitions,
/// so it serves as the natural key for idempotent ingestion.
///
/// Score columns model the regulation result; `et_*` columns extend the
/// shape for knockout matches that go beyond 90' (only the goals scored
/// *during* extra time, not aggregate). `pens_winner_team_id` is set only
/// when a penalty shootout was played.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub id: Uuid,
    pub external_id: String,
    pub tournament_group_id: Option<Uuid>,
    pub knockout_phase_id: Option<Uuid>,
    pub home_team_id: Option<Uuid>,
    pub away_team_id: Option<Uuid>,
    pub kickoff_at: DateTime<Utc>,
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub et_home_score: Option<i32>,
    pub et_away_score: Option<i32>,
    pub pens_winner_team_id: Option<Uuid>,
    pub status: MatchStatus,
}

impl Match {
    /// Derive the match phase from which structural FK is set. The
    /// `matches_parent_xor` CHECK guarantees exactly one is `Some`, so this
    /// helper is total in practice; the `Group` fallback covers a
    /// hypothetical row that violated the constraint and should never
    /// reach Rust callers.
    pub fn phase(&self) -> Phase {
        if self.knockout_phase_id.is_some() {
            Phase::Knockout
        } else {
            Phase::Group
        }
    }

    /// Whether this match's actual result went to extra time. Treats both
    /// ET goal counters being NULL as "did not go to ET". Both NULL or both
    /// set is an invariant enforced by the schema.
    pub fn went_to_extra_time(&self) -> bool {
        self.et_home_score.is_some() && self.et_away_score.is_some()
    }

    /// Whether this match's actual result was decided on penalties.
    pub fn went_to_penalties(&self) -> bool {
        self.pens_winner_team_id.is_some()
    }
}
