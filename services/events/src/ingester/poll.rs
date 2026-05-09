//! Single iteration of the periodic ingestion loop. `poll_once` is one
//! one-pass tick (no internal sleeps), so the driver in `mod.rs` can pace
//! cadence cleanly and an operational CLI can invoke a tick on demand.
//!
//! Per-match failures are logged and skipped so a single corrupt DTO never
//! aborts the whole tick.

use std::collections::HashMap;

use anyhow::Context;
use chrono::Utc;
use domain::repository::MatchRepository;
use domain::{KnockoutStage, Match, MatchStatus, ParentRef, Phase, Team, Tournament};
use messaging::{
    events::{MatchFinished, MatchFinishedV1},
    topics, Publisher,
};
use persistence::repositories::PgMatchRepository;
use sports_client::{Client as SportsClient, MatchDto};
use tracing::{instrument, warn};
use uuid::Uuid;

use super::football_data::{
    dto_to_match, group_label_from_upstream, knockout_stage_from_upstream, phase_from_stage,
};

/// All the bootstrap-resolved state the hot ingestion path needs to map a
/// raw `MatchDto` into a domain `Match`. Built once after bootstrap and
/// passed into every `poll_once` call.
pub struct IngestContext {
    pub tournament: Tournament,
    /// `"Group A" → tournament_groups.id`.
    pub group_index: HashMap<String, Uuid>,
    /// `KnockoutStage → knockout_phases.id`.
    pub knockout_index: HashMap<KnockoutStage, Uuid>,
    /// `country_code → teams.id` (uppercased keys).
    pub team_index: HashMap<String, Uuid>,
}

/// Aggregate snapshot of one ingestion tick. Returned to the driver so it
/// can adapt the polling cadence (fast while `live_count > 0`, slow
/// otherwise) without issuing a separate "is anything live" query.
#[derive(Debug, Clone, Copy)]
pub struct TickReport {
    pub total: usize,
    pub emitted: usize,
    pub skipped: usize,
    pub live_count: usize,
}

impl TickReport {
    pub fn any_live(&self) -> bool {
        self.live_count > 0
    }
}

struct ProcessOutcome {
    emitted_finished: bool,
    status_after: MatchStatus,
}

/// Run a single ingestion tick: fetch fixtures, upsert, and emit
/// `MatchFinished::V1` exactly once per `→ finished` transition.
/// Idempotent: re-running against the same upstream payload yields no
/// transitions and no events.
#[instrument(
    skip(client, match_repo, publisher, ctx),
    fields(tournament_id = %ctx.tournament.id)
)]
pub async fn poll_once(
    client: &SportsClient,
    match_repo: &PgMatchRepository,
    publisher: &Publisher,
    ctx: &IngestContext,
) -> anyhow::Result<TickReport> {
    let resp = client
        .get_competition_matches(&ctx.tournament.external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| {
            format!(
                "fetching fixtures for competition {}",
                ctx.tournament.external_id
            )
        })?;

    let mut emitted = 0usize;
    let mut skipped = 0usize;
    let mut live_count = 0usize;

    for dto in &resp.matches {
        match process_match(dto, ctx, match_repo, publisher).await {
            Ok(outcome) => {
                if outcome.emitted_finished {
                    emitted += 1;
                }
                if matches!(
                    outcome.status_after,
                    MatchStatus::InPlay | MatchStatus::Paused
                ) {
                    live_count += 1;
                }
            }
            Err(err) => {
                warn!(
                    upstream_id = dto.id,
                    error = ?err,
                    "skipping match in this tick"
                );
                skipped += 1;
            }
        }
    }

    let report = TickReport {
        total: resp.matches.len(),
        emitted,
        skipped,
        live_count,
    };
    tracing::info!(
        total = report.total,
        emitted = report.emitted,
        skipped = report.skipped,
        live_count = report.live_count,
        "ingestion tick finished"
    );
    Ok(report)
}

/// Process a single match DTO end-to-end: read prior state, upsert, and
/// publish a `MatchFinished` if the upsert was a `→ finished` transition.
async fn process_match(
    dto: &MatchDto,
    ctx: &IngestContext,
    match_repo: &PgMatchRepository,
    publisher: &Publisher,
) -> anyhow::Result<ProcessOutcome> {
    let parent = parent_for_dto(dto, ctx).with_context(|| {
        format!(
            "no structural parent resolved for match upstream_id={}",
            dto.id
        )
    })?;
    let external_id = dto.id.to_string();

    let prev = match_repo
        .find_by_external(&external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("looking up match external_id={external_id}"))?;

    // Reuse the existing UUID on conflict so any FK already pointing at
    // this match (e.g. `predictions.match_id`) stays valid.
    let match_id = prev.as_ref().map(|p| p.id).unwrap_or_else(Uuid::new_v4);

    let new_match = dto_to_match(dto, parent, match_id, |code| {
        ctx.team_index.get(code).copied()
    })
    .with_context(|| format!("mapping match dto external_id={external_id}"))?;

    // Skip the upsert when nothing changed since the last tick. Hot-path
    // saving — at 60s ticks * ~104 matches the unconditional UPDATE writes
    // tens of thousands of identical rows over the tournament window.
    let status_after = new_match.status;
    if prev.as_ref() == Some(&new_match) {
        return Ok(ProcessOutcome {
            emitted_finished: false,
            status_after,
        });
    }

    match_repo
        .upsert(&new_match)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("upserting match external_id={external_id}"))?;

    if !is_finished_transition(prev.as_ref(), &new_match) {
        return Ok(ProcessOutcome {
            emitted_finished: false,
            status_after,
        });
    }

    // Upstream occasionally reports `finished` before scores land. Defer
    // emission rather than publish a misleading 0-0; the next tick retries.
    let (Some(home_score), Some(away_score)) = (new_match.home_score, new_match.away_score) else {
        warn!(
            external_id = %new_match.external_id,
            "finished status without scores; deferring MatchFinished"
        );
        return Ok(ProcessOutcome {
            emitted_finished: false,
            status_after,
        });
    };

    let event = MatchFinished::V1(MatchFinishedV1 {
        match_id: new_match.id,
        external_id: new_match.external_id.clone(),
        home_score,
        away_score,
        finished_at: Utc::now(),
    });
    publisher
        .publish(topics::MATCH_FINISHED, &event)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| {
            format!(
                "publishing MatchFinished match_id={} external_id={}",
                new_match.id, new_match.external_id
            )
        })?;
    Ok(ProcessOutcome {
        emitted_finished: true,
        status_after,
    })
}

