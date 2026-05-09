//! HTTP API service consumed by the Mini App.
//!
//! Single Axum router, with the `auth::verify_init_data` middleware applied
//! to every route under `/api/*` except `/api/health`.

mod app_state;
mod error;
mod middleware;
mod routes;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use app_state::AppState;
use axum::http::{header, HeaderName, HeaderValue, Method};
use axum::Router;
use shared::Config;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "api",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;

    let pool = persistence::init_pool(config.database_url.expose()).await?;
    let _nats = async_nats::connect(&config.nats_url).await?;

    let secret_key = Arc::new(middleware::auth::derive_secret_key(
        config.telegram_bot_token.expose(),
    ));
    let state = AppState::new(pool);

    let cors = CorsLayer::new()
        .allow_origin(config.miniapp_origin.parse::<HeaderValue>()?)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            HeaderName::from_static("x-telegram-init-data"),
        ])
        .max_age(Duration::from_secs(3600));

    let app: Router = Router::new()
        .merge(routes::router(state, secret_key))
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr: SocketAddr = config.api_bind_addr.parse()?;
    info!(%addr, "api service listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
