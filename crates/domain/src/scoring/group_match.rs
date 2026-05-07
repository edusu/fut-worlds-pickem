//! Group-stage match scoring.
//!
//! The 5-bucket multiplicative-feel table from the spec:
//!
//! | Bucket           | (H, A, R)         | Default points |
//! |------------------|-------------------|----------------|
//! | miss             | (0,0,0)           | 0              |
//! | one_team_only    | (1,0,0) / (0,1,0) | 4              |
//! | sign_only        | (0,0,1)           | 10             |
//! | sign_plus_one    | (1,0,1) / (0,1,1) | 16             |
//! | exact            | (1,1,1)           | 24             |
//!
//! Where:
//!   H = predicted home goals == actual home goals
//!   A = predicted away goals == actual away goals
//!   R = sign(predicted_home - predicted_away) == sign(actual_home - actual_away)
//!
//! Note that H ∧ A implies R, so only 7 of the 8 combinations are reachable.
//!
//! The function is also reused by the knockout scorer to score the
//! regulation 90' bucket — see [`super::knockout_match`].

use crate::{Match, Prediction, ScoringRule};

use super::{apply_repick_penalty, ScoringError, ScoringResult};

/// Compute the 5-bucket point value for a regulation 90' (or group-stage)
/// score line, **without** applying the re-pick penalty.
///
/// Exposed `pub(crate)` so the knockout scorer can compose this directly
/// when summing the reg-bucket into the total.
pub(crate) fn raw_reg_bucket(
    predicted_home: i32,
    predicted_away: i32,
    actual_home: i32,
    actual_away: i32,
    rule: &ScoringRule,
) -> i32 {
    let h = predicted_home == actual_home;
    let a = predicted_away == actual_away;
    let r = (predicted_home - predicted_away).signum() == (actual_home - actual_away).signum();

    match (h, a, r) {
        (true, true, _) => rule.gs_exact_points,
        (true, false, true) | (false, true, true) => rule.gs_sign_plus_one_points,
        (false, false, true) => rule.gs_sign_only_points,
        (true, false, false) | (false, true, false) => rule.gs_one_team_points,
        (false, false, false) => rule.gs_miss_points,
    }
}

/// Score a single group-stage match prediction.
///
/// Returns the awarded points after applying the re-pick penalty when the
/// prediction's `was_changed` flag is true.
///
/// Returns `ScoringError::MissingMatchData` if the match's regulation
/// `home_score` / `away_score` aren't both populated. The caller (events
/// scorer) is expected to only invoke this once a `MatchFinished` event has
/// been received, so this is a defensive check — not a routine path.
pub fn score_group_match(
    prediction: &Prediction,
    m: &Match,
    rule: &ScoringRule,
) -> ScoringResult<i32> {
    prediction
        .is_consistent()
        .map_err(ScoringError::InconsistentPrediction)?;

    let (Some(actual_home), Some(actual_away)) = (m.home_score, m.away_score) else {
        return Err(ScoringError::MissingMatchData(
            "regulation home/away score must be set",
        ));
    };

    let raw = raw_reg_bucket(
        prediction.reg_home,
        prediction.reg_away,
        actual_home,
        actual_away,
        rule,
    );

    Ok(apply_repick_penalty(raw, prediction.was_changed, rule))
}
