//! Shared seed helpers + mock sports client for the scorer
//! integration tests. Each `#[sqlx::test]` gets its own ephemeral
//! database — the helpers seed the minimal skeleton needed by the
//! scoring path and return the ids the test will reference.
//!
//! The `MockSportsClient` is just a holder around a static
//! `CompetitionStandings` value: tests build the standings JSON they
//! want returned and inject it.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use sports_client::{
    CompetitionStandings, SportsClientLike, SportsResult, StandingDto, StandingsRowDto, TeamRefDto,
};
use sqlx::PgPool;
use uuid::Uuid;

/// All the ids a test typically needs after `seed_basic`.
#[allow(dead_code)]
pub struct Scenario {
    pub tournament_id: Uuid,
    pub group_id: Uuid,
    pub team_a: Uuid,
    pub team_b: Uuid,
    pub team_c: Uuid,
    pub team_d: Uuid,
    /// 6 group-stage matches: A-B, A-C, A-D, B-C, B-D, C-D.
    pub matches: Vec<Uuid>,
    pub pickem_id: Uuid,
    pub user1: i64,
    pub user2: i64,
    pub default_rule_id: Uuid,
}

#[allow(dead_code)]
pub async fn seed_basic(pool: &PgPool) -> Scenario {
    seed_basic_named(pool, "Group A").await
}

/// Same as `seed_basic` but lets the caller pick the
/// `tournament_groups.name` value. Useful for tests that need to
/// match a specific `/standings` group code.
#[allow(dead_code)]
pub async fn seed_basic_named(pool: &PgPool, group_name: &str) -> Scenario {
    let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let kickoff_first: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 6, 11, 18, 0, 0).unwrap();

    // Default scoring rule seeded by migration 0004 + 0006.
    let rule = sqlx::query!(r#"SELECT id FROM scoring_rules WHERE name = 'default' LIMIT 1"#)
        .fetch_one(pool)
        .await
        .expect("default scoring rule seeded");
    let default_rule_id = rule.id;

    let tournament_id = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO tournaments (id, name, external_id) VALUES ($1, $2, $3)"#,
        tournament_id,
        "Mundial 2026 (test)",
        "WC",
    )
    .execute(pool)
    .await
    .expect("seed tournament");

    let group_id = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO tournament_groups (id, tournament_id, name, deadline_at, state)
           VALUES ($1, $2, $3, $4, 'open')"#,
        group_id,
        tournament_id,
        group_name,
        kickoff_first,
    )
    .execute(pool)
    .await
    .expect("seed tournament_group");

    let teams = ["MEX", "USA", "CAN", "BEL"];
    let mut team_ids = [Uuid::nil(); 4];
    for (i, code) in teams.iter().enumerate() {
        let id = Uuid::new_v4();
        team_ids[i] = id;
        sqlx::query!(
            r#"INSERT INTO teams (id, name, flag_emoji, country_code) VALUES ($1, $2, $3, $4)"#,
            id,
            *code,
            "🏳",
            *code,
        )
        .execute(pool)
        .await
        .expect("seed team");
        sqlx::query!(
            r#"INSERT INTO tournament_group_teams (tournament_id, tournament_group_id, team_id)
               VALUES ($1, $2, $3)"#,
            tournament_id,
            group_id,
            id,
        )
        .execute(pool)
        .await
        .expect("seed tournament_group_teams");
    }
    let [team_a, team_b, team_c, team_d] = team_ids;

    // 6 round-robin matches, all initially scheduled.
    let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    let mut matches = Vec::with_capacity(6);
    for (i, (h, a)) in pairs.iter().enumerate() {
        let mid = Uuid::new_v4();
        matches.push(mid);
        let kickoff = kickoff_first + chrono::Duration::hours(i as i64 * 3);
        sqlx::query!(
            r#"INSERT INTO matches (
                id, external_id, tournament_group_id, home_team_id, away_team_id,
                kickoff_at, status
            ) VALUES ($1, $2, $3, $4, $5, $6, 'scheduled')"#,
            mid,
            format!("test-{i}"),
            group_id,
            team_ids[*h],
            team_ids[*a],
            kickoff,
        )
        .execute(pool)
        .await
        .expect("seed match");
    }

    let user1 = 1_000_001i64;
    let user2 = 1_000_002i64;
    for &id in &[user1, user2] {
        sqlx::query!(
            r#"INSERT INTO users (telegram_id, first_name) VALUES ($1, $2)"#,
            id,
            "Tester",
        )
        .execute(pool)
        .await
        .expect("seed user");
    }

    let pickem_id = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO groups (id, telegram_chat_id, name, owner_id, scoring_rule_id, created_at)
           VALUES ($1, $2, $3, $4, $5, $6)"#,
        pickem_id,
        -100i64,
        "Test Pickem",
        user1,
        default_rule_id,
        now,
    )
    .execute(pool)
    .await
    .expect("seed pickem");

    for &uid in &[user1, user2] {
        sqlx::query!(
            r#"INSERT INTO group_members (group_id, user_id, joined_at) VALUES ($1, $2, $3)"#,
            pickem_id,
            uid,
            now,
        )
        .execute(pool)
        .await
        .expect("seed group_member");
    }

    Scenario {
        tournament_id,
        group_id,
        team_a,
        team_b,
        team_c,
        team_d,
        matches,
        pickem_id,
        user1,
        user2,
        default_rule_id,
    }
}

