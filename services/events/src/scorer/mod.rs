pub mod dispatch;
pub mod reconcile;
pub mod standings;

use anyhow::Context;
use messaging::{topics, MatchFinished, Subscriber};
use rust_utils::network::nats::NatsClient;
use shared::Config;
use sports_client::Client as SportsClient;
use sqlx::PgPool;
use tracing::{info, warn};

/// Run the scorer: bootstrap reconcile pass, then start (a) the
/// background reconcile loop and (b) the NATS handler. Both loops
/// converge on the same idempotent dispatch functions so a crash or
/// missed event is self-healing on the next reconcile tick.
pub async fn run(config: Config, pool: PgPool, nats: NatsClient) -> anyhow::Result<()> {
    let sports_client = SportsClient::new(config.football_api_key.clone())
        .map_err(shared::report_to_anyhow)
        .context("constructing sports-data client for scorer")?;

    // Bootstrap reconcile: catches matches finished during downtime
    // and any pending standings / best-thirds from a prior crash.
    if let Err(err) = reconcile::reconcile(&pool, &sports_client).await {
        warn!(?err, "bootstrap reconcile failed; continuing");
    }

    // Background reconcile loop. Detached: it ends when the process
    // exits. The sports `Client` is not `Clone`, so we build a second
    // one for the loop — it shares the same retry/rate-limit
    // configuration and only adds one extra `reqwest` connection
    // pool, which is negligible at our cadence (10 req/min ceiling).
    let reconcile_interval = config.scorer_reconcile_interval();
    let r_pool = pool.clone();
    let r_sports = SportsClient::new(config.football_api_key.clone())
        .map_err(shared::report_to_anyhow)
        .context("constructing sports-data client for reconcile loop")?;
    tokio::spawn(async move {
        reconcile::run_reconcile_loop(r_pool, r_sports, reconcile_interval).await;
    });

    // NATS subscriber loop: process `MatchFinished` events as the
    // ingester emits them. Errors are logged but never propagate —
    // the reconciler is the safety net.
    let mut sub: Subscriber<MatchFinished> = Subscriber::subscribe(&nats, topics::MATCH_FINISHED)
        .await
        .map_err(shared::report_to_anyhow)?;

    info!(
        topic = topics::MATCH_FINISHED,
        reconcile_interval_secs = reconcile_interval.as_secs(),
        "scorer ready",
    );

    while let Some(event) = sub.next().await {
        let MatchFinished::V1(payload) = event;
        if let Err(err) =
            dispatch::dispatch_match_scoring(&pool, &sports_client, payload.match_id).await
        {
            warn!(
                match_id = %payload.match_id,
                ?err,
                "match scoring failed; reconciler will retry",
            );
        }
    }
    Ok(())
}
