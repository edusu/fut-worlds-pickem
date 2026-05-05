//! Re-pick penalty: flat percentage applied to a match's awarded points
//! when the prediction's `was_changed` flag is true. Standings predictions
//! do not use this — they have no `was_changed` field.

use crate::ScoringRule;

/// Apply the re-pick penalty to `points` if `was_changed` is true.
///
/// The penalty is a percentage (e.g. `repick_penalty_pct = 25` means keep
/// 75%). Computed as integer arithmetic with truncation toward zero, which
/// for non-negative `points` is identical to flooring. For example,
/// `apply_repick_penalty(50, true, &rule_with_25_pct) == 37` (`50 * 75 / 100
/// = 37` after truncation, matching `floor(37.5) = 37`).
///
/// Multiple revisions don't compound — the boolean only encodes "ever
/// changed", so two edits and ten edits incur the same single penalty.
pub fn apply_repick_penalty(points: i32, was_changed: bool, rule: &ScoringRule) -> i32 {
    if !was_changed {
        return points;
    }
    let kept_pct = (100 - rule.repick_penalty_pct).max(0);
    // i32 arithmetic is fine: `points` is bounded by per-match ceilings that
    // never approach overflow.
    points * kept_pct / 100
}
