//! Postgres-backed `TeamRepository`.
//!
//! Owned by `services/events`: only the events service writes to `teams`,
//! everyone else reads. The seed CLI calls `upsert` once per nation; the
//! ingester resolves DTOs to ids via `find` / `list_all`.

use async_trait::async_trait;
use domain::repository::{RepoResult, TeamRepository};
use domain::{RepositoryError, Team};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Postgres-backed `TeamRepository`.
pub struct PgTeamRepository {
    pub pool: PgPool,
}

impl PgTeamRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TeamRepository for PgTeamRepository {
    /// Insert a team or refresh its display fields if the country_code is
    /// already known.
    ///
    /// `country_code` is the natural key (FIFA / IOC 3-letter for national
    /// teams) and is `UNIQUE` at the schema level. We deliberately *do not*
    /// touch `id` on conflict so any FK that already references the existing
    /// row stays valid.
    async fn upsert(&self, team: &Team) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO teams (id, name, flag_emoji, country_code)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (country_code) DO UPDATE
            SET name = EXCLUDED.name,
                flag_emoji = EXCLUDED.flag_emoji
            "#,
        )
        .bind(team.id)
        .bind(&team.name)
        .bind(&team.flag_emoji)
        .bind(&team.country_code)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            // `flag_emoji NOT NULL` or any future check could surface as a
            // distinct integrity error; bucket it accordingly so callers can
            // distinguish bad data from infrastructure problems.
            let kind = match &e {
                sqlx::Error::Database(db)
                    if db.is_unique_violation() || db.is_check_violation() =>
                {
                    RepositoryError::Integrity
                }
                _ => RepositoryError::Backend,
            };
            Report::new(e).change_context(kind)
        })?;

        Ok(())
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<Team>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, flag_emoji, country_code
            FROM teams
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(row_to_team))
    }

    async fn list_all(&self) -> RepoResult<Vec<Team>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, flag_emoji, country_code
            FROM teams
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows.into_iter().map(row_to_team).collect())
    }
}

impl PgTeamRepository {
    /// Look a team up by its country_code (3-letter FIFA / IOC). Used by the
    /// seed CLI to resolve DTOs to existing rows without paying the cost of
    /// loading every row first.
    pub async fn find_by_country_code(&self, code: &str) -> RepoResult<Option<Team>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, flag_emoji, country_code
            FROM teams
            WHERE country_code = $1
            "#,
        )
        .bind(code)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(row_to_team))
    }
}

fn row_to_team(row: sqlx::postgres::PgRow) -> Team {
    Team {
        id: row.get("id"),
        name: row.get("name"),
        flag_emoji: row.get("flag_emoji"),
        country_code: row.get("country_code"),
    }
}
