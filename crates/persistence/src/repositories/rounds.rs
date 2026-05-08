//! Postgres-backed `RoundRepository`.
//!
//! Owned by `services/events`: only the events service writes rounds (the
//! ingester bootstrap creates them once per tournament). The api and the bot
//! may READ to list active rounds.
//!
//! The `state` and `phase` columns are stored as plain TEXT with explicit
//! string values so a `psql` operator can grep them; the conversion happens
//! at the edge of this module via private helpers.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{RepoResult, RoundRepository};
use domain::{RepositoryError, Round, RoundState};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::mappers::{classify_write_error, phase_from_str, phase_to_str};

/// Postgres-backed `RoundRepository`.
pub struct PgRoundRepository {
    pub pool: PgPool,
}

impl PgRoundRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RoundRepository for PgRoundRepository {
    /// Insert a new round. Surfaces `RepositoryError::Integrity` if the
    /// `tournament_id` foreign key does not point at an existing tournament,
    /// or if a `(tournament_id, name)` collision is hit.
    async fn create(&self, round: &Round) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO rounds (id, tournament_id, name, deadline_at, state, phase, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(round.id)
        .bind(round.tournament_id)
        .bind(&round.name)
        .bind(round.deadline_at)
        .bind(state_to_str(round.state))
        .bind(phase_to_str(round.phase))
        .bind(round.created_at)
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    /// Rounds of a tournament currently accepting predictions. "Active" =
    /// state is `open` AND the deadline has not passed yet — both matter
    /// because the api enforces deadline at write time (no scheduler flips
    /// the row to `closed` in v1).
    async fn list_active(&self, tournament_id: Uuid) -> RepoResult<Vec<Round>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tournament_id, name, deadline_at, state, phase, created_at
            FROM rounds
            WHERE tournament_id = $1
              AND state = 'open'
              AND deadline_at > now()
            ORDER BY deadline_at ASC
            "#,
        )
        .bind(tournament_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_round).collect()
    }

    /// Move a round to a new lifecycle state. Used by the (currently dormant)
    /// scheduler to flip `open → closed` at deadline, and by the scorer to
    /// flip `closed → scored` after every match in the round has been scored.
    async fn set_state(&self, round_id: Uuid, state: RoundState) -> RepoResult<()> {
        let result = sqlx::query(
            r#"
            UPDATE rounds
            SET state = $2
            WHERE id = $1
            "#,
        )
        .bind(round_id)
        .bind(state_to_str(state))
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        if result.rows_affected() == 0 {
            return Err(Report::new(RepositoryError::NotFound)
                .attach(format!("round {round_id} not found")));
        }
        Ok(())
    }

    /// Rounds whose deadline has passed but are still in `open`. The
    /// scheduler uses this to drive the "flip to closed" tick. Returns an
    /// empty vector when no round is due — never an error.
    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<Round>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tournament_id, name, deadline_at, state, phase, created_at
            FROM rounds
            WHERE state = 'open'
              AND deadline_at <= $1
            ORDER BY deadline_at ASC
            "#,
        )
        .bind(before)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_round).collect()
    }
}

impl PgRoundRepository {
    /// Look a round up by its id. Convenience for the seed CLI / scorer.
    pub async fn find(&self, id: Uuid) -> RepoResult<Option<Round>> {
        let row = sqlx::query(
            r#"
            SELECT id, tournament_id, name, deadline_at, state, phase, created_at
            FROM rounds
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_round).transpose()
    }

    /// Look a round up by `(tournament_id, name)` — the natural key under
    /// which the bootstrap creates rounds idempotently. The matching UNIQUE
    /// is enforced in migration `0002`.
    pub async fn find_by_name(
        &self,
        tournament_id: Uuid,
        name: &str,
    ) -> RepoResult<Option<Round>> {
        let row = sqlx::query(
            r#"
            SELECT id, tournament_id, name, deadline_at, state, phase, created_at
            FROM rounds
            WHERE tournament_id = $1 AND name = $2
            "#,
        )
        .bind(tournament_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_round).transpose()
    }
}

/// Convert a row into a domain `Round`, surfacing a typed error when the
/// `state` / `phase` columns hold an unrecognised string. We treat that as
/// `Backend` (not `Integrity`) because it means our schema and our enum
/// drifted — a bug in our code, not a constraint violation by the caller.
fn row_to_round(row: sqlx::postgres::PgRow) -> RepoResult<Round> {
    let state_raw: String = row.get("state");
    let phase_raw: String = row.get("phase");
    Ok(Round {
        id: row.get("id"),
        tournament_id: row.get("tournament_id"),
        name: row.get("name"),
        deadline_at: row.get("deadline_at"),
        state: state_from_str(&state_raw)?,
        phase: phase_from_str(&phase_raw)?,
        created_at: row.get("created_at"),
    })
}

/// Canonical string for a `RoundState`. Mirrors the `#[serde(rename_all =
/// "lowercase")]` on the enum and the values seeded in `0002`.
fn state_to_str(state: RoundState) -> &'static str {
    match state {
        RoundState::Open => "open",
        RoundState::Closed => "closed",
        RoundState::Scored => "scored",
    }
}

fn state_from_str(s: &str) -> RepoResult<RoundState> {
    match s {
        "open" => Ok(RoundState::Open),
        "closed" => Ok(RoundState::Closed),
        "scored" => Ok(RoundState::Scored),
        other => Err(Report::new(RepositoryError::Backend)
            .attach(format!("unknown round state in DB: {other}"))),
    }
}
