use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Build a Postgres connection pool with sensible defaults for our services.
///
/// `database_url` is a standard `postgres://` URL. We cap connections at 10 by
/// default — a service can override it later via `PgPoolOptions` if it needs
/// to. Connections are recycled after 30 minutes to avoid holding stale TLS
/// sessions.
pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .max_lifetime(Some(Duration::from_secs(30 * 60)))
        .connect(database_url)
        .await
}
