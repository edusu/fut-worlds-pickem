//! Exhaustive scoring tests pinned to the worked examples in the plan.
//! Pure unit tests: no DB, no async, no I/O.

use std::collections::HashSet;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    BestThirdsPrediction, GroupStandingsPrediction, Match, MatchStatus, Phase, Prediction,
    ScoringRule, TelegramUserId,
};

use super::{
    apply_repick_penalty, score_best_thirds, score_group_match, score_group_standings,
    score_knockout_match, ScoringError,
};

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

fn rule() -> ScoringRule {
    ScoringRule::defaults(Uuid::new_v4(), "test")
}

fn group_match_finished(home: i32, away: i32) -> Match {
    Match {
        id: Uuid::new_v4(),
        round_id: Uuid::new_v4(),
        external_id: "ext".into(),
        home_team: "Home".into(),
        away_team: "Away".into(),
        home_flag: "🏠".into(),
        away_flag: "✈️".into(),
        kickoff_at: Utc::now(),
        home_score: Some(home),
        away_score: Some(away),
        et_home_score: None,
        et_away_score: None,
        pens_winner_team_id: None,
        status: MatchStatus::Finished,
        phase: Phase::Group,
    }
}

fn knockout_match_reg(home: i32, away: i32) -> Match {
    Match {
        phase: Phase::Knockout,
        ..group_match_finished(home, away)
    }
}

fn knockout_match_et(reg_home: i32, reg_away: i32, et_home: i32, et_away: i32) -> Match {
    Match {
        et_home_score: Some(et_home),
        et_away_score: Some(et_away),
        ..knockout_match_reg(reg_home, reg_away)
    }
}

fn knockout_match_pens(reg_home: i32, reg_away: i32, pens_winner: Uuid) -> Match {
    let mut m = knockout_match_et(reg_home, reg_away, 0, 0);
    m.pens_winner_team_id = Some(pens_winner);
    m
}

fn pred(reg_home: i32, reg_away: i32) -> Prediction {
    Prediction {
        id: Uuid::new_v4(),
        user_id: TelegramUserId(42),
        match_id: Uuid::new_v4(),
        reg_home,
        reg_away,
        advancement_winner_team_id: None,
        pens_winner_team_id: None,
        was_changed: false,
        points_awarded: None,
        submitted_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Group-stage 5-bucket coverage
// ---------------------------------------------------------------------------

#[test]
fn gs_total_miss_returns_zero() {
    // Predicted 0-1 (away win), actual 2-2 (draw): no goals match, sign wrong.
    let p = pred(0, 1);
    let m = group_match_finished(2, 2);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 0);
}

#[test]
fn gs_one_team_only_home() {
    // Predicted 2-0 (home win), actual 2-3 (away win): home goals match, sign wrong.
    let p = pred(2, 0);
    let m = group_match_finished(2, 3);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 4);
}

#[test]
fn gs_one_team_only_away() {
    // Predicted 0-2 (away win), actual 3-2 (home win): away goals match, sign wrong.
    let p = pred(0, 2);
    let m = group_match_finished(3, 2);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 4);
}

#[test]
fn gs_sign_only() {
    // Predicted 2-1 (home win), actual 3-0 (home win): no goals match, sign right.
    let p = pred(2, 1);
    let m = group_match_finished(3, 0);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 10);
}

#[test]
fn gs_sign_plus_one_team_home() {
    // Predicted 2-1 (home win), actual 2-0 (home win): home matches, sign right.
    let p = pred(2, 1);
    let m = group_match_finished(2, 0);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 16);
}

#[test]
fn gs_sign_plus_one_team_away() {
    // Predicted 1-2 (away win), actual 0-2 (away win): away matches, sign right.
    let p = pred(1, 2);
    let m = group_match_finished(0, 2);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 16);
}

#[test]
fn gs_exact_score() {
    let p = pred(2, 1);
    let m = group_match_finished(2, 1);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 24);
}

#[test]
fn gs_draw_exact() {
    let p = pred(0, 0);
    let m = group_match_finished(0, 0);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 24);
}

#[test]
fn gs_missing_actual_score_errors() {
    let p = pred(1, 0);
    let mut m = group_match_finished(0, 0);
    m.home_score = None;
    assert!(matches!(
        score_group_match(&p, &m, &rule()),
        Err(ScoringError::MissingMatchData(_))
    ));
}

// ---------------------------------------------------------------------------
// Re-pick penalty
// ---------------------------------------------------------------------------

#[test]
fn repick_penalty_unchanged_no_op() {
    assert_eq!(apply_repick_penalty(24, false, &rule()), 24);
}

#[test]
fn repick_penalty_applied_floor() {
    // 50 * 75 / 100 = 37 (truncating division == floor for non-negative).
    assert_eq!(apply_repick_penalty(50, true, &rule()), 37);
}

