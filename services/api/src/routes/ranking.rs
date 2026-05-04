use axum::extract::Path;
use axum::Json;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ApiError;

/// `GET /api/groups/{id}/ranking` — leaderboard for a pickem group.
pub async fn group_ranking(Path(_id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    todo!("routes::ranking::group_ranking")
}
