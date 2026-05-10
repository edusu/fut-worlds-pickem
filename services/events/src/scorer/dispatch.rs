//! Match-scoring dispatcher.
//!
//! Entry point: [`dispatch_match_scoring`]. Called by the NATS handler
//! (`scorer::run`) on every `MatchFinished` event and by the
//! reconciliator (`scorer::reconcile`) for matches it finds finished
//! but unscored.
//!
//! Idempotency: every persistence step is overwrite-safe.
//! `record_points` is `UPDATE points_awarded = $2` so re-runs converge
//! on the same value. `scoring_audit` does append a fresh row per run
//! by design — it's the audit log, not a cache.

use std::collections::HashMap;

use anyhow::{anyhow, Context};
use chrono::Utc;
use domain::repository::{
    MatchRepository, PredictionRepository, ScoringAuditRepository, ScoringRuleRepository,
    TournamentGroupRepository,
};
use domain::scoring::{score_group_match, score_knockout_match};
use domain::{
    AuditKind, Match, MatchStatus, Phase, Prediction, ScoringAudit, ScoringRule,
    TournamentGroupState,
};
use persistence::repositories::{
    PgMatchRepository, PgPredictionRepository, PgScoringAuditRepository, PgScoringRuleRepository,
    PgTournamentGroupRepository,
};
use sports_client::SportsClientLike;
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::standings::dispatch_group_standings;

/// Score every prediction targeting `match_id`, persist `points_awarded`
/// and append an audit row. If the match was the last of its
/// `tournament_group`, kick off `dispatch_group_standings` for that
/// group.
pub async fn dispatch_match_scoring(
    pool: &PgPool,
    sports_client: &dyn SportsClientLike,
    match_id: Uuid,
) -> anyhow::Result<()> {
    let match_repo = PgMatchRepository::new(pool.clone());
    let prediction_repo = PgPredictionRepository::new(pool.clone());
    let rule_repo = PgScoringRuleRepository::new(pool.clone());
    let audit_repo = PgScoringAuditRepository::new(pool.clone());
    let group_repo = PgTournamentGroupRepository::new(pool.clone());

    // 1. Load the match. A missing row or non-finished status is a
    //    degenerate case (upstream race or buggy caller); skip cleanly.
    let m = match match_repo
        .find(match_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading match")?
    {
        Some(m) if m.status == MatchStatus::Finished => m,
        Some(other) => {
            debug!(
                match_id = %match_id,
                status = ?other.status,
                "skip scoring: match is not finished",
            );
            return Ok(());
        }
        None => {
            debug!(match_id = %match_id, "skip scoring: match not found");
            return Ok(());
        }
    };

    // 2. Load every prediction targeting this match across all pickems.
    let predictions = prediction_repo
        .list_by_match(match_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing predictions for match")?;

    if predictions.is_empty() {
        debug!(match_id = %match_id, "no predictions to score");
    } else {
        score_predictions(&prediction_repo, &rule_repo, &audit_repo, &m, &predictions).await?;
        info!(
            match_id = %match_id,
            scored = predictions.len(),
            "match predictions scored"
        );
    }

    // 3. End-of-group hook: if this was a group-stage match, check
    //    whether the whole group is now finished. If yes, score
    //    standings.
    if let Some(tournament_group_id) = m.tournament_group_id {
        if all_group_matches_finished(&match_repo, tournament_group_id).await? {
            // Only proceed if the group is still `open` — `scored`
            // means another path (the reconciliator) already
            // closed it.
            let tg = group_repo
                .find(tournament_group_id)
                .await
                .map_err(shared::report_to_anyhow)
                .context("loading tournament group for end-of-group check")?;
            if matches!(tg, Some(g) if g.state == TournamentGroupState::Open) {
                if let Err(err) =
                    dispatch_group_standings(pool, sports_client, tournament_group_id).await
                {
                    warn!(
                        tournament_group_id = %tournament_group_id,
                        error = ?err,
                        "end-of-group scoring failed; reconciler will retry",
                    );
                }
            }
        }
    }

    Ok(())
}

/// Score the given prediction set against the match. Builds a per-pickem
/// rule cache so multi-pickem matches issue one rule lookup per pickem,
/// not one per prediction.
async fn score_predictions(
    prediction_repo: &PgPredictionRepository,
    rule_repo: &PgScoringRuleRepository,
    audit_repo: &PgScoringAuditRepository,
    m: &Match,
    predictions: &[Prediction],
) -> anyhow::Result<()> {
    let mut rule_cache: HashMap<Uuid, ScoringRule> = HashMap::new();

    for prediction in predictions {
        let rule = match rule_cache.get(&prediction.pickem_group_id) {
            Some(r) => r.clone(),
            None => {
                let r = rule_repo
                    .find_by_pickem_group(prediction.pickem_group_id)
                    .await
                    .map_err(shared::report_to_anyhow)
                    .context("loading scoring rule for pickem")?
                    .ok_or_else(|| {
                        anyhow!(
                            "no scoring rule attached to pickem {} (groups.scoring_rule_id missing or stale)",
                            prediction.pickem_group_id
                        )
                    })?;
                rule_cache.insert(prediction.pickem_group_id, r.clone());
                r
            }
        };

        let points = match m.phase() {
            Phase::Group => score_group_match(prediction, m, &rule)
                .with_context(|| format!("scoring group match prediction {}", prediction.id))?,
            Phase::Knockout => {
                let home = m.home_team_id.ok_or_else(|| {
                    anyhow!(
                        "knockout match {} has NULL home_team_id at scoring time",
                        m.id
                    )
                })?;
                let away = m.away_team_id.ok_or_else(|| {
                    anyhow!(
                        "knockout match {} has NULL away_team_id at scoring time",
                        m.id
                    )
                })?;
                score_knockout_match(prediction, m, home, away, &rule).with_context(|| {
                    format!("scoring knockout match prediction {}", prediction.id)
                })?
            }
        };

        prediction_repo
            .record_points(prediction.id, points)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("persisting points for prediction {}", prediction.id))?;

        audit_repo
            .record(&ScoringAudit {
                id: Uuid::new_v4(),
                scored_at: Utc::now(),
                audit_kind: AuditKind::Match,
                prediction_id: prediction.id,
                pickem_group_id: prediction.pickem_group_id,
                user_id: prediction.user_id,
                points_awarded: points,
                scoring_rule_id: rule.id,
                was_changed: prediction.was_changed,
            })
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("appending audit row for prediction {}", prediction.id))?;
    }

    Ok(())
}

/// Whether all matches inside a tournament group are in `MatchStatus::Finished`.
pub(super) async fn all_group_matches_finished(
    match_repo: &PgMatchRepository,
    tournament_group_id: Uuid,
) -> anyhow::Result<bool> {
    let matches = match_repo
        .list_by_tournament_group(tournament_group_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing matches for tournament group")?;

    Ok(!matches.is_empty() && matches.iter().all(|m| m.status == MatchStatus::Finished))
}