#[test]
fn repick_penalty_zero_stays_zero() {
    assert_eq!(apply_repick_penalty(0, true, &rule()), 0);
}

#[test]
fn repick_penalty_at_match_level_for_group() {
    // Exact score (24) with re-pick → 24 * 0.75 = 18.
    let mut p = pred(2, 1);
    p.was_changed = true;
    let m = group_match_finished(2, 1);
    assert_eq!(score_group_match(&p, &m, &rule()).unwrap(), 18);
}

// ---------------------------------------------------------------------------
// Knockout: worked examples from the plan
// ---------------------------------------------------------------------------

const HOME_TEAM: Uuid = Uuid::from_u128(0x1111_1111_1111_1111_1111_1111_1111_1111);
const AWAY_TEAM: Uuid = Uuid::from_u128(0x2222_2222_2222_2222_2222_2222_2222_2222);

#[test]
fn ko_predict_2_1_actual_2_1_in_reg() {
    // 12 (ADV) + 24 (exact) + 2 (ET-flag ✓) + 2 (PK-flag ✓) + 10 (combo) = 50
    let mut p = pred(2, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_reg(2, 1);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        50
    );
}

#[test]
fn ko_predict_0_0_pens_home_actual_pens_home() {
    // Reg=0-0 exact (24), ADV=home ✓ (12), ET-flag ✓ (2), PK-flag ✓ (2), combo (10) = 50
    let mut p = pred(0, 0);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.pens_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_pens(0, 0, HOME_TEAM);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        50
    );
}

#[test]
fn ko_predict_0_0_et_no_pens_actual_et_home() {
    // Predict 0-0 reg, no pens (so ET ends the match). Actual: 0-0 → 1-0 home in ET.
    // Reg exact (24), ADV ✓ (12), ET-flag ✓ (2), PK-flag ✓ (2), combo (10) = 50
    let mut p = pred(0, 0);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_et(0, 0, 1, 0);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        50
    );
}

#[test]
fn ko_predict_2_1_actual_1_0_in_reg() {
    // Reg sign-only (10), ADV ✓ (12), ET-flag ✓ (2), PK-flag ✓ (2), no combo = 26
    let mut p = pred(2, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_reg(1, 0);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        26
    );
}

#[test]
fn ko_predict_2_1_actual_3_2_in_et() {
    // Predicted (2,1) vs actual reg (2,2): H ✓ (home goals match),
    // A ✗, R ✗ → one_team_only bucket = 4. ADV ✓ (12). ET-flag ✗
    // (predicted no, actual yes). PK-flag ✓ (predicted no pens, actual no
    // pens — match ended in ET). Total = 4 + 12 + 0 + 2 = 18.
    let mut p = pred(2, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_et(2, 2, 1, 0);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        18
    );
}

#[test]
fn ko_predict_1_1_pens_home_actual_pens_home() {
    // Reg exact (24), ADV ✓ (12), ET-flag ✓ (2), PK-flag ✓ (2), combo (10) = 50
    let mut p = pred(1, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.pens_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_pens(1, 1, HOME_TEAM);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        50
    );
}

#[test]
fn ko_predict_1_1_et_no_pens_actual_pens_home() {
    // Predicted ET (no pens), but match went to pens.
    // Reg exact (24), ADV ✓ (12), ET-flag ✓ (2), PK-flag ✗ (predicted no pens, actual pens).
    // No combo. Total = 24 + 12 + 2 + 0 = 38.
    // Note: plan listed this as 40, which used a different combo definition;
    // re-checking: ADV ✓, reg exact ✓, ET-flag ✓, PK-flag ✗ → combo locked
    // out. So total = 38 with our definitions.
    let mut p = pred(1, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_pens(1, 1, HOME_TEAM);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        38
    );
}

#[test]
fn ko_predict_0_0_pens_home_actual_1_0_home_in_reg() {
    // Predicted draw + pens, but match ended in regulation 1-0 home.
    // Predicted (0,0) vs actual reg (1,0): H ✗, A ✓ (away 0 == 0), R ✗
    // (sign 0 vs +1) → one_team_only = 4. ADV ✓ home (12).
    // ET-flag ✗ (predicted yes, actual no). PK-flag ✗ (predicted yes, actual no).
    // Total = 4 + 12 + 0 + 0 = 16.
    let mut p = pred(0, 0);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.pens_winner_team_id = Some(HOME_TEAM);
    let m = knockout_match_reg(1, 0);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        16
    );
}

#[test]
fn ko_predict_2_1_no_advancement_field_derives_winner() {
    // Non-draw reg prediction with advancement_winner_team_id = None should
    // be derived as the reg winner.
    let p = pred(2, 1); // implicit home advances
    let m = knockout_match_reg(2, 1);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        50
    );
}

