//! Scoring rules and point calculation.
//!
//! This module is the only place that should encode "how points are awarded".
//! Keeping it here (and pure) lets us unit-test it without a database and
//! reuse it from both the events scorer and any preview/dry-run feature.

use crate::{Match, Prediction, ScoringRule};

/// Calculate the points awarded to `prediction` given the final `match_` and
/// the active `rule`.
///
/// The match must be in `Finished` status with both scores set; otherwise the
/// caller should not invoke this function.
pub fn calculate_points(_prediction: &Prediction, _match_: &Match, _rule: &ScoringRule) -> i32 {
    // TODO: Implement the point award table:
    //   - exact score          -> rule.exact_score_points
    //   - correct goal diff    -> rule.correct_diff_points
    //   - correct sign (1X2)   -> rule.correct_sign_points
    //   - miss                 -> rule.miss_points
    todo!("calculate_points: implement scoring per rule table")
}
