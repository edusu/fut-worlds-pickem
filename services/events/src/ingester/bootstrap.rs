//! One-shot bootstrap routines that run before the periodic ingestion loop.
//!
//! These functions establish the *tournament-wide* state every other
//! ingester pass relies on: the tournament row itself, the global rounds,
//! and (later) the team / tournament-group seed. They are idempotent by
//! construction — re-running a fully-bootstrapped service is a fast no-op
//! that hits the DB at most once per check.

use anyhow::Context;
use chrono::{DateTime, Utc};
use domain::repository::{RoundRepository, TournamentRepository};
use domain::{Phase, Round, RoundState, Tournament};
use persistence::repositories::{PgRoundRepository, PgTournamentRepository};
use sports_client::{Client as SportsClient, MatchDto};
use tracing::{info, instrument};
use uuid::Uuid;

use super::football_data::{parse_kickoff, phase_from_stage};

/// Upstream competition code for the v1 tournament. football-data.org uses
/// `"WC"` for the FIFA World Cup; the CLI mirrors the same default in its
/// `--competition` argument. Hardcoded here because v1 only ever targets
/// the World Cup; promoting this to config is a low-cost follow-up the day
/// a second tournament shows up.
pub const V1_TOURNAMENT_EXTERNAL_ID: &str = "WC";

/// Human-facing name for the v1 tournament. Stored in `tournaments.name`
/// and shown in operational tooling. Deliberately uses the Spanish form to
/// stay clear of FIFA's English-language trademarks ("World Cup",
/// "FIFA World Cup", and combinations); the upstream code in
/// `V1_TOURNAMENT_EXTERNAL_ID` is purely internal and never user-facing.
pub const V1_TOURNAMENT_NAME: &str = "Mundial 2026";

/// Display name of the group-stage round. Spanish form, see
/// `V1_TOURNAMENT_NAME`.
pub const ROUND_GROUP_STAGE_NAME: &str = "Fase de grupos 2026";

/// Display name of the knockout round. Spanish form, see
/// `V1_TOURNAMENT_NAME`.
pub const ROUND_KNOCKOUTS_NAME: &str = "Eliminatorias 2026";

/// Make sure a tournament row with the given `external_id` exists and
/// return the canonical `Tournament`. Idempotent: a fast-path read avoids
/// the upsert + re-read on subsequent boots.
#[instrument(skip(repo))]
pub async fn ensure_tournament(
    repo: &PgTournamentRepository,
    external_id: &str,
    name: &str,
) -> anyhow::Result<Tournament> {
    if let Some(existing) = repo
        .find_by_external_id(external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("looking up tournament by external_id={external_id}"))?
    {
        info!(
            tournament_id = %existing.id,
            external_id = %existing.external_id,
            "tournament already seeded"
        );
        return Ok(existing);
    }

    let tournament = Tournament {
        id: Uuid::new_v4(),
        name: name.to_string(),
        external_id: external_id.to_string(),
        created_at: Utc::now(),
    };
    repo.upsert(&tournament)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("seeding tournament external_id={external_id}"))?;

    // Re-read after upsert: ON CONFLICT preserves the original PK, so this
    // is the only way to surface the canonical UUID when a concurrent
    // writer raced us.
    let canonical = repo
        .find_by_external_id(external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("re-reading tournament after upsert external_id={external_id}"))?
        .context("tournament row missing immediately after upsert")?;

    info!(
        tournament_id = %canonical.id,
        external_id = %canonical.external_id,
        "tournament seeded"
    );
    Ok(canonical)
}

/// Resolved global rounds returned by `ensure_global_rounds`. Both rounds
/// always exist in the returned struct — either pre-existing or freshly
/// created — so downstream callers (the periodic poll, the scorer's
/// round-id resolver) can use either field unconditionally.
pub struct GlobalRounds {
    pub group_stage: Round,
    pub knockouts: Round,
}

/// Make sure the two global rounds (`group_stage`, `knockouts`) exist for
/// this tournament. Idempotent: if both already exist, returns immediately
/// without contacting the upstream provider.
///
/// When at least one round is missing, the function pulls the full fixture
/// list once, classifies each match by `phase_from_stage`, and uses
/// `MIN(kickoff)` per phase as the round deadline. Predictions for every
/// match in that phase remain editable up to that single deadline — for the
/// 2026 World Cup that means the entire group stage shares one deadline
/// (first group-stage kickoff) and the entire knockout bracket shares
/// another (first knockout kickoff, i.e. the Round of 32 opener).
#[instrument(skip(client, round_repo), fields(tournament_id = %tournament.id))]
pub async fn ensure_global_rounds(
    client: &SportsClient,
    round_repo: &PgRoundRepository,
    tournament: &Tournament,
) -> anyhow::Result<GlobalRounds> {
    let (existing_group, existing_knockouts) = tokio::try_join!(
        async {
            round_repo
                .find_by_name(tournament.id, ROUND_GROUP_STAGE_NAME)
                .await
                .map_err(shared::report_to_anyhow)
                .context("looking up group-stage round")
        },
        async {
            round_repo
                .find_by_name(tournament.id, ROUND_KNOCKOUTS_NAME)
                .await
                .map_err(shared::report_to_anyhow)
                .context("looking up knockouts round")
        }
    )?;

    if let (Some(group_stage), Some(knockouts)) = (&existing_group, &existing_knockouts) {
        info!(
            group_round_id = %group_stage.id,
            knockout_round_id = %knockouts.id,
            "global rounds already seeded"
        );
        return Ok(GlobalRounds {
            group_stage: group_stage.clone(),
            knockouts: knockouts.clone(),
        });
    }

    // Always fetch when at least one round is missing: a previous boot may
    // have crashed mid-create, leaving a half-state we cannot reconstruct
    // without an upstream call.
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

    let (group_first, knockout_first) = first_kickoffs_per_phase(&resp.matches);
    let now = Utc::now();

    let group_stage = match existing_group {
        Some(round) => round,
        None => {
            let deadline = group_first.context(
                "no group-stage matches in upstream feed — cannot derive group-stage deadline",
            )?;
            create_round(
                round_repo,
                tournament.id,
                ROUND_GROUP_STAGE_NAME,
                deadline,
                Phase::Group,
                now,
            )
            .await?
        }
    };

    let knockouts = match existing_knockouts {
        Some(round) => round,
        None => {
            let deadline = knockout_first.context(
                "no knockout matches in upstream feed — cannot derive knockouts deadline",
            )?;
            create_round(
                round_repo,
                tournament.id,
                ROUND_KNOCKOUTS_NAME,
                deadline,
                Phase::Knockout,
                now,
            )
            .await?
        }
    };

    Ok(GlobalRounds {
        group_stage,
        knockouts,
    })
}

