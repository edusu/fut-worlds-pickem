//! Postgres-backed `TournamentRepository`.
//!
//! Owned by `services/events`. The ingester upserts the tournament row at
//! boot (one-shot) before seeding tournament_groups, knockout_phases, and
//! the matches that hang off them. The api and bot READ to translate
//! between upstream codes and domain UUIDs.

use async_trait::async_trait;
use domain::repository::{RepoResult, TournamentRepository};
use domain::{RepositoryError, Tournament};
use error_stack::ResultExt;
use sqlx::PgPool;
use uuid::Uuid;

use super::mappers::classify_write_error;

/// Postgres-backed `TournamentRepository`.
pub struct PgTournamentRepository {
    pub pool: PgPool,
}

impl PgTournamentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TournamentRepository for PgTournamentRepository {
    /// Insert a tournament or refresh its display name on conflict. The
    /// natural key is `external_id` (the upstream provider's competition
    /// code), so the same call from the ingester at every boot is a safe
    /// no-op once the row is in place.
    async fn upsert(&self, tournament: &Tournament) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO tournaments (id, name, external_id, created_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (external_id) DO UPDATE
            SET name = EXCLUDED.name
            "#,
            tournament.id,
            tournament.name,
            tournament.external_id,
            tournament.created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<Tournament>> {
        let row = sqlx::query!(
            r#"
            SELECT id, name, external_id, created_at
            FROM tournaments
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|r| Tournament {
            id: r.id,
            name: r.name,
            external_id: r.external_id,
            created_at: r.created_at,
        }))
    }

    async fn find_by_external_id(&self, external_id: &str) -> RepoResult<Option<Tournament>> {
        let row = sqlx::query!(
            r#"
            SELECT id, name, external_id, created_at
            FROM tournaments
            WHERE external_id = $1
            "#,
            external_id,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|r| Tournament {
            id: r.id,
            name: r.name,
            external_id: r.external_id,
            created_at: r.created_at,
        }))
    }
}
