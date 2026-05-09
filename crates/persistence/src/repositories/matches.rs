//! Postgres-backed `MatchRepository`.
//!
//! Owned by `services/events`: the ingester upserts every fixture, the
//! scorer reads via `find` to load a match plus its ET / pens columns, and
//! the api reads via `list_by_tournament_group` / `list_by_knockout_phase`
//! to render the Mini App's match lists.
//!
//! Idempotency hinges on the `UNIQUE (external_id)` index defined in
//! migration `0002`: re-pulling the same upstream fixture rewrites the
//! mutable columns (parent FK, team FKs, status, scores, ET, pens) in
//! place without producing a duplicate row, so the ingester loop can run
//! as often as it likes. football-data.org match ids are globally unique
//! so a global UNIQUE is sufficient.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{MatchRepository, RepoResult};
use domain::{Match, MatchStatus, RepositoryError};
use error_stack::{Report, ResultExt};
use sqlx::PgPool;
use uuid::Uuid;

use super::mappers::classify_write_error;

/// Postgres-backed `MatchRepository`.
pub struct PgMatchRepository {
    pub pool: PgPool,
}

impl PgMatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Mirror of the `matches` row at the schema level, used by the
/// `query_as!` SELECTs. Centralises the row → domain conversion (notably
/// the `status: String` → `MatchStatus` parse) in `into_domain`.
struct MatchRow {
    id: Uuid,
    external_id: String,
    tournament_group_id: Option<Uuid>,
    knockout_phase_id: Option<Uuid>,
    home_team_id: Option<Uuid>,
    away_team_id: Option<Uuid>,
    kickoff_at: DateTime<Utc>,
    home_score: Option<i32>,
    away_score: Option<i32>,
    et_home_score: Option<i32>,
    et_away_score: Option<i32>,
    pens_winner_team_id: Option<Uuid>,
    status: String,
}

impl MatchRow {
    /// Surfaces `RepositoryError::Backend` if the `status` column carries a
    /// value outside the `matches_status_valid` CHECK set — that would
    /// indicate schema↔enum drift inside our own code.
    fn into_domain(self) -> RepoResult<Match> {
        Ok(Match {
            id: self.id,
            external_id: self.external_id,
            tournament_group_id: self.tournament_group_id,
            knockout_phase_id: self.knockout_phase_id,
            home_team_id: self.home_team_id,
            away_team_id: self.away_team_id,
            kickoff_at: self.kickoff_at,
            home_score: self.home_score,
            away_score: self.away_score,
            et_home_score: self.et_home_score,
            et_away_score: self.et_away_score,
            pens_winner_team_id: self.pens_winner_team_id,
            status: status_from_str(&self.status)?,
        })
    }
}

#[async_trait]
impl MatchRepository for PgMatchRepository {
    /// Insert a new match or refresh the mutable columns on an existing
    /// one. The `(external_id)` UNIQUE serves as the natural key: re-running
    /// the ingester rewrites parent FK, teams, kickoff, status, scores, ET
    /// and pens without producing a second row. We deliberately overwrite
    /// the parent FKs and team FKs because the upstream is allowed to
    /// resolve placeholders ("Winner of QF1" → real team UUID) once the
    /// bracket advances.
    ///
    /// `id` is preserved on conflict so foreign keys
    /// (`predictions.match_id`, etc.) stay valid — only the
    /// freshly-arriving DTO's values are rewritten into the existing row.
    ///
    /// Surfaces `RepositoryError::Integrity` for FK / CHECK violations
    /// (parent XOR, distinct teams, ET pair invariant, status enum, pens
    /// pointing at an unknown team).
    async fn upsert(&self, match_: &Match) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO matches (
                id, external_id,
                tournament_group_id, knockout_phase_id,
                home_team_id, away_team_id,
                kickoff_at,
                home_score, away_score,
                et_home_score, et_away_score, pens_winner_team_id,
                status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (external_id) DO UPDATE
            SET tournament_group_id = EXCLUDED.tournament_group_id,
                knockout_phase_id   = EXCLUDED.knockout_phase_id,
                home_team_id        = EXCLUDED.home_team_id,
                away_team_id        = EXCLUDED.away_team_id,
                kickoff_at          = EXCLUDED.kickoff_at,
                home_score          = EXCLUDED.home_score,
                away_score          = EXCLUDED.away_score,
                et_home_score       = EXCLUDED.et_home_score,
                et_away_score       = EXCLUDED.et_away_score,
                pens_winner_team_id = EXCLUDED.pens_winner_team_id,
                status              = EXCLUDED.status
            "#,
            match_.id,
            match_.external_id,
            match_.tournament_group_id,
            match_.knockout_phase_id,
            match_.home_team_id,
            match_.away_team_id,
            match_.kickoff_at,
            match_.home_score,
            match_.away_score,
            match_.et_home_score,
            match_.et_away_score,
            match_.pens_winner_team_id,
            status_to_str(match_.status),
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }

