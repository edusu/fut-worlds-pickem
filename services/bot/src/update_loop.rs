use shared::Config;
use sqlx::PgPool;

/// Run the long-polling getUpdates loop.
///
/// Skeleton: in real life this calls `frankenstein::AsyncApi::get_updates` in a
/// loop with a growing offset, dispatches each update to `commands::dispatch`,
/// and reports errors via `tracing`.
pub async fn run(_config: Config, _pool: PgPool) -> anyhow::Result<()> {
    // TODO: build the Frankenstein client, run the long-polling loop, dispatch.
    todo!("update_loop::run")
}
