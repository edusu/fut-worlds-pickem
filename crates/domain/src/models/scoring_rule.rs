use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Configurable point award table for a pickem group.
///
/// All point fields default to the values documented in the scoring spec
/// (group-stage 0/4/10/16/24, knockout ADV=12 / flags=2 / combo=+10,
/// standings 6/4/2/1 +5 combo, best thirds 3 each, repick penalty 25%).
/// `ScoringRule::default_rule()` returns those defaults without touching
/// the database, which lets the scoring tests stay pure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringRule {
    pub id: Uuid,
    pub name: String,

    // Group-stage match (and reused for the regulation 90' bucket of knockouts).
    pub gs_miss_points: i32,
    pub gs_one_team_points: i32,
    pub gs_sign_only_points: i32,
    pub gs_sign_plus_one_points: i32,
    pub gs_exact_points: i32,

    // Knockout-only components.
    pub ko_adv_points: i32,
    pub ko_et_flag_points: i32,
    pub ko_pk_flag_points: i32,
    pub ko_combo_points: i32,

    // Group-standings and best-thirds.
    pub st_pos1_points: i32,
    pub st_pos2_points: i32,
    pub st_pos3_points: i32,
    pub st_pos4_points: i32,
    pub st_full_combo_points: i32,
    pub st_best_third_points: i32,

    /// Percentage subtracted from the awarded match points when the
    /// prediction's `was_changed` flag is true. Stored as an integer 0..=100;
    /// the scorer applies `points * (100 - pct) / 100` and floors.
    pub repick_penalty_pct: i32,
}

impl ScoringRule {
    /// Construct a rule with the documented default values. Useful for
    /// tests and for seeding new pickems before the api offers per-pickem
    /// overrides. `id` and `name` are caller-supplied because they identify
    /// the row, not the scoring math.
    pub fn defaults(id: Uuid, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            gs_miss_points: 0,
            gs_one_team_points: 4,
            gs_sign_only_points: 10,
            gs_sign_plus_one_points: 16,
            gs_exact_points: 24,
            ko_adv_points: 12,
            ko_et_flag_points: 2,
            ko_pk_flag_points: 2,
            ko_combo_points: 10,
            st_pos1_points: 6,
            st_pos2_points: 4,
            st_pos3_points: 2,
            st_pos4_points: 1,
            st_full_combo_points: 5,
            st_best_third_points: 3,
            repick_penalty_pct: 25,
        }
    }
}
