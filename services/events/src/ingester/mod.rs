pub mod football_data;

use shared::Config;
use sqlx::PgPool;

/// Run the periodic ingestion loop. Polls the upstream provider every 5 min,
/// upserts matches, and emits `MatchLive` / `MatchFinished` for state changes.
pub async fn run(_config: Config, _pool: PgPool, _nats: async_nats::Client) -> anyhow::Result<()> {
    todo!("ingester::run")
}
