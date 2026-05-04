//! HTTP route declarations. Public surface consumed by the Mini App.
//!
//! `/api/health` is unauthenticated; everything else goes through
//! `middleware::auth::verify_init_data`.

mod predictions;
mod ranking;
mod rounds;

use axum::routing::{get, post};
use axum::Router;

pub fn router() -> Router {
    let protected = Router::new()
        .route("/api/rounds/active", get(rounds::active))
        .route("/api/rounds/{id}/matches", get(rounds::matches))
        .route("/api/predictions", post(predictions::submit))
        .route("/api/groups/{id}/ranking", get(ranking::group_ranking))
        .layer(axum::middleware::from_fn(
            crate::middleware::auth::verify_init_data,
        ));

    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
}

async fn health() -> &'static str {
    "ok"
}