async fn create_round(
    repo: &PgRoundRepository,
    tournament_id: Uuid,
    name: &str,
    deadline_at: DateTime<Utc>,
    phase: Phase,
    created_at: DateTime<Utc>,
) -> anyhow::Result<Round> {
    let round = Round {
        id: Uuid::new_v4(),
        tournament_id,
        name: name.to_string(),
        deadline_at,
        state: RoundState::Open,
        phase,
        created_at,
    };
    repo.create(&round)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("creating round name={name}"))?;
    info!(round_id = %round.id, name = %round.name, deadline = %round.deadline_at, "round created");
    Ok(round)
}

/// Walk a fixture list once and return the earliest kickoff for the group
/// phase and for the knockout phase. Either component is `None` only when
/// no match of that phase appears in the feed — pure helper, no I/O.
///
/// Kickoffs that fail to parse are silently skipped (the upstream very
/// occasionally ships placeholder rows during bracket draws); the caller
/// surfaces the "no usable matches" case as a hard error.
fn first_kickoffs_per_phase(
    matches: &[MatchDto],
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let mut group_first: Option<DateTime<Utc>> = None;
    let mut knockout_first: Option<DateTime<Utc>> = None;
    for dto in matches {
        let Some(kickoff) = parse_kickoff(&dto.utc_date) else {
            continue;
        };
        let slot = match phase_from_stage(dto.stage.as_deref()) {
            Phase::Group => &mut group_first,
            Phase::Knockout => &mut knockout_first,
        };
        match slot {
            Some(current) if *current <= kickoff => {}
            _ => *slot = Some(kickoff),
        }
    }
    (group_first, knockout_first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sports_client::{MatchStatusDto, TeamRefDto};

    fn fixture(utc_date: &str, stage: Option<&str>) -> MatchDto {
        MatchDto {
            id: 0,
            utc_date: utc_date.to_string(),
            status: MatchStatusDto::Scheduled,
            stage: stage.map(String::from),
            group: None,
            last_updated: None,
            home_team: TeamRefDto {
                id: None,
                name: None,
                short_name: None,
                tla: None,
                crest: None,
            },
            away_team: TeamRefDto {
                id: None,
                name: None,
                short_name: None,
                tla: None,
                crest: None,
            },
            score: None,
        }
    }

    #[test]
    fn picks_earliest_kickoff_per_phase() {
        // Group stage: three matches, the middle one is the earliest.
        // Knockout: two matches with `LAST_32` (WC 2026's Round of 32).
        let matches = vec![
            fixture("2026-06-12T18:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-06-11T16:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-06-13T20:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-07-01T18:00:00Z", Some("LAST_32")),
            fixture("2026-06-30T20:00:00Z", Some("LAST_32")),
        ];

        let (group, knockout) = first_kickoffs_per_phase(&matches);

        assert_eq!(group.unwrap().to_rfc3339(), "2026-06-11T16:00:00+00:00");
        assert_eq!(knockout.unwrap().to_rfc3339(), "2026-06-30T20:00:00+00:00");
    }

    #[test]
    fn returns_none_for_missing_phase() {
        // Only group-stage matches in the feed — knockout slot stays None.
        let matches = vec![fixture("2026-06-11T16:00:00Z", Some("GROUP_STAGE"))];
        let (group, knockout) = first_kickoffs_per_phase(&matches);
        assert!(group.is_some());
        assert!(knockout.is_none());
    }

    #[test]
    fn skips_unparseable_kickoffs() {
        // Garbage `utc_date` rows must not poison the result. The valid
        // group-stage row should still be picked.
        let matches = vec![
            fixture("not a date", Some("GROUP_STAGE")),
            fixture("2026-06-11T16:00:00Z", Some("GROUP_STAGE")),
        ];
        let (group, _) = first_kickoffs_per_phase(&matches);
        assert_eq!(group.unwrap().to_rfc3339(), "2026-06-11T16:00:00+00:00");
    }

    #[test]
    fn no_stage_is_treated_as_group() {
        // Mirrors `phase_from_stage`'s default: stage=None falls back to
        // Group phase, so its kickoff lands in the group_first slot.
        let matches = vec![fixture("2026-06-11T16:00:00Z", None)];
        let (group, knockout) = first_kickoffs_per_phase(&matches);
        assert!(group.is_some());
        assert!(knockout.is_none());
    }
}