/// Resolve the structural parent (`tournament_group_id` xor
/// `knockout_phase_id`) for a match DTO using the bootstrap-built indexes.
/// Returns `None` when the upstream stage / group cannot be mapped — the
/// caller treats that as a soft skip with a warn.
fn parent_for_dto(dto: &MatchDto, ctx: &IngestContext) -> Option<ParentRef> {
    match phase_from_stage(dto.stage.as_deref()) {
        Phase::Group => {
            let label = group_label_from_upstream(dto.group.as_deref())?;
            ctx.group_index
                .get(&label)
                .copied()
                .map(|id| ParentRef::TournamentGroup { id })
        }
        Phase::Knockout => {
            let stage = knockout_stage_from_upstream(dto.stage.as_deref())?;
            ctx.knockout_index
                .get(&stage)
                .copied()
                .map(|id| ParentRef::KnockoutPhase { id })
        }
    }
}

/// Build the `country_code → team_id` index. Codes are uppercased to match
/// the convention `team_country_code` uses at the call site.
pub fn build_team_index(teams: &[Team]) -> HashMap<String, Uuid> {
    teams
        .iter()
        .map(|t| (t.country_code.to_ascii_uppercase(), t.id))
        .collect()
}

/// Detect whether `new` represents a fresh transition into `finished`.
/// The `None` arm covers the boot-after-final case (we still want to emit
/// so downstream scoring runs).
fn is_finished_transition(prev: Option<&Match>, new: &Match) -> bool {
    if new.status != MatchStatus::Finished {
        return false;
    }
    match prev {
        None => true,
        Some(p) => p.status != MatchStatus::Finished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn match_with_status(status: MatchStatus) -> Match {
        Match {
            id: Uuid::nil(),
            external_id: "1".into(),
            tournament_group_id: Some(Uuid::nil()),
            knockout_phase_id: None,
            home_team_id: None,
            away_team_id: None,
            kickoff_at: Utc.with_ymd_and_hms(2026, 6, 12, 18, 0, 0).unwrap(),
            home_score: Some(1),
            away_score: Some(0),
            et_home_score: None,
            et_away_score: None,
            pens_winner_team_id: None,
            status,
        }
    }

    #[test]
    fn first_sighting_finished_is_a_transition() {
        let new = match_with_status(MatchStatus::Finished);
        assert!(is_finished_transition(None, &new));
    }

    #[test]
    fn first_sighting_scheduled_is_not_a_transition() {
        let new = match_with_status(MatchStatus::Scheduled);
        assert!(!is_finished_transition(None, &new));
    }

    #[test]
    fn scheduled_to_finished_is_a_transition() {
        let prev = match_with_status(MatchStatus::Scheduled);
        let new = match_with_status(MatchStatus::Finished);
        assert!(is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn in_play_to_finished_is_a_transition() {
        let prev = match_with_status(MatchStatus::InPlay);
        let new = match_with_status(MatchStatus::Finished);
        assert!(is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn finished_to_finished_is_not_a_transition() {
        let prev = match_with_status(MatchStatus::Finished);
        let new = match_with_status(MatchStatus::Finished);
        assert!(!is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn finished_to_scheduled_is_not_a_transition() {
        let prev = match_with_status(MatchStatus::Finished);
        let new = match_with_status(MatchStatus::Scheduled);
        assert!(!is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn in_play_to_in_play_is_not_a_transition() {
        let prev = match_with_status(MatchStatus::InPlay);
        let new = match_with_status(MatchStatus::InPlay);
        assert!(!is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn tick_report_any_live_reflects_live_count() {
        let none_live = TickReport {
            total: 104,
            emitted: 0,
            skipped: 0,
            live_count: 0,
        };
        let some_live = TickReport {
            total: 104,
            emitted: 0,
            skipped: 0,
            live_count: 2,
        };
        assert!(!none_live.any_live());
        assert!(some_live.any_live());
    }

    #[test]
    fn build_team_index_uppercases_codes() {
        let teams = vec![
            Team {
                id: Uuid::from_u128(1),
                name: "Spain".into(),
                flag_emoji: String::new(),
                country_code: "esp".into(),
            },
            Team {
                id: Uuid::from_u128(2),
                name: "Germany".into(),
                flag_emoji: String::new(),
                country_code: "GER".into(),
            },
        ];
        let idx = build_team_index(&teams);
        assert_eq!(idx.get("ESP"), Some(&Uuid::from_u128(1)));
        assert_eq!(idx.get("GER"), Some(&Uuid::from_u128(2)));
        assert_eq!(idx.len(), 2);
    }
}
