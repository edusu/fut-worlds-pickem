//! Postgres-backed `RankingRepository`.
//!
//! The leaderboard for a pickem aggregates `points_awarded` from three
//! tables:
//!
//! - `predictions` — group-stage and knockout match picks. The column is
//!   `NULL` until the scorer fills it in, hence the `IS NOT NULL` guard
//!   inside the CTE.
//! - `group_standings_predictions` — final 1..4 ordering per WC group.
//!   Same nullable semantics as `predictions`.
//! - `best_thirds_scoring` — per-(pickem, user) aggregate written once at
//!   end of group stage. PK `(pickem_group_id, user_id)` guarantees at
//!   most one row per user, so it joins as a direct PK lookup instead of
//!   a third aggregating CTE.
//!
//! `predictions.pickem_group_id` was added in migration 0007 (the per-
//! pickem rewrite) so the same user playing in two pickems gets two rows
//! and each filters cleanly here. All filters share the same `$1`
//! parameter — `groups.id` == `pickem_group_id` == `group_members.group_id`
//! by FK construction, so one substitution drives every join.
//!
//! After any JOIN, sqlx loses the `NOT NULL` info coming from `users`; the
//! `as "telegram_id!"` / `as "first_name!"` overrides re-assert it so the
//! generated row struct uses `i64` / `String` instead of `Option<...>`.
//! The outer `ORDER BY` uses positional indexes so it does not reference
//! those sqlx-flavored quoted aliases.

use async_trait::async_trait;
use domain::repository::{RankingRepository, RepoResult};
use domain::{RankingEntry, RepositoryError, TelegramUserId};
use error_stack::ResultExt;
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `RankingRepository`.
pub struct PgRankingRepository {
    pub pool: PgPool,
}

impl PgRankingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RankingRepository for PgRankingRepository {
    async fn group_ranking(&self, pickem_group_id: Uuid) -> RepoResult<Vec<RankingEntry>> {
        let rows = sqlx::query!(
            r#"
            WITH match_pts AS (
                SELECT user_id, SUM(points_awarded) AS total
                FROM predictions
                WHERE pickem_group_id = $1
                  AND points_awarded IS NOT NULL
                GROUP BY user_id
            ),
            standings_pts AS (
                SELECT user_id, SUM(points_awarded) AS total
                FROM group_standings_predictions
                WHERE pickem_group_id = $1
                  AND points_awarded IS NOT NULL
                GROUP BY user_id
            )
            SELECT u.telegram_id AS "telegram_id!",
                   u.first_name  AS "first_name!",
                   (COALESCE(mp.total, 0)
                    + COALESCE(sp.total, 0)
                    + COALESCE(bts.points_awarded, 0)) AS "total_points!"
            FROM group_members gm
            JOIN users u                   ON u.telegram_id = gm.user_id
            LEFT JOIN match_pts mp         ON mp.user_id    = gm.user_id
            LEFT JOIN standings_pts sp     ON sp.user_id    = gm.user_id
            LEFT JOIN best_thirds_scoring bts
                ON bts.user_id = gm.user_id AND bts.pickem_group_id = $1
            WHERE gm.group_id = $1
            ORDER BY 3 DESC, u.first_name ASC
            "#,
            pickem_group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|r| RankingEntry {
                user_id: TelegramUserId(r.telegram_id),
                first_name: r.first_name,
                total_points: r.total_points,
            })
            .collect())
    }
}
