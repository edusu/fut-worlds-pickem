//! Integration tests for `events::scorer::reconcile`.

mod common;

use chrono::TimeZone;
use common::{finish_group_match, insert_prediction, seed_basic, MockSportsClient};
use events::scorer::reconcile::reconcile;
use sqlx::PgPool;
use uuid::Uuid;

/// The reconciliator picks up matches finished but unscored
/// (predictions.points_awarded IS NULL) and dispatches them.
#[sqlx::test(migrations = "../../migrations")]
async fn reconcile_picks_up_unscored_matches(pool: PgPool) {
    let s = seed_basic(&pool).await;
    let mid = s.matches[0];

    insert_prediction(&pool, s.user1, s.pickem_id, mid, 2, 1).await;
    finish_group_match(&pool, mid, 2, 1).await;
    // Note: we do NOT call dispatch_match_scoring here. That's the
    // exact at-most-once gap the reconciliator is meant to close.

    let mock = MockSportsClient::empty();
    reconcile(&pool, &mock).await.expect("reconcile ok");

    let row = sqlx::query!(
        r#"SELECT points_awarded FROM predictions WHERE match_id = $1 AND user_id = $2"#,
        mid,
        s.user1,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        row.points_awarded.is_some(),
        "reconcile should have scored the unscored prediction"
    );
}

/// Reconcile + dispatch_group_standings happy path: all 6 matches in
/// the group finished, a standings prediction in the DB, /standings
/// mocked. After the reconciliator runs, the group is in `scored`
/// and the standings prediction has points.
#[sqlx::test(migrations = "../../migrations")]
async fn reconcile_pending_group_standings_happy_path(pool: PgPool) {
    let s = common::seed_basic_named(&pool, "Group A").await;

    // Finish every match with team A winning consistently. Real scores
    // don't matter for this test as long as the matches are flagged
    // `finished`.
    for &mid in &s.matches {
        finish_group_match(&pool, mid, 2, 0).await;
    }

    // One user submits a standings prediction. Order: A 1st, B 2nd, C
    // 3rd, D 4th — same as what the mock /standings will report.
    let pred_id = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO group_standings_predictions (
            id, user_id, pickem_group_id, tournament_group_id,
            pos1_team_id, pos2_team_id, pos3_team_id, pos4_team_id,
            submitted_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        pred_id,
        s.user1,
        s.pickem_id,
        s.group_id,
        s.team_a,
        s.team_b,
        s.team_c,
        s.team_d,
        chrono::Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
    )
    .execute(&pool)
    .await
    .unwrap();

    let mock = MockSportsClient::with_single_group(
        "GROUP_A",
        vec![(1, "MEX"), (2, "USA"), (3, "CAN"), (4, "BEL")],
    );

    reconcile(&pool, &mock).await.expect("reconcile ok");

    let row = sqlx::query!(
        r#"SELECT points_awarded, (
              SELECT state FROM tournament_groups WHERE id = $1
           ) AS "group_state!"
           FROM group_standings_predictions WHERE id = $2"#,
        s.group_id,
        pred_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        row.group_state, "scored",
        "tournament_group should transition to scored"
    );
    let pts = row.points_awarded.expect("standings prediction scored");
    assert!(
        pts > 0,
        "standings prediction matched ordering exactly, expected > 0"
    );
}

/// Reconcile + dispatch_best_thirds gating: with the 12 groups not all
/// in `scored`, and even if we asked the dispatcher directly, it
/// must short-circuit. The smaller test verifies the bracket-level
/// gate: feed a tournament where a R16 match has NULL teams and
/// confirm `best_thirds_scoring` stays empty.
#[sqlx::test(migrations = "../../migrations")]
async fn dispatch_best_thirds_defers_when_r16_partial(pool: PgPool) {
    use events::scorer::standings::dispatch_best_thirds;

    let s = seed_basic(&pool).await;

    // Force the single seeded group into `scored` so the "all groups
    // scored" gate fires (we only have one group in the fixture; the
    // dispatcher's `all_group_stage_groups_scored` checks "all groups
    // for this tournament are scored" which is trivially true with
    // just one group).
    sqlx::query!(
        r#"UPDATE tournament_groups SET state = 'scored' WHERE id = $1"#,
        s.group_id,
    )
    .execute(&pool)
    .await
    .unwrap();

    // Add a knockout R16 phase with one match whose teams are NULL —
    // partial bracket, dispatcher must defer.
    let phase_id = Uuid::new_v4();
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

    let mid = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO matches (
            id, external_id, knockout_phase_id,
            kickoff_at, status
        ) VALUES ($1, $2, $3, $4, 'scheduled')"#,
        mid,
        "r16-1",
        phase_id,
        chrono::Utc.with_ymd_and_hms(2026, 6, 28, 18, 0, 0).unwrap(),
    )
    .execute(&pool)
    .await
    .unwrap();

    // No best_thirds_scoring rows exist yet. Even with the gate
    // partially open (groups scored), the dispatcher should not write
    // anything because the R16 bracket has < 16 fixtures + NULL teams.
    let mock = MockSportsClient::empty();
    dispatch_best_thirds(&pool, &mock)
        .await
        .expect("dispatch ok");

    let count = sqlx::query!(
        r#"SELECT COUNT(*)::BIGINT AS "n!" FROM best_thirds_scoring
           WHERE pickem_group_id = $1"#,
        s.pickem_id,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        count.n, 0,
        "dispatch_best_thirds must defer while R16 is incomplete"
    );
}
