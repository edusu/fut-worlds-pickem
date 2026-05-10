pub mod jobs;

use rust_utils::network::nats::NatsClient;
use shared::Config;
use sqlx::PgPool;

/// Run the cron scheduler loop. Registers jobs in `jobs::register` and blocks
/// forever (until shutdown).
pub async fn run(_config: Config, _pool: PgPool, _nats: NatsClient) -> anyhow::Result<()> {
    todo!("scheduler::run")
}
