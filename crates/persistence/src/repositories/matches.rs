//! Postgres-backed `MatchRepository`.
//!
//! Owned by `services/events`: the ingester upserts every fixture, the
//! scorer reads via `find` to load a match plus its ET / pens columns, and
//! the api reads via `list_by_round` to render the Mini App's match list.
//!
//! Idempotency hinges on the `UNIQUE (round_id, external_id)` index defined
//! in migration `0002`: re-pulling the same upstream fixture rewrites the
//! mutable columns (status, scores, ET, pens) in place without producing a
//! duplicate row, so the ingester loop can run as often as it likes.

use async_trait::async_trait;
use domain::repository::{MatchRepository, RepoResult};
use domain::{Match, MatchStatus, Phase, RepositoryError};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Postgres-backed `MatchRepository`.
pub struct PgMatchRepository {
    pub pool: PgPool,
}

impl PgMatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MatchRepository for PgMatchRepository {
    /// Insert a new match or refresh the mutable columns on an existing one.
    ///
    /// The `(round_id, external_id)` UNIQUE constraint serves as the natural
    /// key: re-running the ingester rewrites status, scores, ET and pens
    /// without producing a second row. We deliberately also overwrite
    /// `home_team` / `away_team` / flags / `phase` / `kickoff_at` because
    /// the upstream is allowed to amend any of them (kickoff reschedules,
    /// "Winner of QF1" being resolved into a real team name once the QF
    /// finishes) and the latest pull is authoritative.
    ///
    /// `id` is preserved on conflict so foreign keys (`predictions.match_id`,
    /// `pens_winner_team_id` references coming back from `predictions`,
    /// etc.) stay valid — only the freshly-arriving DTO's values are
    /// rewritten into the existing row.
    ///
    /// Surfaces `RepositoryError::Integrity` for FK / CHECK violations
    /// (e.g. ET pair invariant, `pens_winner_team_id` pointing at an unknown
    /// team) and `RepositoryError::Backend` for everything else.
    async fn upsert(&self, match_: &Match) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO matches (
                id, round_id, external_id,
                home_team, away_team, home_flag, away_flag,
                kickoff_at,
                home_score, away_score,
                et_home_score, et_away_score, pens_winner_team_id,
                status, phase
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (round_id, external_id) DO UPDATE
            SET home_team           = EXCLUDED.home_team,
                away_team           = EXCLUDED.away_team,
                home_flag           = EXCLUDED.home_flag,
                away_flag           = EXCLUDED.away_flag,
                kickoff_at          = EXCLUDED.kickoff_at,
                home_score          = EXCLUDED.home_score,
                away_score          = EXCLUDED.away_score,
                et_home_score       = EXCLUDED.et_home_score,
                et_away_score       = EXCLUDED.et_away_score,
                pens_winner_team_id = EXCLUDED.pens_winner_team_id,
                status              = EXCLUDED.status,
                phase               = EXCLUDED.phase
            "#,
        )
        .bind(match_.id)
        .bind(match_.round_id)
        .bind(&match_.external_id)
        .bind(&match_.home_team)
        .bind(&match_.away_team)
        .bind(&match_.home_flag)
        .bind(&match_.away_flag)
        .bind(match_.kickoff_at)
        .bind(match_.home_score)
        .bind(match_.away_score)
        .bind(match_.et_home_score)
        .bind(match_.et_away_score)
        .bind(match_.pens_winner_team_id)
        .bind(status_to_str(match_.status))
        .bind(phase_to_str(match_.phase))
        .execute(&self.pool)
        .await
        .map_err(|e| {
            let kind = match &e {
                sqlx::Error::Database(db)
                    if db.is_foreign_key_violation()
                        || db.is_check_violation()
                        || db.is_unique_violation() =>
                {
                    RepositoryError::Integrity
                }
                _ => RepositoryError::Backend,
            };
            Report::new(e).change_context(kind)
        })?;