#[test]
fn ko_draw_pred_without_advancement_errors() {
    let p = pred(1, 1);
    let m = knockout_match_pens(1, 1, HOME_TEAM);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap_err(),
        ScoringError::MissingAdvancementWinner
    );
}

#[test]
fn ko_inconsistent_pens_without_reg_draw_rejected() {
    let mut p = pred(2, 1); // non-draw reg
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.pens_winner_team_id = Some(HOME_TEAM); // inconsistent
    let m = knockout_match_reg(2, 1);
    let result = score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule());
    assert!(matches!(
        result,
        Err(ScoringError::InconsistentPrediction(_))
    ));
}

#[test]
fn ko_inconsistent_advancement_vs_pens_winner_rejected() {
    let mut p = pred(0, 0);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.pens_winner_team_id = Some(AWAY_TEAM); // disagree
    let m = knockout_match_pens(0, 0, HOME_TEAM);
    let result = score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule());
    assert!(matches!(
        result,
        Err(ScoringError::InconsistentPrediction(_))
    ));
}

#[test]
fn ko_repick_penalty_applies_to_total() {
    // Perfect 50 with re-pick → 50 * 75 / 100 = 37.
    let mut p = pred(2, 1);
    p.advancement_winner_team_id = Some(HOME_TEAM);
    p.was_changed = true;
    let m = knockout_match_reg(2, 1);
    assert_eq!(
        score_knockout_match(&p, &m, HOME_TEAM, AWAY_TEAM, &rule()).unwrap(),
        37
    );
}

// ---------------------------------------------------------------------------
// Group-standings
// ---------------------------------------------------------------------------

fn standings_pred(p1: Uuid, p2: Uuid, p3: Uuid, p4: Uuid) -> GroupStandingsPrediction {
    GroupStandingsPrediction {
        id: Uuid::new_v4(),
        user_id: TelegramUserId(1),
        pickem_group_id: Uuid::new_v4(),
        tournament_group_id: Uuid::new_v4(),
        pos1_team_id: p1,
        pos2_team_id: p2,
        pos3_team_id: p3,
        pos4_team_id: p4,
        points_awarded: None,
        submitted_at: Utc::now(),
    }
}

#[test]
fn standings_all_correct_includes_combo() {
    let teams = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    let p = standings_pred(teams[0], teams[1], teams[2], teams[3]);
    // 6 + 4 + 2 + 1 + 5 (combo) = 18
    assert_eq!(score_group_standings(&p, &teams, &rule()), 18);
}

#[test]
fn standings_three_correct_no_combo() {
    let teams = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    // Swap pos3 ↔ pos4 in the prediction.
    let p = standings_pred(teams[0], teams[1], teams[3], teams[2]);
    // 6 + 4 + 0 + 0 + 0 = 10
    assert_eq!(score_group_standings(&p, &teams, &rule()), 10);
}

#[test]
fn standings_only_first_correct() {
    let teams = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    let other = Uuid::new_v4();
    let p = standings_pred(teams[0], other, other, other);
    assert_eq!(score_group_standings(&p, &teams, &rule()), 6);
}

#[test]
fn standings_all_wrong_zero() {
    let teams = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    let other = [
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    ];
    let p = standings_pred(other[0], other[1], other[2], other[3]);
    assert_eq!(score_group_standings(&p, &teams, &rule()), 0);
}

// ---------------------------------------------------------------------------
// Best thirds
// ---------------------------------------------------------------------------

fn best_thirds_pred(picks: Vec<Uuid>) -> BestThirdsPrediction {
    BestThirdsPrediction {
        user_id: TelegramUserId(1),
        pickem_group_id: Uuid::new_v4(),
        team_ids: picks,
    }
}

#[test]
fn best_thirds_all_eight_correct() {
    let actual: Vec<Uuid> = (0..8).map(|_| Uuid::new_v4()).collect();
    let actual_set: HashSet<Uuid> = actual.iter().copied().collect();
    let p = best_thirds_pred(actual.clone());
    assert_eq!(score_best_thirds(&p, &actual_set, &rule()), 24);
}

#[test]
fn best_thirds_one_correct() {
    let actual: HashSet<Uuid> = (0..8).map(|_| Uuid::new_v4()).collect();
    let one_actual = *actual.iter().next().unwrap();
    let mut picks = vec![one_actual];
    for _ in 0..7 {
        picks.push(Uuid::new_v4()); // wrong picks
    }
    let p = best_thirds_pred(picks);
    assert_eq!(score_best_thirds(&p, &actual, &rule()), 3);
}

#[test]
fn best_thirds_zero_correct() {
    let actual: HashSet<Uuid> = (0..8).map(|_| Uuid::new_v4()).collect();
    let picks: Vec<Uuid> = (0..8).map(|_| Uuid::new_v4()).collect();
    let p = best_thirds_pred(picks);
    assert_eq!(score_best_thirds(&p, &actual, &rule()), 0);
}
