use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;

/// Body of `POST /api/predictions`. The Mini App posts a batch of
/// per-match predictions in one request to keep round-trips low.
#[allow(dead_code)] // fields are read once handler logic lands
#[derive(Debug, Deserialize)]
pub struct SubmitPredictionsRequest {
    pub round_id: Uuid,
    pub predictions: Vec<PredictionInput>,
}

#[allow(dead_code)] // fields are read once handler logic lands
#[derive(Debug, Deserialize)]
pub struct PredictionInput {
    pub match_id: Uuid,
    pub home: i32,
    pub away: i32,
}

#[derive(Debug, Serialize)]
pub struct SubmitPredictionsResponse {
    pub accepted: usize,
}

/// `POST /api/predictions` — upsert a batch of predictions for the caller.
pub async fn submit(
    Json(_body): Json<SubmitPredictionsRequest>,
) -> Result<Json<SubmitPredictionsResponse>, ApiError> {
    // TODO: extract user id from validated init-data, validate round still
    // open, upsert via PgPredictionRepository, publish PredictionsSubmitted.
    todo!("routes::predictions::submit")
}
