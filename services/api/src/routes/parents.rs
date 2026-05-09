//! `GET` handlers for the structural parents of matches:
//! `tournament_groups` (group stage) and `knockout_phases` (knockout
//! bracket). Match-listing endpoints also fold in the caller's existing
//! predictions so the Mini App can render with one round-trip per parent.

use std::collections::HashMap;

use axum::extract::{Extension, Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use domain::{
    KnockoutPhase, KnockoutPhaseState, Match, Team, TelegramUserId, TournamentGroupState,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use shared::V1_TOURNAMENT_EXTERNAL_ID;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::routes::ERR_SUBMISSION_CLOSED;

/// Query parameters for the `*/matches` endpoints. The Mini App reads the
/// pickem id from `start_param` of `initData` (set by the bot's inline
/// button) and forwards it here so we can index per-pickem predictions.
#[derive(Debug, Deserialize)]
pub struct PickemQuery {
    pub pickem: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ActiveGroupResponse {
    pub id: Uuid,
    pub name: String,
    pub deadline_at: DateTime<Utc>,
    pub state: TournamentGroupState,
    pub teams: Vec<Team>,
}

#[derive(Debug, Serialize)]
pub struct ActiveKnockoutResponse {
    pub id: Uuid,
    pub stage: domain::KnockoutStage,
    pub position: i16,
    pub display_name: String,
    pub deadline_at: DateTime<Utc>,
    pub state: KnockoutPhaseState,
}

#[derive(Debug, Serialize)]
pub struct MatchWithPrediction {
    #[serde(rename = "match")]
    pub match_: Match,
    pub my_prediction: Option<MyPrediction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyPrediction {
    pub reg_home: i32,
    pub reg_away: i32,
    pub advancement_winner_team_id: Option<Uuid>,
    pub pens_winner_team_id: Option<Uuid>,
    pub was_changed: bool,
}

/// Resolve the v1 tournament UUID from its upstream code. Looked up per
/// request — if the row is missing the API cannot function, so we surface a
/// 500 with a clear cause. Plan note: cacheable via `OnceCell` if it
/// becomes a hot-path concern (one query per page-load is fine for v1).
async fn resolve_tournament_id(state: &AppState) -> Result<Uuid, ApiError> {
    let t = state
        .tournaments
        .find_by_external_id(V1_TOURNAMENT_EXTERNAL_ID)
        .await?
        .ok_or_else(|| {
            ApiError::Internal(format!(
                "tournament '{V1_TOURNAMENT_EXTERNAL_ID}' is not seeded"
            ))
        })?;
    Ok(t.id)
}

/// `GET /api/tournament-groups/active` — every group the caller can
/// predict on, with its 4 teams embedded so the Mini App's standings UI
/// can render with a single round-trip.
pub async fn active_groups(
    State(state): State<AppState>,
    Extension(_user_id): Extension<TelegramUserId>,
) -> Result<Json<Vec<ActiveGroupResponse>>, ApiError> {
    let tournament_id = resolve_tournament_id(&state).await?;
    let groups = state.tournament_groups.list_active(tournament_id).await?;

    let mut response = Vec::with_capacity(groups.len());
    for g in groups {
        let teams = state.tournament_groups.list_teams_in_group(g.id).await?;
        response.push(ActiveGroupResponse {
            id: g.id,
            name: g.name,
            deadline_at: g.deadline_at,
            state: g.state,
            teams,
        });
    }

    Ok(Json(response))
}

/// `GET /api/tournament-groups/{id}/matches?pickem=<uuid>` — fixtures of
/// a single tournament group with the caller's existing predictions
/// merged per match. 403 if the caller is not a member of `pickem`.
pub async fn group_matches(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Path(group_id): Path<Uuid>,
    Query(q): Query<PickemQuery>,
) -> Result<Json<Vec<MatchWithPrediction>>, ApiError> {
    require_membership(&state, q.pickem, user_id).await?;

    if state.tournament_groups.find(group_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }

    let matches = state.matches.list_by_tournament_group(group_id).await?;
    Ok(Json(
        merge_predictions(&state, q.pickem, user_id, matches).await?,
    ))
}

/// `GET /api/knockouts/active` — every knockout phase open for predictions.
pub async fn active_knockouts(
    State(state): State<AppState>,
    Extension(_user_id): Extension<TelegramUserId>,
) -> Result<Json<Vec<ActiveKnockoutResponse>>, ApiError> {
    let tournament_id = resolve_tournament_id(&state).await?;
    let phases = state.knockout_phases.list_active(tournament_id).await?;

    let response = phases
        .into_iter()
        .map(|p: KnockoutPhase| ActiveKnockoutResponse {
            id: p.id,
            stage: p.stage,
            position: p.position,
            display_name: p.display_name,
            deadline_at: p.deadline_at,
            state: p.state,
        })
        .collect();
    Ok(Json(response))
}

/// `GET /api/knockouts/{id}/matches?pickem=<uuid>` — fixtures of a single
/// knockout phase with the caller's existing predictions merged.
pub async fn knockout_matches(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Path(phase_id): Path<Uuid>,
    Query(q): Query<PickemQuery>,
) -> Result<Json<Vec<MatchWithPrediction>>, ApiError> {
    require_membership(&state, q.pickem, user_id).await?;

    if state.knockout_phases.find(phase_id).await?.is_none() {
        return Err(ApiError::NotFound);
    }

    let matches = state.matches.list_by_knockout_phase(phase_id).await?;
    Ok(Json(
        merge_predictions(&state, q.pickem, user_id, matches).await?,
    ))
}

/// Reject with 403 when the caller is not a member of the pickem they're
/// trying to act on.
pub(crate) async fn require_membership(
    state: &AppState,
    pickem_group_id: Uuid,
    user_id: TelegramUserId,
) -> Result<(), ApiError> {
    if state.groups.is_member(pickem_group_id, user_id).await? {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

async fn merge_predictions(
    state: &AppState,
    pickem_group_id: Uuid,
    user_id: TelegramUserId,
    matches: Vec<Match>,
) -> Result<Vec<MatchWithPrediction>, ApiError> {
    let predictions = state
        .predictions
        .list_by_user_in_pickem(user_id, pickem_group_id)
        .await?;
    let by_match: HashMap<Uuid, MyPrediction> = predictions
        .into_iter()
        .map(|p| {
            (
                p.match_id,
                MyPrediction {
                    reg_home: p.reg_home,
                    reg_away: p.reg_away,
                    advancement_winner_team_id: p.advancement_winner_team_id,
                    pens_winner_team_id: p.pens_winner_team_id,
                    was_changed: p.was_changed,
                },
            )
        })
        .collect();

    Ok(matches
        .into_iter()
        .map(|m| MatchWithPrediction {
            my_prediction: by_match.get(&m.id).cloned(),
            match_: m,
        })
        .collect())
}

/// Standings and best-thirds lock at the kickoff of the first group-stage
/// match (the earliest `deadline_at` among active tournament groups).
/// Returns `Ok(())` when the window is still open, `BadRequest` otherwise.
pub(crate) async fn enforce_global_group_deadline(state: &AppState) -> Result<(), ApiError> {
    let tid = resolve_tournament_id(state).await?;
    let earliest = state
        .tournament_groups
        .list_active(tid)
        .await?
        .into_iter()
        .map(|g| g.deadline_at)
        .min();
    match earliest {
        Some(deadline) if Utc::now() < deadline => Ok(()),
        _ => Err(ApiError::BadRequest(ERR_SUBMISSION_CLOSED.into())),
    }
}
