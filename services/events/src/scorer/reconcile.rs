//! Reconciliator: catch-all scoring loop that finds work the NATS
//! handler may have missed (crash, lost message, R16 bracket
//! published mid-runtime) and re-runs the same idempotent dispatch
//! functions.
//!
//! Three responsibilities, executed in order on every tick:
//!
//! 1. **Matches finished but unscored** — `predictions.points_awarded
//!    IS NULL` while `matches.status = 'finished'`. Calls
//!    [`dispatch_match_scoring`] per match.
//! 2. **Tournament groups all-finished but `state = 'open'`** — every
//!    match in the group is finished, but the group hasn't been
//!    transitioned. Calls [`dispatch_group_standings`] per group.
//! 3. **Best-thirds pending** — only when the 12 group-stage
//!    `tournament_groups` are all in `scored`. Calls
//!    [`dispatch_best_thirds`] which gates internally on R16 being
//!    populated.
//!
//! The loop is invoked once at scorer startup (catches up after a
//! crash or first deploy) and then on a `tokio::time::interval` for
//! the long tail (R16 brackets published hours after group stage
//! ends; transient sports-API failures).

use std::time::Duration;

use anyhow::Context;
use sports_client::SportsClientLike;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::dispatch::dispatch_match_scoring;
use super::standings::{dispatch_best_thirds, dispatch_group_standings};

/// One pass of all three reconcile responsibilities. Errors from the
/// per-item dispatchers are logged and swallowed: a single bad match
/// must not stall the whole tick.
pub async fn reconcile(pool: &PgPool, sports_client: &dyn SportsClientLike) -> anyhow::Result<()> {
    let unscored = unscored_finished_matches(pool).await?;
    if !unscored.is_empty() {
        info!(
            count = unscored.len(),
            "reconcile: scoring unscored matches"
        );
    }
    for match_id in unscored {
        if let Err(err) = dispatch_match_scoring(pool, sports_client, match_id).await {
            warn!(
                match_id = %match_id,
                error = ?err,
                "reconcile: match scoring failed; will retry next tick",
            );
        }
    }

    let pending_groups = groups_pending_standings(pool).await?;
    if !pending_groups.is_empty() {
        info!(
            count = pending_groups.len(),
            "reconcile: scoring pending group standings"
        );
    }
    for group_id in pending_groups {
        if let Err(err) = dispatch_group_standings(pool, sports_client, group_id).await {
            warn!(
                tournament_group_id = %group_id,
                error = ?err,
                "reconcile: group standings failed; will retry next tick",
            );
        }
    }

    // Best-thirds pass. The dispatcher itself gates on (a) all 12
    // groups in `scored` and (b) R16 fully populated, so we can call
    // it unconditionally and let it short-circuit on either gate.
    if let Err(err) = dispatch_best_thirds(pool, sports_client).await {
        warn!(
            error = ?err,
            "reconcile: best-thirds dispatch failed; will retry next tick",
        );
    }

    Ok(())
}

/// Run [`reconcile`] forever on a `tokio::time::interval`. Per-tick
/// errors are logged but do not stop the loop. Generic over the
/// concrete sports-client type (the production `Client`, a fake in
/// tests) — the trait already pins `Send + Sync`.
pub async fn run_reconcile_loop<S>(pool: PgPool, sports_client: S, interval: Duration)
where
    S: SportsClientLike + 'static,
{
    let mut tick = tokio::time::interval(interval);
    // First `tick` resolves immediately on `tokio::time::interval`;
    // skip it so the bootstrap call (`scorer::run`) and the first
    // periodic call don't fire back-to-back.
    tick.tick().await;
    loop {
        tick.tick().await;
        debug!("reconcile tick start");
        if let Err(err) = reconcile(&pool, &sports_client).await {
            warn!(error = ?err, "reconcile tick failed");
        }
    }
}

/// Match ids whose row has `status = 'finished'` AND at least one
/// prediction has not been scored (`points_awarded IS NULL`). Joining
/// against `predictions` is intentional: a finished match that no
/// pickem predicted is not work for the scorer.
async fn unscored_finished_matches(pool: &PgPool) -> anyhow::Result<Vec<Uuid>> {
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT m.id AS "id!"
        FROM matches m
        JOIN predictions p ON p.match_id = m.id
        WHERE m.status = 'finished'
          AND p.points_awarded IS NULL
        "#,
    )
    .fetch_all(pool)
    .await
    .context("listing unscored finished matches")?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

/// Tournament group ids where every match is `finished` but the group
/// itself is still `open` (i.e. the standings dispatcher has not yet
/// run, or it ran but crashed before transitioning the group).
async fn groups_pending_standings(pool: &PgPool) -> anyhow::Result<Vec<Uuid>> {
    let rows = sqlx::query!(
        r#"
        SELECT tg.id AS "id!"
        FROM tournament_groups tg
        WHERE tg.state = 'open'
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE m.tournament_group_id = tg.id
                AND m.status <> 'finished'
          )
          AND EXISTS (
              SELECT 1 FROM matches m
              WHERE m.tournament_group_id = tg.id
          )
        "#,
    )
    .fetch_all(pool)
    .await
    .context("listing tournament_groups pending standings")?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}