    async fn list_by_tournament_group(&self, group_id: Uuid) -> RepoResult<Vec<Match>> {
        let rows = sqlx::query_as!(
            MatchRow,
            r#"
            SELECT id, external_id,
                   tournament_group_id, knockout_phase_id,
                   home_team_id, away_team_id,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status
            FROM matches
            WHERE tournament_group_id = $1
            ORDER BY kickoff_at ASC
            "#,
            group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(MatchRow::into_domain).collect()
    }

    async fn list_by_knockout_phase(&self, phase_id: Uuid) -> RepoResult<Vec<Match>> {
        let rows = sqlx::query_as!(
            MatchRow,
            r#"
            SELECT id, external_id,
                   tournament_group_id, knockout_phase_id,
                   home_team_id, away_team_id,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status
            FROM matches
            WHERE knockout_phase_id = $1
            ORDER BY kickoff_at ASC
            "#,
            phase_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(MatchRow::into_domain).collect()
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<Match>> {
        let row = sqlx::query_as!(
            MatchRow,
            r#"
            SELECT id, external_id,
                   tournament_group_id, knockout_phase_id,
                   home_team_id, away_team_id,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status
            FROM matches
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(MatchRow::into_domain).transpose()
    }
}

impl PgMatchRepository {
    /// Find a match by its `external_id`. The ingester uses this to read
    /// the *previous* state of a match before upserting, so it can detect
    /// transitions (`scheduled → finished`) and emit `MatchFinished::V1`
    /// exactly once per transition rather than on every poll.
    pub async fn find_by_external(&self, external_id: &str) -> RepoResult<Option<Match>> {
        let row = sqlx::query_as!(
            MatchRow,
            r#"
            SELECT id, external_id,
                   tournament_group_id, knockout_phase_id,
                   home_team_id, away_team_id,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status
            FROM matches
            WHERE external_id = $1
            "#,
            external_id,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(MatchRow::into_domain).transpose()
    }
}

/// Canonical strings for `MatchStatus`. Mirror the `matches_status_valid`
/// CHECK constraint in migration `0002` 1:1.
fn status_to_str(status: MatchStatus) -> &'static str {
    match status {
        MatchStatus::Scheduled => "scheduled",
        MatchStatus::Timed => "timed",
        MatchStatus::InPlay => "in_play",
        MatchStatus::Paused => "paused",
        MatchStatus::Finished => "finished",
        MatchStatus::Suspended => "suspended",
        MatchStatus::Postponed => "postponed",
        MatchStatus::Cancelled => "cancelled",
        MatchStatus::Awarded => "awarded",
    }
}

fn status_from_str(s: &str) -> RepoResult<MatchStatus> {
    match s {
        "scheduled" => Ok(MatchStatus::Scheduled),
        "timed" => Ok(MatchStatus::Timed),
        "in_play" => Ok(MatchStatus::InPlay),
        "paused" => Ok(MatchStatus::Paused),
        "finished" => Ok(MatchStatus::Finished),
        "suspended" => Ok(MatchStatus::Suspended),
        "postponed" => Ok(MatchStatus::Postponed),
        "cancelled" => Ok(MatchStatus::Cancelled),
        "awarded" => Ok(MatchStatus::Awarded),
        other => Err(Report::new(RepositoryError::Backend)
            .attach(format!("unknown match status in DB: {other}"))),
    }
}
