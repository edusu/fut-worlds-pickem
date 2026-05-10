//! Integration tests for `events::scorer::dispatch`.
//!
//! Each test runs against an ephemeral Postgres database supplied by
//! `#[sqlx::test(migrations = "../../migrations")]`. The migrations
//! folder lives at the workspace root, two levels up from the test
//! crate manifest.

mod common;

use common::{finish_group_match, insert_prediction, seed_basic, MockSportsClient};
use events::scorer::dispatch::dispatch_match_scoring;
use sqlx::PgPool;

/// dispatch_match_scoring with a single group-stage match:
///
/// - User1 predicted exactly the right scoreline (`gs_exact_points`).
/// - User2 predicted an off-by-N result with the right sign
///   (`gs_sign_only_points`).
///
/// Both predictions must end up with `points_awarded` set, and one
/// audit row each in `scoring_audit`.
#[sqlx::test(migrations = "../../migrations")]
async fn dispatch_match_scoring_group_happy_path(pool: PgPool) {
    let s = seed_basic(&pool).await;
    let mid = s.matches[0]; // first round-robin match: A vs B

    insert_prediction(&pool, s.user1, s.pickem_id, mid, 2, 1).await;
    insert_prediction(&pool, s.user2, s.pickem_id, mid, 1, 0).await;
    finish_group_match(&pool, mid, 2, 1).await;

    let mock = MockSportsClient::empty();
    dispatch_match_scoring(&pool, &mock, mid)
        .await
        .expect("dispatch ok");

    let scored = sqlx::query!(
        r#"SELECT user_id, points_awarded FROM predictions WHERE match_id = $1
           ORDER BY user_id"#,
        mid,
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(scored.len(), 2);
    assert!(scored[0].points_awarded.is_some(), "user1 not scored");
    assert!(scored[1].points_awarded.is_some(), "user2 not scored");
    assert!(
        scored[0].points_awarded.unwrap() > scored[1].points_awarded.unwrap(),
        "user1 (exact) should outscore user2 (sign-only): {:?} vs {:?}",
        scored[0].points_awarded,
        scored[1].points_awarded,
    );

    let audit = sqlx::query!(
        r#"SELECT COUNT(*)::BIGINT AS "n!" FROM scoring_audit
           WHERE pickem_group_id = $1 AND audit_kind = 'match'"#,
        s.pickem_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.n, 2, "expected one audit row per scored prediction");
}

/// Calling `dispatch_match_scoring` twice for the same match leaves
/// `predictions.points_awarded` at the same final value (`record_points`
/// is idempotent UPDATE). The audit log *does* grow because it's
/// append-only by design.
#[sqlx::test(migrations = "../../migrations")]
async fn dispatch_match_scoring_idempotent(pool: PgPool) {
    let s = seed_basic(&pool).await;
    let mid = s.matches[0];

    insert_prediction(&pool, s.user1, s.pickem_id, mid, 2, 1).await;
    finish_group_match(&pool, mid, 2, 1).await;

    let mock = MockSportsClient::empty();
    dispatch_match_scoring(&pool, &mock, mid).await.unwrap();
    let after_first = sqlx::query!(
        r#"SELECT points_awarded FROM predictions WHERE match_id = $1 AND user_id = $2"#,
        mid,
        s.user1,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    dispatch_match_scoring(&pool, &mock, mid).await.unwrap();
    let after_second = sqlx::query!(
        r#"SELECT points_awarded FROM predictions WHERE match_id = $1 AND user_id = $2"#,
        mid,
        s.user1,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(after_first.points_awarded, after_second.points_awarded);

    // Audit is append-only, so two rows after two runs is correct.
    let audit_count = sqlx::query!(
        r#"SELECT COUNT(*)::BIGINT AS "n!" FROM scoring_audit
           WHERE pickem_group_id = $1 AND audit_kind = 'match'"#,
        s.pickem_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit_count.n, 2,
        "scoring_audit should have grown by one row per dispatch run"
    );
}

/// dispatch_match_scoring against a knockout match: the test seeds a
/// knockout phase + match (R16-style) with regulation scores, then
/// confirms the prediction is scored via the knockout scoring rule.
#[sqlx::test(migrations = "../../migrations")]
async fn dispatch_match_scoring_knockout_happy_path(pool: PgPool) {
    let s = seed_basic(&pool).await;

    // Add a knockout phase + a single R16 match between team_a and team_b.
    let phase_id = uuid::Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO knockout_phases (
            id, tournament_id, stage, position, display_name, deadline_at, state
        ) VALUES ($1, $2, 'LAST_16', 2, 'Octavos', $3, 'open')"#,
        phase_id,
        s.tournament_id,
        chrono::Utc.with_ymd_and_hms(2026, 6, 28, 18, 0, 0).unwrap(),
    )
    .execute(&pool)
    .await
    .unwrap();

    let mid = uuid::Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO matches (
            id, external_id, knockout_phase_id, home_team_id, away_team_id,
            kickoff_at, status, home_score, away_score
        ) VALUES ($1, $2, $3, $4, $5, $6, 'finished', 2, 1)"#,
        mid,
        "ko-1",
        phase_id,
        s.team_a,
        s.team_b,
        chrono::Utc.with_ymd_and_hms(2026, 6, 28, 18, 0, 0).unwrap(),
    )
    .execute(&pool)
    .await
    .unwrap();

    // Prediction: 2-1 (regulation, team A wins). Implies team A advances.
    let pid = uuid::Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO predictions (
            id, user_id, pickem_group_id, match_id,
            reg_home, reg_away, advancement_winner_team_id
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        pid,
        s.user1,
        s.pickem_id,
        mid,
        2,
        1,
        s.team_a,
    )
    .execute(&pool)
    .await
    .unwrap();

    let mock = MockSportsClient::empty();
    dispatch_match_scoring(&pool, &mock, mid)
        .await
        .expect("dispatch ok");

    let row = sqlx::query!(
        r#"SELECT points_awarded FROM predictions WHERE id = $1"#,
        pid,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        row.points_awarded.unwrap() > 0,
        "knockout prediction should score > 0 for an exact + advancement match"
    );
}

// chrono needs to be importable for the inline timestamps used above.
use chrono::TimeZone;
