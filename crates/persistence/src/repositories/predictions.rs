//! Postgres-backed `PredictionRepository`.
//!
//! Owned by `services/api`: the api inserts on submit, reads back to render
//! the Mini App, and the events scorer reads via `list_by_match` to compute
//! `points_awarded` for every pickem the match touches. Predictions are
//! per-pickem (`UNIQUE (user_id, pickem_group_id, match_id)`), so the same
//! user playing the same match in two pickems gets two rows that score
//! independently.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{PredictionRepository, RepoResult};
use domain::{Prediction, RepositoryError, TelegramUserId};
use error_stack::ResultExt;
use sqlx::PgPool;
use uuid::Uuid;

use super::mappers::{classify_write_error, not_found_by_id};

/// Postgres-backed `PredictionRepository`.
pub struct PgPredictionRepository {
    pub pool: PgPool,
}

impl PgPredictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Mirror of the `predictions` row, used by the `query_as!` SELECTs.
/// Keeping a dedicated row struct lets the macro infer column types
/// directly and keeps the row → domain conversion in one place.
struct PredictionRow {
    id: Uuid,
    user_id: i64,
    pickem_group_id: Uuid,
    match_id: Uuid,
    reg_home: i32,
    reg_away: i32,
    advancement_winner_team_id: Option<Uuid>,
    pens_winner_team_id: Option<Uuid>,
    was_changed: bool,
    points_awarded: Option<i32>,
    submitted_at: DateTime<Utc>,
}

impl PredictionRow {
    /// Infallible — every column is either typed-already or wraps cleanly
    /// into the domain type. No enum parsing means no failure mode.
    fn into_domain(self) -> Prediction {
        Prediction {
            id: self.id,
            user_id: TelegramUserId(self.user_id),
            pickem_group_id: self.pickem_group_id,
            match_id: self.match_id,
            reg_home: self.reg_home,
            reg_away: self.reg_away,
            advancement_winner_team_id: self.advancement_winner_team_id,
            pens_winner_team_id: self.pens_winner_team_id,
            was_changed: self.was_changed,
            points_awarded: self.points_awarded,
            submitted_at: self.submitted_at,
        }
    }
}

#[async_trait]
impl PredictionRepository for PgPredictionRepository {
    /// Insert a new prediction or rewrite an existing
    /// `(user_id, pickem_group_id, match_id)` row. The conflict path
    /// unconditionally flips `was_changed` to TRUE so the scorer can apply
    /// the re-pick penalty; the insert path leaves it FALSE via the
    /// column DEFAULT. `points_awarded` is left untouched on conflict —
    /// the api's deadline gate guarantees no resubmission can happen
    /// after scoring, so a non-NULL value here would be a bug to surface
    /// upstream rather than paper over.
    ///
    /// Surfaces `RepositoryError::Integrity` for FK violations (unknown
    /// user / pickem / match / team) and the
    /// `predictions_knockout_consistency` CHECK.
    async fn upsert(&self, prediction: &Prediction) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO predictions (
                id, user_id, pickem_group_id, match_id,
                reg_home, reg_away,
                advancement_winner_team_id, pens_winner_team_id,
                submitted_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (user_id, pickem_group_id, match_id) DO UPDATE
            SET reg_home                   = EXCLUDED.reg_home,
                reg_away                   = EXCLUDED.reg_away,
                advancement_winner_team_id = EXCLUDED.advancement_winner_team_id,
                pens_winner_team_id        = EXCLUDED.pens_winner_team_id,
                was_changed                = TRUE,
                submitted_at               = EXCLUDED.submitted_at
            "#,
            prediction.id,
            prediction.user_id.0,
            prediction.pickem_group_id,
            prediction.match_id,
            prediction.reg_home,
            prediction.reg_away,
            prediction.advancement_winner_team_id,
            prediction.pens_winner_team_id,
            prediction.submitted_at,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn list_by_match(&self, match_id: Uuid) -> RepoResult<Vec<Prediction>> {
        let rows = sqlx::query_as!(
            PredictionRow,
            r#"
            SELECT id, user_id, pickem_group_id, match_id,
                   reg_home, reg_away,
                   advancement_winner_team_id, pens_winner_team_id,
                   was_changed, points_awarded, submitted_at
            FROM predictions
            WHERE match_id = $1
            "#,
            match_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(PredictionRow::into_domain).collect())
    }

    async fn list_by_user(&self, user_id: TelegramUserId) -> RepoResult<Vec<Prediction>> {
        let rows = sqlx::query_as!(
            PredictionRow,
            r#"
            SELECT id, user_id, pickem_group_id, match_id,
                   reg_home, reg_away,
                   advancement_winner_team_id, pens_winner_team_id,
                   was_changed, points_awarded, submitted_at
            FROM predictions
            WHERE user_id = $1
            ORDER BY submitted_at DESC
            "#,
            user_id.0,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(PredictionRow::into_domain).collect())
    }

    async fn list_by_pickem(&self, pickem_group_id: Uuid) -> RepoResult<Vec<Prediction>> {
        let rows = sqlx::query_as!(
            PredictionRow,
            r#"
            SELECT id, user_id, pickem_group_id, match_id,
                   reg_home, reg_away,
                   advancement_winner_team_id, pens_winner_team_id,
                   was_changed, points_awarded, submitted_at
            FROM predictions
            WHERE pickem_group_id = $1
            "#,
            pickem_group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(PredictionRow::into_domain).collect())
    }

    async fn list_by_user_in_pickem(
        &self,
        user_id: TelegramUserId,
        pickem_group_id: Uuid,
    ) -> RepoResult<Vec<Prediction>> {
        let rows = sqlx::query_as!(
            PredictionRow,
            r#"
            SELECT id, user_id, pickem_group_id, match_id,
                   reg_home, reg_away,
                   advancement_winner_team_id, pens_winner_team_id,
                   was_changed, points_awarded, submitted_at
            FROM predictions
            WHERE user_id = $1 AND pickem_group_id = $2
            "#,
            user_id.0,
            pickem_group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(PredictionRow::into_domain).collect())
    }

    /// Idempotent — re-running the scorer for the same prediction will
    /// overwrite the previous value. Errors when the row id is unknown so
    /// the scorer surfaces ghost references instead of silently swallowing.
    async fn record_points(&self, prediction_id: Uuid, points: i32) -> RepoResult<()> {
        let result = sqlx::query!(
            r#"
            UPDATE predictions
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
            return Err(not_found_by_id("prediction", prediction_id));
        }
        Ok(())
    }

    /// Standalone toggle for the rare case where an upstream signal flips
    /// `was_changed` independently of an upsert. The standard re-pick
    /// path goes through `upsert` and never needs to call this.
    async fn mark_as_changed(&self, prediction_id: Uuid) -> RepoResult<()> {
        let result = sqlx::query!(
            r#"
            UPDATE predictions
            SET was_changed = TRUE
            WHERE id = $1
            "#,
            prediction_id,
        )
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if result.rows_affected() == 0 {
            return Err(not_found_by_id("prediction", prediction_id));
        }
        Ok(())
    }
}
