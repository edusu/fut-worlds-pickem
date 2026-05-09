//! Postgres-backed `BestThirdsPredictionRepository`.
//!
//! Owned by `services/api`: the api writes on submit and reads back to
//! render the Mini App's best-thirds panel; the events scorer writes the
//! per-pickem-per-user score row into `best_thirds_scoring`.
//!
//! Storage shape: `best_thirds_predictions` carries one row per
//! `(user, pickem, team)` (PK on the triple), so an 8-pick submission is
//! 8 rows. The domain struct collapses these into a single
//! `BestThirdsPrediction { team_ids: Vec<Uuid> }`. `replace` keeps the
//! "stored set == requested set" invariant atomic by issuing the DELETE
//! and 1..N INSERTs inside one transaction.

use async_trait::async_trait;
use chrono::Utc;
use domain::repository::{BestThirdsPredictionRepository, RepoResult};
use domain::{BestThirdsPrediction, RepositoryError, TelegramUserId};
use error_stack::{Report, ResultExt};
use sqlx::PgPool;
use uuid::Uuid;

use super::mappers::classify_write_error;

/// Postgres-backed `BestThirdsPredictionRepository`.
pub struct PgBestThirdsPredictionRepository {
    pub pool: PgPool,
}

impl PgBestThirdsPredictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BestThirdsPredictionRepository for PgBestThirdsPredictionRepository {
    /// Atomically replace the user's best-thirds picks for a pickem.
    /// Both the DELETE and every INSERT run on the same transaction so a
    /// failure mid-way rolls back to the prior state — the stored set is
    /// never observed in a partial intermediate.
    ///
    /// No size validation here: the api enforces "exactly 8 picks" before
    /// calling. Persistence trusts the schema's PK to reject duplicates
    /// (same team picked twice surfaces as `Integrity`).
    async fn replace(&self, prediction: &BestThirdsPrediction) -> RepoResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .change_context(RepositoryError::Backend)?;

        sqlx::query!(
            r#"
            DELETE FROM best_thirds_predictions
            WHERE user_id = $1 AND pickem_group_id = $2
            "#,
            prediction.user_id.0,
            prediction.pickem_group_id,
        )
        .execute(&mut *tx)
        .await
        .change_context(RepositoryError::Backend)?;

        // The submitted_at column has a `DEFAULT now()` per migration
        // 0008; passing an explicit timestamp keeps every row in a single
        // submission consistent (otherwise rows in the same tx could
        // disagree by microseconds).
        let submitted_at = Utc::now();
        for team_id in &prediction.team_ids {
            sqlx::query!(
                r#"
                INSERT INTO best_thirds_predictions
                    (user_id, pickem_group_id, team_id, submitted_at)
                VALUES ($1, $2, $3, $4)
                "#,
                prediction.user_id.0,
                prediction.pickem_group_id,
                team_id,
                submitted_at,
            )
            .execute(&mut *tx)
            .await
            .map_err(classify_write_error)?;
        }

        tx.commit().await.change_context(RepositoryError::Backend)?;
        Ok(())
    }

    /// Aggregate the up-to-8 stored rows back into a single
    /// `BestThirdsPrediction`. Returns `RepositoryError::NotFound` when
    /// the user has no rows at all — the trait signature is
    /// `RepoResult<…>` (no `Option`), so callers must distinguish "not
    /// submitted" from infrastructure errors via the error kind.
    async fn list_by_user(
        &self,
        pickem_group_id: Uuid,
        user_id: TelegramUserId,
    ) -> RepoResult<BestThirdsPrediction> {
        let rows = sqlx::query!(
            r#"
            SELECT team_id
            FROM best_thirds_predictions
            WHERE user_id = $1 AND pickem_group_id = $2
            ORDER BY team_id
            "#,
            user_id.0,
            pickem_group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if rows.is_empty() {
            return Err(Report::new(RepositoryError::NotFound).attach(format!(
                "no best_thirds_predictions for user {} in pickem {pickem_group_id}",
                user_id.0
            )));
        }

        Ok(BestThirdsPrediction {
            user_id,
            pickem_group_id,
            team_ids: rows.into_iter().map(|r| r.team_id).collect(),
        })
    }

    /// Write the scored points for a `(pickem, user)` pair. Idempotent:
    /// re-running the scorer overwrites the previous value (and refreshes
    /// `scored_at`).
    ///
    /// Unlike the sibling `record_*` methods on the prediction repos
    /// (which UPDATE in place and check `rows_affected == 0` for ghost
    /// ids), this is an UPSERT and always writes — there is no missing-
    /// id failure mode to surface.
    async fn record_score(
        &self,
        pickem_group_id: Uuid,
        user_id: TelegramUserId,
        points: i32,
    ) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO best_thirds_scoring (pickem_group_id, user_id, points_awarded)
            VALUES ($1, $2, $3)
            ON CONFLICT (pickem_group_id, user_id) DO UPDATE
            SET points_awarded = EXCLUDED.points_awarded,
                scored_at      = now()
            "#,
            pickem_group_id,
            user_id.0,
            points,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }
}
