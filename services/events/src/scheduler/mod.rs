pub mod jobs;

use shared::Config;
use sqlx::PgPool;

/// Run the cron scheduler loop. Registers jobs in `jobs::register` and blocks
/// forever (until shutdown).
pub async fn run(_config: Config, _pool: PgPool, _nats: async_nats::Client) -> anyhow::Result<()> {
    todo!("scheduler::run")
}
