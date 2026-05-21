//! `GET /api/groups/{id}/ranking` — pickem leaderboard.
//!
//! Returns the full roster (one row per `group_members` entry) so members
//! with zero scored points still appear at the bottom; the Mini App
//! renders a stable list rather than a "leaderboard with gaps".

use axum::extract::{Extension, Path, State};
use axum::Json;
use domain::{RankingEntry, TelegramUserId};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::routes::parents::require_membership;

/// `GET /api/groups/{id}/ranking` — leaderboard for a pickem group.
///
/// Gated on membership (403 otherwise) so a leaked group UUID cannot be
/// used to enumerate someone else's pickem from outside. `RankingEntry`
/// is serialized directly: `TelegramUserId` is `#[serde(transparent)]`
/// over `i64`, so the wire shape matches what the Mini App expects
/// without a separate DTO.
pub async fn group_ranking(
    State(state): State<AppState>,
    Extension(user_id): Extension<TelegramUserId>,
    Path(group_id): Path<Uuid>,
) -> Result<Json<Vec<RankingEntry>>, ApiError> {
    require_membership(&state, group_id, user_id).await?;
    Ok(Json(state.ranking.group_ranking(group_id).await?))
}
