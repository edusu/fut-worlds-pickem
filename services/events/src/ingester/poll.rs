//! Single iteration of the periodic ingestion loop.
//!
//! `poll_once` is the steady-state body the long-running ingester invokes
//! every 5 minutes. It is intentionally a *one-pass* function — no internal
//! sleeps, no background tasks — so the surrounding driver can drive its
//! cadence cleanly via `tokio::time::interval` and so an operational CLI
//! can invoke a single tick on demand.
//!
//! Side effects in dependency order:
//!   1. Pull the full fixture list from the upstream provider.
//!   2. Load every team into a `country_code → uuid` map (one query) so
//!      `dto_to_match`'s pens-winner resolver runs in memory.
//!   3. For each fixture: read prior state, upsert, and emit
//!      `MatchFinished::V1` exactly once per `→ finished` transition.
//!
//! Per-match failures (one bad DTO) are logged and skipped so a single
//! corrupt row never aborts the whole tick.

use std::collections::HashMap;

use anyhow::Context;
use chrono::Utc;
use domain::repository::{MatchRepository, TeamRepository};
use domain::{Match, MatchStatus, Phase, Team, Tournament};
use messaging::{
    events::{MatchFinished, MatchFinishedV1},
    topics, Publisher,
};
use persistence::repositories::{PgMatchRepository, PgTeamRepository};
use sports_client::{Client as SportsClient, MatchDto};
use tracing::{instrument, warn};
use uuid::Uuid;

use super::bootstrap::GlobalRounds;
use super::football_data::{dto_to_match, phase_from_stage};

/// Aggregate snapshot of one ingestion tick. Returned to the driver so it
/// can adapt the polling cadence — in particular, switch to fast mode while
/// `live_count > 0` — without issuing a separate "is anything live" query
/// against Postgres. Producer-side data is always cheaper than asking again.
#[derive(Debug, Clone, Copy)]
pub struct TickReport {
    /// Number of fixtures the upstream returned in this tick.
    pub total: usize,
    /// Fixtures for which a `MatchFinished` event was emitted (i.e. transitioned
    /// into `finished` during this tick).
    pub emitted: usize,
    /// Fixtures whose row failed to upsert / publish; skipped without aborting
    /// the rest of the tick. Investigate via the per-row `warn!` log lines.
    pub skipped: usize,
    /// Fixtures whose post-upsert status is `live`. Drives the adaptive
    /// cadence in `run`.
    pub live_count: usize,
}

impl TickReport {
    /// Whether any match is currently live. Cheap accessor to keep call
    /// sites readable.
    pub fn any_live(&self) -> bool {
        self.live_count > 0
    }
}

/// Per-match outcome of `process_match`. Carried back to `poll_once` so it
/// can build a `TickReport` from in-flight knowledge instead of re-reading
/// the DB after the loop.
struct ProcessOutcome {
    /// Whether `MatchFinished::V1` was published for this fixture.
    emitted_finished: bool,
    /// Domain status of the post-upsert row. Used to count live matches.
    status_after: MatchStatus,
}