/// Mark a match as finished with the given regulation scores.
#[allow(dead_code)]
pub async fn finish_group_match(pool: &PgPool, match_id: Uuid, home: i32, away: i32) {
    sqlx::query!(
        r#"UPDATE matches
           SET status = 'finished', home_score = $2, away_score = $3
           WHERE id = $1"#,
        match_id,
        home,
        away,
    )
    .execute(pool)
    .await
    .expect("finish match");
}

/// Insert a single group-stage prediction.
#[allow(dead_code)]
pub async fn insert_prediction(
    pool: &PgPool,
    user_id: i64,
    pickem_id: Uuid,
    match_id: Uuid,
    reg_home: i32,
    reg_away: i32,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query!(
        r#"INSERT INTO predictions (
            id, user_id, pickem_group_id, match_id, reg_home, reg_away
        ) VALUES ($1, $2, $3, $4, $5, $6)"#,
        id,
        user_id,
        pickem_id,
        match_id,
        reg_home,
        reg_away,
    )
    .execute(pool)
    .await
    .expect("insert prediction");
    id
}

/// Mock sports client used by tests that exercise the dispatchers
/// without hitting football-data.org. Holds a fixed
/// `CompetitionStandings` payload which is returned on every call.
pub struct MockSportsClient {
    pub standings: CompetitionStandings,
}

impl MockSportsClient {
    /// Empty payload — useful when the test path will never reach a
    /// `/standings` call (matches that don't trigger end-of-group).
    #[allow(dead_code)]
    pub fn empty() -> Self {
        Self {
            standings: CompetitionStandings { standings: vec![] },
        }
    }

    /// Build a payload with one group entry whose 4 rows are taken
    /// from `(position, tla)` tuples.
    #[allow(dead_code)]
    pub fn with_single_group(group_code: &str, rows: Vec<(i32, &'static str)>) -> Self {
        Self {
            standings: CompetitionStandings {
                standings: vec![StandingDto {
                    stage: Some("GROUP_STAGE".to_string()),
                    type_: Some("TOTAL".to_string()),
                    group: Some(group_code.to_string()),
                    table: rows
                        .into_iter()
                        .map(|(pos, tla)| StandingsRowDto {
                            position: Some(pos),
                            team: TeamRefDto {
                                id: Some(0),
                                name: Some(tla.to_string()),
                                short_name: None,
                                tla: Some(tla.to_string()),
                                crest: None,
                            },
                            played_games: Some(3),
                            won: None,
                            draw: None,
                            lost: None,
                            points: None,
                            goals_for: None,
                            goals_against: None,
                            goal_difference: None,
                        })
                        .collect(),
                }],
            },
        }
    }
}

#[async_trait]
impl SportsClientLike for MockSportsClient {
    async fn get_competition_standings(
        &self,
        _competition_code: &str,
    ) -> SportsResult<CompetitionStandings> {
        Ok(self.standings.clone())
    }
}
