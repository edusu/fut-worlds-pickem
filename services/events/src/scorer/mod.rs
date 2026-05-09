use messaging::{topics, MatchFinished, Subscriber};
use shared::Config;
use sqlx::PgPool;
use tracing::info;

/// Subscribe to `pickem.match.finished` and award points for every prediction
/// on the finished match using `domain::scoring::calculate_points`.
pub async fn run(_config: Config, _pool: PgPool, nats: async_nats::Client) -> anyhow::Result<()> {
    let mut sub: Subscriber<MatchFinished> = Subscriber::subscribe(&nats, topics::MATCH_FINISHED)
        .await
        .map_err(shared::report_to_anyhow)?;

    info!(topic = topics::MATCH_FINISHED, "scorer ready");

    while let Some(_event) = sub.next().await {
        // TODO: load predictions for the match, compute points using the
        // pickem's scoring rule, persist via PgPredictionRepository, then
        // publish `SubmissionWindowScored` if every match in the parent
        // window (tournament_group or knockout_phase) is now scored.
        todo!("scorer::run process event")
    }
    Ok(())
}
