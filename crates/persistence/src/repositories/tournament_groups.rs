//! Postgres-backed `TournamentGroupRepository`.
//!
//! Owned by `services/events`. The bootstrap seeds groups (with their
//! shared submission deadline and `state = open`) on first boot; the seed
//! CLI seeds team-to-group assignments. The api and bot may READ.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{RepoResult, TournamentGroupRepository};
use domain::{
    RepositoryError, Team, TournamentGroup, TournamentGroupAssignment, TournamentGroupState,
};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::mappers::{classify_write_error, parse_state, LifecycleState};

const SELECT_GROUP_COLUMNS: &str =
    "SELECT id, tournament_id, name, deadline_at, state, created_at FROM tournament_groups";

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
    /// Insert or refresh a tournament group. The natural key is
    /// `(tournament_id, name)`, so resending "Group A" within the same
    /// tournament is idempotent. Mutable display fields (`deadline_at`,
    /// `state`) are updated on conflict; immutable identity columns are
    /// left alone via `EXCLUDED.column`.
    async fn upsert(&self, group: &TournamentGroup) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO tournament_groups
                (id, tournament_id, name, deadline_at, state, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (tournament_id, name) DO UPDATE
            SET deadline_at = EXCLUDED.deadline_at,
                state       = EXCLUDED.state
            "#,
        )
        .bind(group.id)
        .bind(group.tournament_id)
        .bind(&group.name)
        .bind(group.deadline_at)
        .bind(group.state.as_db_str())
        .bind(group.created_at)
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<TournamentGroup>> {
        let row = sqlx::query(&format!("{SELECT_GROUP_COLUMNS} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .change_context(RepositoryError::Backend)?;

        row.map(row_to_group).transpose()
    }

    async fn find_by_name(
        &self,
        tournament_id: Uuid,
        name: &str,
    ) -> RepoResult<Option<TournamentGroup>> {
        let row = sqlx::query(&format!(
            "{SELECT_GROUP_COLUMNS} WHERE tournament_id = $1 AND name = $2"
        ))
        .bind(tournament_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_group).transpose()
    }

    async fn list_by_tournament(&self, tournament_id: Uuid) -> RepoResult<Vec<TournamentGroup>> {
        let rows = sqlx::query(&format!(
            "{SELECT_GROUP_COLUMNS} WHERE tournament_id = $1 ORDER BY name ASC"
        ))
        .bind(tournament_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_group).collect()
    }

    async fn list_active(&self, tournament_id: Uuid) -> RepoResult<Vec<TournamentGroup>> {
        let rows = sqlx::query(&format!(
            "{SELECT_GROUP_COLUMNS} WHERE tournament_id = $1 \
             AND state = 'open' AND deadline_at > now() \
             ORDER BY deadline_at ASC"
        ))
        .bind(tournament_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_group).collect()
    }

    async fn set_state(&self, group_id: Uuid, state: TournamentGroupState) -> RepoResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE tournament_groups
            SET state = $2
            WHERE id = $1
            "#,
        )
        .bind(group_id)
        .bind(state.as_db_str())
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if result.rows_affected() == 0 {
            return Err(Report::new(RepositoryError::NotFound)
                .attach(format!("tournament_group {group_id} not found")));
        }
        Ok(())
    }

    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<TournamentGroup>> {
        let rows = sqlx::query(&format!(
            "{SELECT_GROUP_COLUMNS} \
             WHERE state = 'open' AND deadline_at <= $1 \
             ORDER BY deadline_at ASC"
        ))
        .bind(before)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_group).collect()
    }

    /// Idempotently associate a team with a tournament group. The
    /// composite FK `(tournament_id, tournament_group_id)` →
    /// `tournament_groups(tournament_id, id)` enforces that the supplied
    /// `tournament_id` matches the parent group's tournament; an
    /// inconsistent assignment surfaces as `Integrity`.
    async fn assign_team(&self, assignment: &TournamentGroupAssignment) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO tournament_group_teams
                (tournament_id, tournament_group_id, team_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (tournament_id, team_id) DO UPDATE
            SET tournament_group_id = EXCLUDED.tournament_group_id
            "#,
        )
        .bind(assignment.tournament_id)
        .bind(assignment.tournament_group_id)
        .bind(assignment.team_id)
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

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

fn row_to_group(row: sqlx::postgres::PgRow) -> RepoResult<TournamentGroup> {
    let state_raw: String = row.get("state");
    Ok(TournamentGroup {
        id: row.get("id"),
        tournament_id: row.get("tournament_id"),
        name: row.get("name"),
        deadline_at: row.get("deadline_at"),
        state: parse_state(&state_raw)?,
        created_at: row.get("created_at"),
    })
}
