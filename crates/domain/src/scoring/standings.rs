//! Group-standings and best-thirds scoring.
//!
//! Group-standings: per WC group of 4 teams, the user submits an ordered
//! tuple `(pos1, pos2, pos3, pos4)`. Each position scores independently;
//! getting all four right unlocks an additional combo bonus.
//!
//! Best thirds: per pickem, the user submits an unordered set of 8 team
//! ids. Score = `st_best_third_points` per team in the user's set that is
//! also in the set of 8 actual best thirds (set intersection size × per-team
//! value). Order doesn't matter.
//!
//! Standings are not subject to the re-pick penalty — there's no
//! `was_changed` field on standings predictions, and the api freezes them
//! at the deadline of the first group-stage match.

use std::collections::HashSet;

use uuid::Uuid;

use crate::{BestThirdsPrediction, GroupStandingsPrediction, ScoringRule};

/// Score a single group-standings prediction against the actual ordering of
/// the 4 teams in that group. `actual` is `[1st, 2nd, 3rd, 4th]`.
pub fn score_group_standings(
    prediction: &GroupStandingsPrediction,
    actual: &[Uuid; 4],
    rule: &ScoringRule,
) -> i32 {
    let p1 = prediction.pos1_team_id == actual[0];
    let p2 = prediction.pos2_team_id == actual[1];
    let p3 = prediction.pos3_team_id == actual[2];
    let p4 = prediction.pos4_team_id == actual[3];

    let mut total = 0;
    if p1 {
        total += rule.st_pos1_points;
    }
    if p2 {
        total += rule.st_pos2_points;
    }
    if p3 {
        total += rule.st_pos3_points;
    }
    if p4 {
        total += rule.st_pos4_points;
    }
    if p1 && p2 && p3 && p4 {
        total += rule.st_full_combo_points;
    }
    total
}

/// Score a best-thirds prediction against the actual set of 8 best thirds.
///
/// Each correctly-picked team yields `rule.st_best_third_points`. Picks not
/// in the actual set yield 0. The scorer does not penalize over-picking;
/// the api enforces "exactly 8 picks at lock time".
pub fn score_best_thirds(
    prediction: &BestThirdsPrediction,
    actual: &HashSet<Uuid>,
    rule: &ScoringRule,
) -> i32 {
    let hits = prediction
        .team_ids
        .iter()
        .filter(|t| actual.contains(*t))
        .count();
    rule.st_best_third_points * hits as i32
}
