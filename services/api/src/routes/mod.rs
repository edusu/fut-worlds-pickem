//! HTTP route declarations. Public surface consumed by the Mini App.
//!
//! `/api/health` is unauthenticated; everything else goes through
//! `middleware::auth::verify_init_data`.
//!
//! Two distinct "group" concepts share this surface, distinguished by
//! URL prefix: `/api/groups/{id}/...` refers to **pickem** groups (a
//! Telegram chat bound to a pickem); `/api/tournament-groups/{id}/...`
//! refers to a **tournament** group (Group A through Group L). The Mini
//! App is the sole consumer and routes consistently.

mod parents;
mod predictions;
mod ranking;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use crate::middleware::auth::SecretKey;

/// Router factory. Takes the pre-derived HMAC secret key used by the auth
/// middleware to validate `initData` (see
/// [`middleware::auth::derive_secret_key`]).
pub fn router(secret_key: Arc<SecretKey>) -> Router {
    let protected = Router::new()
        .route("/api/tournament-groups/active", get(parents::active_groups))
        .route(
            "/api/tournament-groups/{id}/matches",
            get(parents::group_matches),
        )
        .route("/api/knockouts/active", get(parents::active_knockouts))
        .route(
            "/api/knockouts/{id}/matches",
            get(parents::knockout_matches),
        )
        .route("/api/predictions", post(predictions::submit))
        .route("/api/groups/{id}/ranking", get(ranking::group_ranking))
        .layer(axum::middleware::from_fn_with_state(
            secret_key,
            crate::middleware::auth::verify_init_data,
        ));

    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
