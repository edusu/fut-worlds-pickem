//! `GET` handlers for the structural parents of matches:
//! `tournament_groups` (group stage) and `knockout_phases` (knockout
//! bracket). Replaces the old `routes::rounds` module — every former
//! "round" route now lives under one of these two surfaces, with the
//! parent kind explicit in the URL.

use axum::extract::Path;
use axum::Json;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ApiError;

/// `GET /api/groups/active` — list tournament_groups the caller can
/// predict on (state `open` and deadline still in the future).
pub async fn active_groups() -> Result<Json<Value>, ApiError> {
    todo!("routes::parents::active_groups")
}

/// `GET /api/groups/{id}/matches` — list matches that belong to a single
/// tournament_group.
pub async fn group_matches(Path(_id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    todo!("routes::parents::group_matches")
}

/// `GET /api/knockouts/active` — list knockout_phases the caller can
/// predict on (state `open` and deadline still in the future).
pub async fn active_knockouts() -> Result<Json<Value>, ApiError> {
    todo!("routes::parents::active_knockouts")
}

/// `GET /api/knockouts/{id}/matches` — list matches that belong to a
/// single knockout_phase.
pub async fn knockout_matches(Path(_id): Path<Uuid>) -> Result<Json<Value>, ApiError> {
    todo!("routes::parents::knockout_matches")
}