/// Run a single ingestion tick: fetch fixtures, upsert, and emit
/// `MatchFinished` for any match that just transitioned into the
/// `finished` state. Idempotent: re-running on the same upstream payload
/// is a no-op (the upsert preserves the row, no transition is detected).
///
/// Returns a `TickReport` so the driver can choose its next sleep duration
/// based on the live-match count without an extra DB round-trip.
#[instrument(
    skip(client, match_repo, team_repo, publisher, tournament, rounds),
    fields(tournament_id = %tournament.id)
)]
pub async fn poll_once(
    client: &SportsClient,
    match_repo: &PgMatchRepository,
    team_repo: &PgTeamRepository,
    publisher: &Publisher,
    tournament: &Tournament,
    rounds: &GlobalRounds,
) -> anyhow::Result<TickReport> {
    let resp = client
        .get_competition_matches(&tournament.external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| {
            format!(
                "fetching fixtures for competition {}",
                tournament.external_id
            )
        })?;

    // Single pre-fetch of teams keeps the inner loop allocation-free and
    // saves one Postgres round-trip per match. The map is empty until the
    // tournament-data ingester (#9) populates `teams`; until then the
    // resolver returns None for every code, leaving `pens_winner_team_id`
    // NULL in the upserted row — which matches the v1 reality (no match
    // finishes before #9 lands).
    let teams = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading teams for resolver map")?;
    let team_index = build_team_index(&teams);

    let mut emitted = 0usize;
    let mut skipped = 0usize;
    let mut live_count = 0usize;

    for dto in &resp.matches {
        match process_match(dto, rounds, &team_index, match_repo, publisher).await {
            Ok(outcome) => {
                if outcome.emitted_finished {
                    emitted += 1;
                }
                if matches!(outcome.status_after, MatchStatus::Live) {
                    live_count += 1;
                }
            }
            Err(err) => {
                // One bad row must not poison the whole tick. We log loud
                // (warn, not error) so the next pass has a chance to retry
                // without operator intervention.
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

/// Process a single match DTO end-to-end. Returns the post-upsert status
/// and whether a `MatchFinished` event was published for this row, so the
/// outer loop can build aggregate counts (live, emitted) without any extra
/// queries.
async fn process_match(
    dto: &MatchDto,
    rounds: &GlobalRounds,
    team_index: &HashMap<String, Uuid>,
    match_repo: &PgMatchRepository,
    publisher: &Publisher,
) -> anyhow::Result<ProcessOutcome> {
    let round_id = round_id_for_phase(dto, rounds);
    let external_id = dto.id.to_string();

    let prev = match_repo
        .find_by_external(round_id, &external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("looking up match external_id={external_id}"))?;

    // Reuse the existing UUID on conflict so any FK already pointing at
    // this match (e.g. `predictions.match_id`) stays valid. When prev is
    // None, mint a fresh UUID — the INSERT will use it; on a future
    // conflict it would be ignored by ON CONFLICT (round_id, external_id)
    // DO UPDATE.
    let match_id = prev.as_ref().map(|p| p.id).unwrap_or_else(Uuid::new_v4);

    let new_match = dto_to_match(dto, round_id, match_id, |code| {
        team_index.get(code).copied()
    })
    .with_context(|| format!("mapping match dto external_id={external_id}"))?;

    match_repo
        .upsert(&new_match)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("upserting match external_id={external_id}"))?;

    let status_after = new_match.status;

    if !is_finished_transition(prev.as_ref(), &new_match) {
        return Ok(ProcessOutcome {
            emitted_finished: false,
            status_after,
        });
    }

    let (Some(home_score), Some(away_score)) = (new_match.home_score, new_match.away_score) else {
        // Upstream said `finished` but did not ship scores. We refuse to
        // emit a misleading 0-0 event; the next tick will retry once the
        // upstream catches up.
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
        round_id: new_match.round_id,
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

/// Pick which global round a fixture belongs to based on its phase.
fn round_id_for_phase(dto: &MatchDto, rounds: &GlobalRounds) -> Uuid {
    match phase_from_stage(dto.stage.as_deref()) {
        Phase::Group => rounds.group_stage.id,
        Phase::Knockout => rounds.knockouts.id,
    }
}

/// Build the `country_code → team_id` index `dto_to_match`'s pens-winner
/// resolver consults. Codes are stored uppercased to match how
/// `team_country_code` produces them at the call site.
fn build_team_index(teams: &[Team]) -> HashMap<String, Uuid> {
    teams
        .iter()
        .map(|t| (t.country_code.to_ascii_uppercase(), t.id))
        .collect()
}

/// Detect whether `new` represents a fresh transition into `finished`,
/// given an optional `prev` snapshot read from the DB before the upsert.
///
/// Pure: no I/O, no side effects. Driven entirely by the two `MatchStatus`
/// values. This isolation is what lets the function be unit-tested without
/// a Postgres or NATS instance.
fn is_finished_transition(prev: Option<&Match>, new: &Match) -> bool {
    if new.status != MatchStatus::Finished {
        return false;
    }
    match prev {
        // First sighting of a match that is already finished. Happens when
        // the ingester boots after a match has concluded. We do want to
        // emit so downstream scoring runs.
        None => true,
        // Transition into finished from any non-finished state.
        Some(p) => p.status != MatchStatus::Finished,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Convenience: build a `Match` value with everything set to a sane
    /// default. Tests override the single field they care about (`status`).
    fn match_with_status(status: MatchStatus) -> Match {
        Match {
            id: Uuid::nil(),
            round_id: Uuid::nil(),
            external_id: "1".into(),
            home_team: "ESP".into(),
            away_team: "GER".into(),
            home_flag: String::new(),
            away_flag: String::new(),
            kickoff_at: Utc.with_ymd_and_hms(2026, 6, 12, 18, 0, 0).unwrap(),
            home_score: Some(1),
            away_score: Some(0),
            et_home_score: None,
            et_away_score: None,
            pens_winner_team_id: None,
            status,
            phase: Phase::Group,
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
    fn live_to_finished_is_a_transition() {
        let prev = match_with_status(MatchStatus::Live);
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
        // Pathological upstream regression: a match that was finished
        // somehow goes back to scheduled. Not our event — the scorer would
        // have already processed it on the prior tick.
        let prev = match_with_status(MatchStatus::Finished);
        let new = match_with_status(MatchStatus::Scheduled);
        assert!(!is_finished_transition(Some(&prev), &new));
    }

    #[test]
    fn live_to_live_is_not_a_transition() {
        let prev = match_with_status(MatchStatus::Live);
        let new = match_with_status(MatchStatus::Live);
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
        // Domain stores codes already uppercased, but the helper guards
        // against legacy / hand-inserted rows that slipped through.
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
