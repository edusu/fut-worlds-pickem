//! Postgres-backed `TournamentGroupRepository`.
//!
//! Owned by `services/events`. The seed CLI fills `tournament_groups` and
//! `tournament_group_teams` from football-data.org standings; the scorer
//! reads the join later to resolve home/away team UUIDs in knockout games.

use async_trait::async_trait;
use domain::repository::{RepoResult, TournamentGroupRepository};
use domain::{RepositoryError, Team, TournamentGroup, TournamentGroupAssignment};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Postgres-backed `TournamentGroupRepository`.
pub struct PgTournamentGroupRepository {
    pub pool: PgPool,
}

impl PgTournamentGroupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TournamentGroupRepository for PgTournamentGroupRepository {
    /// Insert or refresh a tournament group. The natural key is `name`
    /// (UNIQUE in the schema), so resending "Group A" with the same id is a
    /// no-op and a different id is reconciled to the existing row.
    async fn upsert(&self, group: &TournamentGroup) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO tournament_groups (id, name)
            VALUES ($1, $2)
            ON CONFLICT (name) DO UPDATE
            SET name = EXCLUDED.name
            "#,
        )
        .bind(group.id)
        .bind(&group.name)
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(())
    }

    async fn list_all(&self) -> RepoResult<Vec<TournamentGroup>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name
            FROM tournament_groups
            ORDER BY name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|row| TournamentGroup {
                id: row.get("id"),
                name: row.get("name"),
            })
            .collect())
    }

    /// Idempotently associate a team with a tournament group. The schema
    /// enforces single-group membership for a team via `UNIQUE(team_id)`,
    /// so re-running the seed silently becomes a no-op for already-assigned
    /// teams and surfaces an integrity error if the same team is sent to a
    /// different group (intentional: that's a real upstream conflict the
    /// caller must resolve, not a transient hiccup).
    async fn assign_team(&self, assignment: &TournamentGroupAssignment) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO tournament_group_teams (tournament_group_id, team_id)
            VALUES ($1, $2)
            ON CONFLICT (tournament_group_id, team_id) DO NOTHING
            "#,
        )
        .bind(assignment.tournament_group_id)
        .bind(assignment.team_id)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let kind = match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => RepositoryError::Integrity,
                _ => RepositoryError::Backend,
            };
            Report::new(e).change_context(kind)
        })?;

        Ok(())
    }

    async fn list_teams_in_group(&self, group_id: Uuid) -> RepoResult<Vec<Team>> {
        let rows = sqlx::query(
            r#"
            SELECT t.id, t.name, t.flag_emoji, t.country_code
            FROM teams t
            JOIN tournament_group_teams tgt ON tgt.team_id = t.id
            WHERE tgt.tournament_group_id = $1
            ORDER BY t.name ASC
            "#,
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|row| Team {
                id: row.get("id"),
                name: row.get("name"),
                flag_emoji: row.get("flag_emoji"),
                country_code: row.get("country_code"),
            })
            .collect())
    }
}

impl PgTournamentGroupRepository {
    /// Look up by canonical name (e.g. "Group A"). Used by the seed CLI to
    /// resolve a name to an existing row's id before issuing assignments.
    pub async fn find_by_name(&self, name: &str) -> RepoResult<Option<TournamentGroup>> {
        let row = sqlx::query(
            r#"
            SELECT id, name
            FROM tournament_groups
            WHERE name = $1
            "#,
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|row| TournamentGroup {
            id: row.get("id"),
            name: row.get("name"),
        }))
    }
}
