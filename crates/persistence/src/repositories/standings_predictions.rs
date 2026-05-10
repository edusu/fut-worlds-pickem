//! Postgres-backed `StandingsPredictionRepository`.
//!
//! Owned by `services/api`: the api inserts on submit, reads back to render
//! the Mini App's standings panel, and the events scorer reads via
//! `list_by_pickem` to score every pickem's predictions for each
//! tournament group at end-of-group-stage.
//!
//! Idempotency: a re-submission of the same `(user_id, pickem_group_id,
//! tournament_group_id)` rewrites the four position picks in place via
//! `ON CONFLICT … DO UPDATE`. The natural key is enforced at the schema
//! level by `UNIQUE (user_id, pickem_group_id, tournament_group_id)`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{RepoResult, StandingsPredictionRepository};
use domain::{GroupStandingsPrediction, RepositoryError, TelegramUserId};
use error_stack::ResultExt;
use sqlx::PgPool;
use uuid::Uuid;

use super::mappers::{classify_write_error, not_found_by_id};

/// Postgres-backed `StandingsPredictionRepository`.
pub struct PgStandingsPredictionRepository {
    pub pool: PgPool,
}

impl PgStandingsPredictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Mirror of the `group_standings_predictions` row, used by `query_as!`.
struct StandingsRow {
    id: Uuid,
    user_id: i64,
    pickem_group_id: Uuid,
    tournament_group_id: Uuid,
    pos1_team_id: Uuid,
    pos2_team_id: Uuid,
    pos3_team_id: Uuid,
    pos4_team_id: Uuid,
    points_awarded: Option<i32>,
    submitted_at: DateTime<Utc>,
}

impl StandingsRow {
    fn into_domain(self) -> GroupStandingsPrediction {
        GroupStandingsPrediction {
            id: self.id,
            user_id: TelegramUserId(self.user_id),
            pickem_group_id: self.pickem_group_id,
            tournament_group_id: self.tournament_group_id,
            pos1_team_id: self.pos1_team_id,
            pos2_team_id: self.pos2_team_id,
            pos3_team_id: self.pos3_team_id,
            pos4_team_id: self.pos4_team_id,
            points_awarded: self.points_awarded,
            submitted_at: self.submitted_at,
        }
    }
}

#[async_trait]
impl StandingsPredictionRepository for PgStandingsPredictionRepository {
    /// Insert or rewrite the four position picks for a `(user, pickem,
    /// tournament_group)` triple. `points_awarded` is left untouched on
    /// conflict — the api's deadline gate ensures resubmission cannot
    /// happen after scoring.
    ///
    /// Surfaces `RepositoryError::Integrity` for the
    /// `group_standings_distinct_teams` CHECK (any two of pos1..4 equal)
    /// and FK violations (unknown team / pickem / tournament group).
    async fn upsert(&self, prediction: &GroupStandingsPrediction) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO group_standings_predictions (
                id, user_id, pickem_group_id, tournament_group_id,
                pos1_team_id, pos2_team_id, pos3_team_id, pos4_team_id,
                submitted_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (user_id, pickem_group_id, tournament_group_id) DO UPDATE
            SET pos1_team_id = EXCLUDED.pos1_team_id,
                pos2_team_id = EXCLUDED.pos2_team_id,
                pos3_team_id = EXCLUDED.pos3_team_id,
                pos4_team_id = EXCLUDED.pos4_team_id,
                submitted_at = EXCLUDED.submitted_at
            "#,
            prediction.id,
            prediction.user_id.0,
            prediction.pickem_group_id,
            prediction.tournament_group_id,
            prediction.pos1_team_id,
            prediction.pos2_team_id,
            prediction.pos3_team_id,
            prediction.pos4_team_id,
            prediction.submitted_at,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn list_by_pickem(
        &self,
        pickem_group_id: Uuid,
    ) -> RepoResult<Vec<GroupStandingsPrediction>> {
        let rows = sqlx::query_as!(
            StandingsRow,
            r#"
            SELECT id, user_id, pickem_group_id, tournament_group_id,
                   pos1_team_id, pos2_team_id, pos3_team_id, pos4_team_id,
                   points_awarded, submitted_at
            FROM group_standings_predictions
            WHERE pickem_group_id = $1
            ORDER BY tournament_group_id, user_id
            "#,
            pickem_group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(StandingsRow::into_domain).collect())
    }

    /// Idempotent — re-running the scorer for the same prediction will
    /// overwrite the previous value. Errors when the row id is unknown.
    async fn record_points(&self, prediction_id: Uuid, points: i32) -> RepoResult<()> {
        let result = sqlx::query!(
            r#"
            UPDATE group_standings_predictions
            SET points_awarded = $2
            WHERE id = $1
            "#,
            prediction_id,
            points,
        )
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if result.rows_affected() == 0 {
            return Err(not_found_by_id("group_standings_prediction", prediction_id));
        }
        Ok(())
    }
}