        Ok(())
    }

    async fn list_by_round(&self, round_id: Uuid) -> RepoResult<Vec<Match>> {
        let rows = sqlx::query(
            r#"
            SELECT id, round_id, external_id,
                   home_team, away_team, home_flag, away_flag,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status, phase
            FROM matches
            WHERE round_id = $1
            ORDER BY kickoff_at ASC
            "#,
        )
        .bind(round_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        rows.into_iter().map(row_to_match).collect()
    }

    async fn find(&self, id: Uuid) -> RepoResult<Option<Match>> {
        let row = sqlx::query(
            r#"
            SELECT id, round_id, external_id,
                   home_team, away_team, home_flag, away_flag,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status, phase
            FROM matches
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_match).transpose()
    }
}

impl PgMatchRepository {
    /// Find a match by its `(round_id, external_id)` natural key. The
    /// ingester uses this to read the *previous* state of a match before
    /// upserting, so it can detect transitions (`scheduled → finished`) and
    /// emit `MatchFinished::V1` exactly once per transition rather than on
    /// every poll.
    pub async fn find_by_external(
        &self,
        round_id: Uuid,
        external_id: &str,
    ) -> RepoResult<Option<Match>> {
        let row = sqlx::query(
            r#"
            SELECT id, round_id, external_id,
                   home_team, away_team, home_flag, away_flag,
                   kickoff_at,
                   home_score, away_score,
                   et_home_score, et_away_score, pens_winner_team_id,
                   status, phase
            FROM matches
            WHERE round_id = $1 AND external_id = $2
            "#,
        )
        .bind(round_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_match).transpose()
    }
}

/// Convert a row into a domain `Match`, surfacing typed errors when the
/// `status` / `phase` columns hold an unrecognised string (schema↔enum
/// drift = bug in our code, mapped to `Backend`).
fn row_to_match(row: sqlx::postgres::PgRow) -> RepoResult<Match> {
    let status_raw: String = row.get("status");
    let phase_raw: String = row.get("phase");
    Ok(Match {
        id: row.get("id"),
        round_id: row.get("round_id"),
        external_id: row.get("external_id"),
        home_team: row.get("home_team"),
        away_team: row.get("away_team"),
        home_flag: row.get("home_flag"),
        away_flag: row.get("away_flag"),
        kickoff_at: row.get("kickoff_at"),
        home_score: row.get("home_score"),
        away_score: row.get("away_score"),
        et_home_score: row.get("et_home_score"),
        et_away_score: row.get("et_away_score"),
        pens_winner_team_id: row.get("pens_winner_team_id"),
        status: status_from_str(&status_raw)?,
        phase: phase_from_str(&phase_raw)?,
    })
}

/// Canonical strings for `MatchStatus`. Match the `serde(rename_all =
/// "lowercase")` on the enum and the values used by the schema's CHECK-free
/// status column (we don't constrain the column at the DB level — the
/// adapter is the gatekeeper).
fn status_to_str(status: MatchStatus) -> &'static str {
    match status {
        MatchStatus::Scheduled => "scheduled",
        MatchStatus::Live => "live",
        MatchStatus::Finished => "finished",
        MatchStatus::Postponed => "postponed",
        MatchStatus::Cancelled => "cancelled",
    }
}

fn status_from_str(s: &str) -> RepoResult<MatchStatus> {
    match s {
        "scheduled" => Ok(MatchStatus::Scheduled),
        "live" => Ok(MatchStatus::Live),
        "finished" => Ok(MatchStatus::Finished),
        "postponed" => Ok(MatchStatus::Postponed),
        "cancelled" => Ok(MatchStatus::Cancelled),
        other => Err(Report::new(RepositoryError::Backend)
            .attach(format!("unknown match status in DB: {other}"))),
    }
}

fn phase_to_str(phase: Phase) -> &'static str {
    match phase {
        Phase::Group => "group",
        Phase::Knockout => "knockout",
    }
}

fn phase_from_str(s: &str) -> RepoResult<Phase> {
    match s {
        "group" => Ok(Phase::Group),
        "knockout" => Ok(Phase::Knockout),
        other => {
            Err(Report::new(RepositoryError::Backend)
                .attach(format!("unknown phase in DB: {other}")))
        }
    }
}
