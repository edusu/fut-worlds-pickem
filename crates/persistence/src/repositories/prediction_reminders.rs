//! Postgres-backed `PredictionReminderRepository`.
//!
//! Owned by `services/events` (the scheduler tick). The query in
//! `list_pending` is the heart of feature #8: it joins matches against
//! both parent kinds, the pickem roster, the existing predictions, and
//! the reminder ledger in a single round-trip.
//!
//! Anti-join on `predictions` ("members missing a prediction") and on
//! `prediction_reminders` ("haven't been notified yet") avoids an N+1
//! pattern that a naive Rust loop would produce. The `CROSS JOIN` over
//! `matches × group_members` is bounded by `WHERE` filters and is fine
//! at v1 scale (≤ ~100 matches × ≤ tens of members per pickem).

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::repository::{PredictionReminderRepository, RepoResult};
use domain::{PendingReminder, RepositoryError, TelegramUserId};
use error_stack::ResultExt;
use sqlx::PgPool;

use super::mappers::classify_write_error;

pub struct PgPredictionReminderRepository {
    pub pool: PgPool,
}

impl PgPredictionReminderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PredictionReminderRepository for PgPredictionReminderRepository {
    async fn list_pending(
        &self,
        now: DateTime<Utc>,
        min_offset: Duration,
        max_offset: Duration,
    ) -> RepoResult<Vec<PendingReminder>> {
        let window_start = now + min_offset;
        let window_end = now + max_offset;

        let rows = sqlx::query!(
            r#"
            SELECT DISTINCT gm.user_id AS "user_id!",
                            m.id       AS "match_id!"
            FROM matches m
            LEFT JOIN tournament_groups tg ON tg.id = m.tournament_group_id
            LEFT JOIN knockout_phases  kp  ON kp.id = m.knockout_phase_id
            CROSS JOIN group_members gm
            LEFT JOIN predictions p
                ON p.user_id         = gm.user_id
               AND p.match_id        = m.id
               AND p.pickem_group_id = gm.group_id
            WHERE m.kickoff_at > $1
              AND p.id IS NULL
              AND (
                  (tg.state = 'open' AND tg.deadline_at BETWEEN $2 AND $3)
               OR (kp.state = 'open' AND kp.deadline_at BETWEEN $2 AND $3)
              )
              AND NOT EXISTS (
                  SELECT 1 FROM prediction_reminders pr
                  WHERE pr.user_id  = gm.user_id
                    AND pr.match_id = m.id
              )
            ORDER BY gm.user_id, m.id
            "#,
            now,
            window_start,
            window_end,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|r| PendingReminder {
                user_id: TelegramUserId(r.user_id),
                match_id: r.match_id,
            })
            .collect())
    }

    async fn mark_sent_batch(&self, pairs: &[PendingReminder]) -> RepoResult<()> {
        if pairs.is_empty() {
            return Ok(());
        }

        // Unpack into two parallel Vecs so we can pass them to the
        // multi-row INSERT via `UNNEST($1, $2)` — the idiomatic Postgres
        // pattern for bulk inserts that does not need a runtime-built SQL
        // string.
        let user_ids: Vec<i64> = pairs.iter().map(|p| p.user_id.0).collect();
        let match_ids: Vec<_> = pairs.iter().map(|p| p.match_id).collect();

        sqlx::query!(
            r#"
            INSERT INTO prediction_reminders (user_id, match_id)
            SELECT * FROM UNNEST($1::BIGINT[], $2::UUID[])
            ON CONFLICT (user_id, match_id) DO NOTHING
            "#,
            &user_ids,
            &match_ids,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }
}
