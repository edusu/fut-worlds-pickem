use messaging::{topics, MatchFinished, Subscriber};
use shared::Config;
use sqlx::PgPool;
use tracing::info;

/// Subscribe to `pickem.match.finished` and award points for every prediction
/// on the finished match using `domain::scoring::calculate_points`.
pub async fn run(_config: Config, _pool: PgPool, nats: async_nats::Client) -> anyhow::Result<()> {
    let mut sub: Subscriber<MatchFinished> =
        Subscriber::subscribe(&nats, topics::MATCH_FINISHED).await?;

    info!(topic = topics::MATCH_FINISHED, "scorer ready");

    while let Some(_event) = sub.next().await {
        // TODO: load predictions for the match, compute points using the
        // group's scoring rule, persist via PgPredictionRepository, then
        // publish RoundScored if every match in the round is now scored.
        todo!("scorer::run process event")
    }
    Ok(())
}
