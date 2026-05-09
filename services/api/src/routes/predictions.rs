//! `POST` handlers for the four prediction surfaces:
//! - `submit_matches` — group-stage and knockout regulation predictions
//!   (the parent's `kind` selects the dispatch path).
//! - `submit_standings` — final ordering of the four teams in a single
//!   tournament group.
//! - `submit_best_thirds` — unordered set of exactly 8 teams predicted to
//!   advance as the best third-placed teams.
//!
//! Validation order in every handler is the same: authentication is already
//! done (the auth middleware injected `TelegramUserId`), then membership of
//! the addressed pickem, then the deadline / state gate, then the
//! shape-of-payload validation, and finally the upsert to Postgres.

use std::collections::{HashMap, HashSet};

use axum::extract::{Extension, State};
use axum::Json;
use chrono::{DateTime, Utc};
use domain::{
    BestThirdsPrediction, GroupStandingsPrediction, Match, ParentRef, Prediction, TelegramUserId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::routes::parents::{enforce_global_group_deadline, require_membership};
use crate::routes::ERR_SUBMISSION_CLOSED;

const BEST_THIRDS_REQUIRED: usize = 8;

// ---------------------------------------------------------------------------
// /api/predictions/matches
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubmitMatchesRequest {
    pub pickem_group_id: Uuid,
    pub parent: ParentRef,
    pub predictions: Vec<MatchPredictionInput>,
}

#[derive(Debug, Deserialize)]
pub struct MatchPredictionInput {
    pub match_id: Uuid,
    pub reg_home: i32,
    pub reg_away: i32,
    #[serde(default)]
    pub advancement_winner_team_id: Option<Uuid>,
    #[serde(default)]
    pub pens_winner_team_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct AcceptedResponse {
    pub accepted: usize,
}

/// `POST /api/predictions/matches` — upsert a batch of match predictions.
///
/// Validation:
/// 1. Caller must be a member of `pickem_group_id` (403 otherwise).
/// 2. The parent must exist (404), be `state == Open`, and have a deadline
///    in the future (400 otherwise).
/// 3. Every `match_id` in the body must belong to that parent (400).
/// 4. Every match's `kickoff_at` must still be in the future (400).
/// 5. For group-stage parents, `advancement_winner_team_id` and
///    `pens_winner_team_id` must be `None` (those fields only apply to
///    knockouts).
/// 6. The cross-field invariants in `Prediction::is_consistent` must hold.
///
/// Validation runs over the whole batch *before* any write so a partial
/// failure cannot leave the caller with half-updated rows.
pub async fn submit_matches(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Json(body): Json<SubmitMatchesRequest>,
) -> Result<Json<AcceptedResponse>, ApiError> {
    require_membership(&state, body.pickem_group_id, user_id).await?;

    let now = Utc::now();
    let parent_matches = resolve_parent_matches(&state, &body.parent, now).await?;
    let matches_by_id: HashMap<Uuid, &Match> = parent_matches.iter().map(|m| (m.id, m)).collect();
    let is_group_stage = matches!(body.parent, ParentRef::TournamentGroup { .. });

    let mut to_upsert = Vec::with_capacity(body.predictions.len());
    for input in &body.predictions {
        let m = matches_by_id.get(&input.match_id).ok_or_else(|| {
            ApiError::BadRequest(format!(
                "match {} does not belong to the requested parent",
                input.match_id
            ))
        })?;
        if m.kickoff_at <= now {
            return Err(ApiError::BadRequest(format!(
                "match {} has already kicked off",
                m.id
            )));
        }
        if is_group_stage
            && (input.advancement_winner_team_id.is_some() || input.pens_winner_team_id.is_some())
        {
            return Err(ApiError::BadRequest(
                "advancement_winner / pens_winner only apply to knockout matches".into(),
            ));
        }

        let prediction = Prediction {
            id: Uuid::new_v4(),
            user_id,
            pickem_group_id: body.pickem_group_id,
            match_id: input.match_id,
            reg_home: input.reg_home,
            reg_away: input.reg_away,
            advancement_winner_team_id: input.advancement_winner_team_id,
            pens_winner_team_id: input.pens_winner_team_id,
            was_changed: false,
            points_awarded: None,
            submitted_at: now,
        };
        prediction
            .is_consistent()
            .map_err(|e| ApiError::BadRequest(e.into()))?;
        to_upsert.push(prediction);
    }

    for prediction in &to_upsert {
        state.predictions.upsert(prediction).await?;
    }

    Ok(Json(AcceptedResponse {
        accepted: to_upsert.len(),
    }))
}

/// Look the parent up, gate it on `state == Open && deadline > now`, then
/// return the matches that belong to it. Single source of truth for the
/// dispatch on `ParentRef::{TournamentGroup, KnockoutPhase}` so callers
/// don't repeat the gate per arm.
async fn resolve_parent_matches(
    state: &AppState,
    parent: &ParentRef,
    now: DateTime<Utc>,
) -> Result<Vec<Match>, ApiError> {
    match parent {
        ParentRef::TournamentGroup { id } => {
            let row = state
                .tournament_groups
                .find(*id)
                .await?
                .ok_or(ApiError::NotFound)?;
            if !matches!(row.state, domain::TournamentGroupState::Open) || row.deadline_at <= now {
                return Err(ApiError::BadRequest(ERR_SUBMISSION_CLOSED.into()));
            }
            Ok(state.matches.list_by_tournament_group(*id).await?)
        }
        ParentRef::KnockoutPhase { id } => {
            let row = state
                .knockout_phases
                .find(*id)
                .await?
                .ok_or(ApiError::NotFound)?;
            if !matches!(row.state, domain::KnockoutPhaseState::Open) || row.deadline_at <= now {
                return Err(ApiError::BadRequest(ERR_SUBMISSION_CLOSED.into()));
            }
            Ok(state.matches.list_by_knockout_phase(*id).await?)
        }
    }
}

// ---------------------------------------------------------------------------
// /api/predictions/standings
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubmitStandingsRequest {
    pub pickem_group_id: Uuid,
    pub tournament_group_id: Uuid,
    pub pos1_team_id: Uuid,
    pub pos2_team_id: Uuid,
    pub pos3_team_id: Uuid,
    pub pos4_team_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

pub async fn submit_standings(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Json(body): Json<SubmitStandingsRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    require_membership(&state, body.pickem_group_id, user_id).await?;
    enforce_global_group_deadline(&state).await?;

    let group = state
        .tournament_groups
        .find(body.tournament_group_id)
        .await?
        .ok_or(ApiError::NotFound)?;

    let group_teams = state
        .tournament_groups
        .list_teams_in_group(group.id)
        .await?;
    let group_team_ids: HashSet<Uuid> = group_teams.iter().map(|t| t.id).collect();

    let positions = [
        body.pos1_team_id,
        body.pos2_team_id,
        body.pos3_team_id,
        body.pos4_team_id,
    ];
    let unique: HashSet<Uuid> = positions.iter().copied().collect();
    if unique.len() != positions.len() {
        return Err(ApiError::BadRequest(
            "position teams must be distinct".into(),
        ));
    }
    if !positions.iter().all(|id| group_team_ids.contains(id)) {
        return Err(ApiError::BadRequest(
            "every position team must belong to the addressed tournament group".into(),
        ));
    }

    let prediction = GroupStandingsPrediction {
        id: Uuid::new_v4(),
        user_id,
        pickem_group_id: body.pickem_group_id,
        tournament_group_id: body.tournament_group_id,
        pos1_team_id: body.pos1_team_id,
        pos2_team_id: body.pos2_team_id,
        pos3_team_id: body.pos3_team_id,
        pos4_team_id: body.pos4_team_id,
        points_awarded: None,
        submitted_at: Utc::now(),
    };
    state.standings.upsert(&prediction).await?;
    Ok(Json(OkResponse { ok: true }))
}

// ---------------------------------------------------------------------------
// /api/predictions/best-thirds
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SubmitBestThirdsRequest {
    pub pickem_group_id: Uuid,
    pub team_ids: Vec<Uuid>,
}

pub async fn submit_best_thirds(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Json(body): Json<SubmitBestThirdsRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    require_membership(&state, body.pickem_group_id, user_id).await?;
    enforce_global_group_deadline(&state).await?;

    if body.team_ids.len() != BEST_THIRDS_REQUIRED {
        return Err(ApiError::BadRequest(format!(
            "best-thirds expects exactly {BEST_THIRDS_REQUIRED} team_ids"
        )));
    }
    let unique: HashSet<Uuid> = body.team_ids.iter().copied().collect();
    if unique.len() != body.team_ids.len() {
        return Err(ApiError::BadRequest("team_ids must be distinct".into()));
    }

    // The teams set is small (48 rows for a World Cup), so a single
    // `list_all` is cheaper than 8 `find` round-trips.
    let valid_teams: HashSet<Uuid> = state
        .teams
        .list_all()
        .await?
        .into_iter()
        .map(|t| t.id)
        .collect();
    if !body.team_ids.iter().all(|id| valid_teams.contains(id)) {
        return Err(ApiError::BadRequest(
            "every team_id must reference an existing tournament team".into(),
        ));
    }

    let prediction = BestThirdsPrediction {
        user_id,
        pickem_group_id: body.pickem_group_id,
        team_ids: body.team_ids,
    };
    state.best_thirds.replace(&prediction).await?;
    Ok(Json(OkResponse { ok: true }))
}
