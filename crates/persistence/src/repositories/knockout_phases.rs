//! Postgres-backed `KnockoutPhaseRepository`.
//!
//! Owned by `services/events`: the bootstrap seeds the 6 phases (Round of
//! 32 through Final) on first boot, deriving the shared knockout deadline
//! from the upstream's earliest knockout kickoff. The api may READ to list
//! the active phase and render the bracket; nobody else writes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{KnockoutPhaseRepository, RepoResult};
use domain::{KnockoutPhase, KnockoutPhaseState, KnockoutStage, RepositoryError};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::mappers::{classify_write_error, parse_state, LifecycleState};

/// Postgres-backed `KnockoutPhaseRepository`.
pub struct PgKnockoutPhaseRepository {
    pub pool: PgPool,
}

impl PgKnockoutPhaseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl KnockoutPhaseRepository for PgKnockoutPhaseRepository {
    /// Insert or refresh a phase. Natural key: `(tournament_id, stage)`.
    /// On conflict, mutable display fields (`position`, `display_name`,
    /// `deadline_at`, `state`) are updated; immutable identity columns
    /// (`id`, `tournament_id`, `stage`, `created_at`) are left alone.
    async fn upsert(&self, phase: &KnockoutPhase) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO knockout_phases
                (id, tournament_id, stage, position, display_name,
                 deadline_at, state, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (tournament_id, stage) DO UPDATE
            SET position     = EXCLUDED.position,
                display_name = EXCLUDED.display_name,
                deadline_at  = EXCLUDED.deadline_at,
                state        = EXCLUDED.state
            "#,
        )
        .bind(phase.id)
        .bind(phase.tournament_id)
        .bind(phase.stage.as_db_str())
        .bind(phase.position)
        .bind(&phase.display_name)
        .bind(phase.deadline_at)
        .bind(phase.state.as_db_str())
        .bind(phase.created_at)
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<KnockoutPhase>> {
        let row = sqlx::query(&format!("{SELECT_PHASE_COLUMNS} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .change_context(RepositoryError::Backend)?;

        row.map(row_to_phase).transpose()
    }

    async fn find_by_stage(
        &self,
        tournament_id: Uuid,
        stage: KnockoutStage,
    ) -> RepoResult<Option<KnockoutPhase>> {
        let row = sqlx::query(&format!(
            "{SELECT_PHASE_COLUMNS} WHERE tournament_id = $1 AND stage = $2"
        ))
        .bind(tournament_id)
        .bind(stage.as_db_str())
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_phase).transpose()
    }

    async fn list_by_tournament(&self, tournament_id: Uuid) -> RepoResult<Vec<KnockoutPhase>> {
        let rows = sqlx::query(&format!(
            "{SELECT_PHASE_COLUMNS} WHERE tournament_id = $1 ORDER BY position ASC"
        ))
        .bind(tournament_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_phase).collect()
    }

    async fn list_active(&self, tournament_id: Uuid) -> RepoResult<Vec<KnockoutPhase>> {
        let rows = sqlx::query(&format!(
            "{SELECT_PHASE_COLUMNS} WHERE tournament_id = $1 \
             AND state = 'open' AND deadline_at > now() \
             ORDER BY position ASC"
        ))
        .bind(tournament_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_phase).collect()
    }

    async fn set_state(&self, phase_id: Uuid, state: KnockoutPhaseState) -> RepoResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE knockout_phases
            SET state = $2
            WHERE id = $1
            "#,
        )
        .bind(phase_id)
        .bind(state.as_db_str())
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if result.rows_affected() == 0 {
            return Err(Report::new(RepositoryError::NotFound)
                .attach(format!("knockout_phase {phase_id} not found")));
        }
        Ok(())
    }

    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<KnockoutPhase>> {
        let rows = sqlx::query(&format!(
            "{SELECT_PHASE_COLUMNS} \
             WHERE state = 'open' AND deadline_at <= $1 \
             ORDER BY deadline_at ASC"
        ))
        .bind(before)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_phase).collect()
    }
}

const SELECT_PHASE_COLUMNS: &str = "SELECT id, tournament_id, stage, position, \
    display_name, deadline_at, state, created_at FROM knockout_phases";

fn row_to_phase(row: sqlx::postgres::PgRow) -> RepoResult<KnockoutPhase> {
    let stage_raw: String = row.get("stage");
    let state_raw: String = row.get("state");
    let stage = KnockoutStage::from_db_str(&stage_raw).ok_or_else(|| {
        Report::new(RepositoryError::Backend)
            .attach(format!("unknown knockout stage in DB: {stage_raw}"))
    })?;
    Ok(KnockoutPhase {
        id: row.get("id"),
        tournament_id: row.get("tournament_id"),
        stage,
        position: row.get("position"),
        display_name: row.get("display_name"),
        deadline_at: row.get("deadline_at"),
        state: parse_state(&state_raw)?,
        created_at: row.get("created_at"),
    })
}
