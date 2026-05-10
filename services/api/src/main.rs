//! HTTP API service consumed by the Mini App.
//!
//! Single Axum router, with the `auth::verify_init_data` middleware applied
//! to every route under `/api/*` except `/api/health`.

mod error;
mod middleware;
mod routes;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use shared::Config;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().map_err(shared::report_to_anyhow)?;
    let _tracing_guard = shared::tracing::init(
        "api",
        config.otel_endpoint.as_deref(),
        config.otel_service_namespace.as_deref(),
    )?;

    let _pool = persistence::init_pool(config.database_url.expose()).await?;
    let _nats = async_nats::connect(&config.nats_url).await?;

    let secret_key = Arc::new(middleware::auth::derive_secret_key(
        config.telegram_bot_token.expose(),
    ));

    let app: Router = Router::new()
        .merge(routes::router(secret_key))
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr: SocketAddr = config.api_bind_addr.parse()?;
    info!(%addr, "api service listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
