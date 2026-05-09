//! Knockout-stage match scoring.
//!
//! Components (additive, with one combo bonus):
//!
//! | Component                                                   | Default |
//! |-------------------------------------------------------------|---------|
//! | Advancement winner correct (binary)                         | 12      |
//! | Regulation 90' goals — same 5-bucket table as group stage   | 0..24   |
//! | "Goes to extra time" flag (implicit from reg-draw)          | 2       |
//! | "Goes to penalties" flag (implicit from pens_winner field)  | 2       |
//! | Combo: ADV ✓ AND reg=exact AND ET-flag ✓ AND PK-flag ✓      | +10     |
//!
//! Per-match max: 50. The re-pick penalty is applied to the **total**, not
//! to each component, matching the user-facing intent ("you keep 75% of
//! whatever points you'd otherwise have earned for this match").
//!
//! For non-draw regulation predictions the api will normally leave
//! `advancement_winner_team_id` set to `Some(reg_winner)` so the scorer
//! doesn't need to know about derivation. If it's `None` *and* the reg
//! prediction is non-draw, we derive on the fly using the `home_team_id`
//! and `away_team_id` parameters passed in. The `Match` struct carries
//! these as `Option<Uuid>` (nullable for unresolved knockouts), but by
//! scoring time the bracket has resolved so the api passes the resolved
//! UUIDs explicitly. Keeping them as parameters keeps this function pure
//! (no team-id lookups); the api is responsible for setting
//! `advancement_winner_team_id` correctly for non-draw reg predictions
//! before persisting — here we only validate it, returning
//! `MissingAdvancementWinner` if it isn't set.

use uuid::Uuid;

use crate::{Match, Prediction, ScoringRule};

use super::{apply_repick_penalty, group_match::raw_reg_bucket, ScoringError, ScoringResult};

/// Score a single knockout-stage match prediction.
///
/// `home_team_id` and `away_team_id` are the team UUIDs for the match's
/// home and away sides. `Match` carries these as `Option<Uuid>` (nullable
/// for unresolved knockouts), but by scoring time the bracket has
/// resolved so the caller can unwrap and pass them in explicitly. This
/// keeps the scoring function pure and self-contained.
pub fn score_knockout_match(
    prediction: &Prediction,
    m: &Match,
    home_team_id: Uuid,
    away_team_id: Uuid,
    rule: &ScoringRule,
) -> ScoringResult<i32> {
    prediction
        .is_consistent()
        .map_err(ScoringError::InconsistentPrediction)?;

    let (Some(actual_reg_home), Some(actual_reg_away)) = (m.home_score, m.away_score) else {
        return Err(ScoringError::MissingMatchData(
            "regulation home/away score must be set",
        ));
    };

    // Derive both predicted and actual paths.
    let predicted_goes_to_et = prediction.predicted_goes_to_et();
    let predicted_goes_to_pens = prediction.predicted_goes_to_pens();
    let actual_goes_to_et = m.went_to_extra_time();
    let actual_goes_to_pens = m.went_to_penalties();

    // Identify the actual advancement winner. If the match went to pens,
    // the pens winner is the advancement winner. Otherwise the team with
    // the higher cumulative goals (regulation + ET if any) advances.
    let actual_advancement = match m.pens_winner_team_id {
        Some(team_id) => team_id,
        None => {
            let total_home = actual_reg_home + m.et_home_score.unwrap_or(0);
            let total_away = actual_reg_away + m.et_away_score.unwrap_or(0);
            if total_home > total_away {
                home_team_id
            } else if total_away > total_home {
                away_team_id
            } else {
                // A knockout match cannot finish tied without a pens winner.
                return Err(ScoringError::MissingMatchData(
                    "knockout match has no winner: scores tied with no pens_winner_team_id",
                ));
            }
        }
    };

    // Determine the predicted advancement winner. For draw reg predictions
    // the user must supply it; for non-draw reg predictions either the api
    // populated it (preferred) or we derive it from the predicted reg score.
    let predicted_advancement = match (
        prediction.advancement_winner_team_id,
        prediction.predicted_reg_draw(),
    ) {
        (Some(team_id), _) => team_id,
        (None, false) => {
            if prediction.reg_home > prediction.reg_away {
                home_team_id
            } else {
                away_team_id
            }
        }
        (None, true) => return Err(ScoringError::MissingAdvancementWinner),
    };

    // === Component scoring ===

    let reg_bucket_pts = raw_reg_bucket(
        prediction.reg_home,
        prediction.reg_away,
        actual_reg_home,
        actual_reg_away,
        rule,
    );

    let adv_correct = predicted_advancement == actual_advancement;
    let adv_pts = if adv_correct { rule.ko_adv_points } else { 0 };

    let et_flag_correct = predicted_goes_to_et == actual_goes_to_et;
    let et_flag_pts = if et_flag_correct {
        rule.ko_et_flag_points
    } else {
        0
    };

    let pk_flag_correct = predicted_goes_to_pens == actual_goes_to_pens;
    let pk_flag_pts = if pk_flag_correct {
        rule.ko_pk_flag_points
    } else {
        0
    };

    // Combo bonus: every atomic outcome correct AND reg-bucket hit the
    // exact-score row of the table. We compare against `gs_exact_points` to
    // detect the exact bucket without a separate flag, so callers customizing
    // the bucket values still see a consistent definition of "exact".
    let reg_was_exact = reg_bucket_pts == rule.gs_exact_points;
    let combo_unlocked = adv_correct && reg_was_exact && et_flag_correct && pk_flag_correct;
    let combo_pts = if combo_unlocked {
        rule.ko_combo_points
    } else {
        0
    };

    let total = reg_bucket_pts + adv_pts + et_flag_pts + pk_flag_pts + combo_pts;

    Ok(apply_repick_penalty(total, prediction.was_changed, rule))
}
