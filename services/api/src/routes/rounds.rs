use axum::extract::Path;
use axum::Json;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ApiError;

/// `GET /api/rounds/active` — list rounds the caller can predict on.
pub async fn active() -> Result<Json<Value>, ApiError> {
    todo!("routes::rounds::active")
}

/// `GET /api/rounds/{id}/matches` — list matches in a round.
pub async fn matches(Path(_id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    todo!("routes::rounds::matches")
}
