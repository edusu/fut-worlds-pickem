//! HTTP API service consumed by the Mini App.
//!
//! Single Axum router, with the `auth::verify_init_data` middleware applied
//! to every route under `/api/*` except `/api/health`.

mod error;
mod middleware;
mod routes;

use axum::Router;
use shared::Config;
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    shared::tracing::init("api")?;
    let config = Config::from_env()?;

    let _pool = persistence::init_pool(&config.database_url).await?;
    let _nats = async_nats::connect(&config.nats_url).await?;

    let app: Router = Router::new()
        .merge(routes::router())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let addr: SocketAddr = config.api_bind_addr.parse()?;
    info!(%addr, "api service listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
