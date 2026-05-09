//! One-shot bootstrap routines that run before the periodic ingestion loop.
//!
//! These functions establish the *tournament-wide* state every other
//! ingester pass relies on: the tournament row itself, the 12 group-stage
//! `tournament_groups`, and the 6 `knockout_phases` (Round of 32 through
//! Final). They are idempotent by construction — re-running a
//! fully-bootstrapped service is a fast no-op that hits the DB at most
//! once per check.
//!
//! Group-stage and knockout-phase deadlines are derived from the upstream
//! fixture list: in v1 every group shares a single timestamp (the first
//! group-stage kickoff) and every knockout phase shares another (the
//! first knockout kickoff). The schema lets these diverge per-row without
//! migration the day product wants per-stage or per-group windows.

use std::collections::HashMap;

use anyhow::Context;
use chrono::{DateTime, Utc};
use domain::repository::{
    KnockoutPhaseRepository, TeamRepository, TournamentGroupRepository, TournamentRepository,
};
use domain::{
    KnockoutPhase, KnockoutPhaseState, KnockoutStage, Team, Tournament, TournamentGroup,
    TournamentGroupAssignment, TournamentGroupState,
};
use persistence::repositories::{
    PgKnockoutPhaseRepository, PgTeamRepository, PgTournamentGroupRepository,
    PgTournamentRepository,
};
use sports_client::{Client as SportsClient, MatchDto};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use super::football_data::{
    flag_emoji_for_team, group_assignments_from_matches, group_assignments_from_standings,
    group_label_from_upstream, knockout_stage_from_upstream, parse_kickoff, phase_from_stage,
    team_country_code,
};

pub use shared::V1_TOURNAMENT_EXTERNAL_ID;

/// Human-facing name for the v1 tournament. Stored in `tournaments.name`
/// and shown in operational tooling. Deliberately uses the Spanish form to
/// stay clear of FIFA's English-language trademarks ("World Cup",
/// "FIFA World Cup", and combinations); the upstream code in
/// `V1_TOURNAMENT_EXTERNAL_ID` is purely internal and never user-facing.
pub const V1_TOURNAMENT_NAME: &str = "Mundial 2026";

/// Make sure the v1 tournament row exists, returning the canonical
/// `Tournament` value other bootstrap steps need to anchor their writes.
///
/// Looks the row up by its `external_id` first to avoid an unnecessary
/// write on subsequent boots. Falls back to `upsert` when the row is
/// missing — using `upsert` rather than `create` means a concurrent boot
/// that wrote the row between our `find` and `upsert` does not error out;
/// the second writer simply rewrites the same `name` onto the existing row.
#[instrument(skip(repo))]
pub async fn ensure_tournament(repo: &PgTournamentRepository) -> anyhow::Result<Tournament> {
    if let Some(existing) = repo
        .find_by_external_id(V1_TOURNAMENT_EXTERNAL_ID)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| {
            format!("looking up tournament by external_id={V1_TOURNAMENT_EXTERNAL_ID}")
        })?
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
        name: V1_TOURNAMENT_NAME.to_string(),
        external_id: V1_TOURNAMENT_EXTERNAL_ID.to_string(),
        created_at: Utc::now(),
    };
    repo.upsert(&tournament)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("seeding tournament external_id={V1_TOURNAMENT_EXTERNAL_ID}"))?;

    let canonical = repo
        .find_by_external_id(V1_TOURNAMENT_EXTERNAL_ID)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| {
            format!("re-reading tournament after upsert external_id={V1_TOURNAMENT_EXTERNAL_ID}")
        })?
        .context("tournament row missing immediately after upsert")?;

    info!(
        tournament_id = %canonical.id,
        external_id = %canonical.external_id,
        "tournament seeded"
    );
    Ok(canonical)
}

/// Resolved tournament structure returned by the structure-seeding
/// bootstrap. The two index maps (`group_index`, `knockout_index`) are
/// what `poll::process_match` looks up to map an upstream match DTO to its
/// structural parent.
pub struct TournamentStructure {
    pub groups: Vec<TournamentGroup>,
    pub knockouts: Vec<KnockoutPhase>,
    /// `"Group A" → tournament_group.id`. Built once at bootstrap so the
    /// hot-path lookup is O(1).
    pub group_index: HashMap<String, Uuid>,
    /// `KnockoutStage → knockout_phase.id`.
    pub knockout_index: HashMap<KnockoutStage, Uuid>,
}

/// Make sure the 12 tournament_groups and the 6 knockout_phases exist for
/// this tournament. Idempotent: if all rows are present, returns
/// immediately without contacting the upstream provider.
///
/// When at least one row is missing, the function pulls the full fixture
/// list once, then upserts every group named in the matches feed (with
/// `deadline_at` set to the earliest group-stage kickoff) and every
/// knockout phase the upstream feed exposes (each with the same earliest
/// knockout-kickoff timestamp in v1). The schema is happy to grow into
/// per-stage or per-group deadlines later without migration.
#[instrument(skip(client, group_repo, phase_repo), fields(tournament_id = %tournament.id))]
pub async fn ensure_tournament_structure(
    client: &SportsClient,
    group_repo: &PgTournamentGroupRepository,
    phase_repo: &PgKnockoutPhaseRepository,
    tournament: &Tournament,
) -> anyhow::Result<TournamentStructure> {
    let existing_groups = group_repo
        .list_by_tournament(tournament.id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing tournament_groups")?;
    let existing_phases = phase_repo
        .list_by_tournament(tournament.id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing knockout_phases")?;

    let groups_complete = !existing_groups.is_empty();
    let phases_complete = existing_phases.len() == ALL_STAGES.len();

    if groups_complete && phases_complete {
        info!(
            groups = existing_groups.len(),
            phases = existing_phases.len(),
            "tournament structure already seeded"
        );
        return Ok(build_structure(existing_groups, existing_phases));
    }

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

    let kickoffs = first_kickoffs_per_stage(&resp.matches);
    let now = Utc::now();

    let groups = ensure_groups_seeded(group_repo, tournament, &resp.matches, &kickoffs, now)
        .await
        .context("seeding tournament_groups")?;
    let knockouts = ensure_knockouts_seeded(phase_repo, tournament, &kickoffs, now)
        .await
        .context("seeding knockout_phases")?;

    Ok(build_structure(groups, knockouts))
}

fn build_structure(
    groups: Vec<TournamentGroup>,
    knockouts: Vec<KnockoutPhase>,
) -> TournamentStructure {
    let group_index = groups.iter().map(|g| (g.name.clone(), g.id)).collect();
    let knockout_index = knockouts.iter().map(|p| (p.stage, p.id)).collect();
    TournamentStructure {
        groups,
        knockouts,
        group_index,
        knockout_index,
    }
}

/// First kickoff per stage, derived from the upstream match list. The
/// group-stage entry is the earliest kickoff across every group-stage
/// match; the knockout entries are per-`KnockoutStage` first kickoffs.
/// Matches whose kickoff fails to parse or whose stage is unrecognised are
/// silently skipped.
struct StageKickoffs {
    group_first: Option<DateTime<Utc>>,
    knockout_first: HashMap<KnockoutStage, DateTime<Utc>>,
}

fn first_kickoffs_per_stage(matches: &[MatchDto]) -> StageKickoffs {
    let mut group_first: Option<DateTime<Utc>> = None;
    let mut knockout_first: HashMap<KnockoutStage, DateTime<Utc>> = HashMap::new();

    for dto in matches {
        let Some(kickoff) = parse_kickoff(&dto.utc_date) else {
            continue;
        };
        match phase_from_stage(dto.stage.as_deref()) {
            domain::Phase::Group => {
                group_first = match group_first {
                    Some(current) if current <= kickoff => Some(current),
                    _ => Some(kickoff),
                };
            }
            domain::Phase::Knockout => {
                let Some(stage) = knockout_stage_from_upstream(dto.stage.as_deref()) else {
                    continue;
                };
                knockout_first
                    .entry(stage)
                    .and_modify(|current| {
                        if kickoff < *current {
                            *current = kickoff;
                        }
                    })
                    .or_insert(kickoff);
            }
        }
    }

    StageKickoffs {
        group_first,
        knockout_first,
    }
}

async fn ensure_groups_seeded(
    repo: &PgTournamentGroupRepository,
    tournament: &Tournament,
    matches: &[MatchDto],
    kickoffs: &StageKickoffs,
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<TournamentGroup>> {
    let group_deadline = kickoffs
        .group_first
        .context("no group-stage matches in upstream feed — cannot derive group-stage deadline")?;

    // Distinct group labels in the order they appear (matches the upstream
    // ordering Group A → Group L for the WC).
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut labels: Vec<String> = Vec::new();
    for dto in matches {
        if !matches!(phase_from_stage(dto.stage.as_deref()), domain::Phase::Group) {
            continue;
        }
        let Some(label) = group_label_from_upstream(dto.group.as_deref()) else {
            continue;
        };
        if seen.insert(label.clone(), ()).is_none() {
            labels.push(label);
        }
    }

    if labels.is_empty() {
        return Err(anyhow::anyhow!(
            "no group labels in upstream matches — cannot seed tournament_groups"
        ));
    }

    let mut canonical = Vec::with_capacity(labels.len());
    for name in &labels {
        let id = match repo
            .find_by_name(tournament.id, name)
            .await
            .map_err(shared::report_to_anyhow)?
        {
            Some(existing) => existing.id,
            None => Uuid::new_v4(),
        };
        let group = TournamentGroup {
            id,
            tournament_id: tournament.id,
            name: name.clone(),
            deadline_at: group_deadline,
            state: TournamentGroupState::Open,
            created_at: now,
        };
        repo.upsert(&group)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("upserting tournament_group {name}"))?;
        canonical.push(group);
    }

    info!(count = canonical.len(), "tournament_groups upserted");
    Ok(canonical)
}

async fn ensure_knockouts_seeded(
    repo: &PgKnockoutPhaseRepository,
    tournament: &Tournament,
    kickoffs: &StageKickoffs,
    now: DateTime<Utc>,
) -> anyhow::Result<Vec<KnockoutPhase>> {
    // v1: every knockout phase shares the same deadline = first knockout
    // kickoff in the entire bracket. The schema supports per-stage values
    // but no producer sets them yet.
    let shared_deadline = kickoffs
        .knockout_first
        .values()
        .min()
        .copied()
        .context("no knockout matches in upstream feed — cannot derive knockout deadline")?;

    let mut canonical = Vec::with_capacity(ALL_STAGES.len());
    for stage in ALL_STAGES {
        let id = match repo
            .find_by_stage(tournament.id, *stage)
            .await
            .map_err(shared::report_to_anyhow)?
        {
            Some(existing) => existing.id,
            None => Uuid::new_v4(),
        };
        // The upstream may not list every stage we expect (e.g. Third
        // Place could be missing in pre-tournament feeds). When that
        // happens we still seed the row with the shared deadline so the
        // bracket is renderable — a refresh later will tighten the value.
        if !kickoffs.knockout_first.contains_key(stage) {
            warn!(stage = ?stage, "no upstream kickoff for stage; using shared knockout deadline");
        }
        let phase = KnockoutPhase {
            id,
            tournament_id: tournament.id,
            stage: *stage,
            position: stage.position(),
            display_name: stage.display_name_es().to_string(),
            deadline_at: shared_deadline,
            state: KnockoutPhaseState::Open,
            created_at: now,
        };
        repo.upsert(&phase)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("upserting knockout_phase {:?}", stage))?;
        canonical.push(phase);
    }

    info!(count = canonical.len(), "knockout_phases upserted");
    Ok(canonical)
}

const ALL_STAGES: &[KnockoutStage] = &[
    KnockoutStage::Last32,
    KnockoutStage::Last16,
    KnockoutStage::QuarterFinals,
    KnockoutStage::SemiFinals,
    KnockoutStage::ThirdPlace,
    KnockoutStage::Final,
];

/// Make sure `teams` and `tournament_group_teams` are populated for the v1
/// tournament. Idempotent: when any team row already exists in the global
/// catalog, the function assumes a previous run (CLI or boot) finished and
/// returns the existing rows without contacting the upstream.
///
/// On first boot it fetches `/teams` + `/matches` (with `/standings` as a
/// pre-tournament fallback) from football-data.org, upserts each team, and
/// writes one `tournament_group_teams` row per `(group, country)` pair.
/// Requires `structure.group_index` to be populated — call
/// `ensure_tournament_structure` first.
///
/// Returns the canonical team list so the caller can build its resolver
/// index without paying for a second `list_all` round-trip.
#[instrument(skip(client, team_repo, group_repo, structure), fields(tournament_id = %tournament.id))]
pub async fn ensure_teams_and_assignments_seeded(
    client: &SportsClient,
    team_repo: &PgTeamRepository,
    group_repo: &PgTournamentGroupRepository,
    tournament: &Tournament,
    structure: &TournamentStructure,
) -> anyhow::Result<Vec<Team>> {
    let existing = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing teams catalog")?;

    if !existing.is_empty() {
        info!(count = existing.len(), "teams already seeded");
        return Ok(existing);
    }

    info!(competition = %tournament.external_id, "seeding teams from upstream");
    let teams_payload = client
        .get_competition_teams(&tournament.external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching teams for {}", tournament.external_id))?;

    // Local code → id index built as we upsert, so the assignment step can
    // resolve country codes without a per-team `find_by_country_code`.
    let mut country_to_id: HashMap<String, Uuid> = HashMap::new();
    for dto in &teams_payload.teams {
        let Some(code) = team_country_code(dto) else {
            warn!(name = %dto.name, "team has no country code, skipping");
            continue;
        };
        let id = Uuid::new_v4();
        let team = Team {
            id,
            name: dto.name.clone(),
            flag_emoji: flag_emoji_for_team(dto),
            country_code: code.clone(),
        };
        team_repo
            .upsert(&team)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("upserting team {code}"))?;
        country_to_id.insert(code, id);
    }
    info!(count = country_to_id.len(), "teams upserted");

    // /matches is the primary source: it carries `group` on every group-
    // stage fixture and is non-empty before the competition starts. We only
    // hit /standings as a fallback because the upstream returns it empty
    // until the tournament is live.
    let matches_payload = client
        .get_competition_matches(&tournament.external_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("fetching matches for {}", tournament.external_id))?;
    let mut assignments = group_assignments_from_matches(&matches_payload.matches);
    if assignments.is_empty() {
        info!("no group-stage matches yet; falling back to /standings");
        let standings = client
            .get_competition_standings(&tournament.external_id)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("fetching standings for {}", tournament.external_id))?;
        assignments = group_assignments_from_standings(&standings.standings);
    }

    let mut assigned = 0;
    let mut skipped_missing_team = 0;
    let mut skipped_missing_group = 0;
    for (group_name, country_code) in &assignments {
        let Some(team_id) = country_to_id.get(country_code) else {
            warn!(
                country_code,
                group_name, "team not in /teams payload, skipping"
            );
            skipped_missing_team += 1;
            continue;
        };
        let Some(group_id) = structure.group_index.get(group_name) else {
            warn!(group_name, "group not in seeded structure, skipping");
            skipped_missing_group += 1;
            continue;
        };
        group_repo
            .assign_team(&TournamentGroupAssignment {
                tournament_id: tournament.id,
                tournament_group_id: *group_id,
                team_id: *team_id,
            })
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("assigning {country_code} to {group_name}"))?;
        assigned += 1;
    }

    info!(
        assignments = assigned,
        missing_team = skipped_missing_team,
        missing_group = skipped_missing_group,
        "team-to-group assignments written"
    );

    // Re-read so the returned set reflects what is actually in Postgres
    // (some upstream rows may have been skipped for lack of a country
    // code, and a parallel writer could have appended in the meantime).
    team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("re-reading teams after seed")
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
    fn picks_earliest_kickoff_per_stage() {
        let matches = vec![
            fixture("2026-06-12T18:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-06-11T16:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-06-13T20:00:00Z", Some("GROUP_STAGE")),
            fixture("2026-07-01T18:00:00Z", Some("LAST_32")),
            fixture("2026-06-30T20:00:00Z", Some("LAST_32")),
            fixture("2026-07-15T20:00:00Z", Some("FINAL")),
        ];

        let resolved = first_kickoffs_per_stage(&matches);

        assert_eq!(
            resolved.group_first.unwrap().to_rfc3339(),
            "2026-06-11T16:00:00+00:00"
        );
        assert_eq!(
            resolved.knockout_first[&KnockoutStage::Last32].to_rfc3339(),
            "2026-06-30T20:00:00+00:00"
        );
        assert_eq!(
            resolved.knockout_first[&KnockoutStage::Final].to_rfc3339(),
            "2026-07-15T20:00:00+00:00"
        );
    }

    #[test]
    fn skips_unparseable_kickoffs() {
        let matches = vec![
            fixture("not a date", Some("GROUP_STAGE")),
            fixture("2026-06-11T16:00:00Z", Some("GROUP_STAGE")),
        ];
        let resolved = first_kickoffs_per_stage(&matches);
        assert_eq!(
            resolved.group_first.unwrap().to_rfc3339(),
            "2026-06-11T16:00:00+00:00"
        );
    }

    #[test]
    fn no_stage_is_treated_as_group() {
        let matches = vec![fixture("2026-06-11T16:00:00Z", None)];
        let resolved = first_kickoffs_per_stage(&matches);
        assert!(resolved.group_first.is_some());
        assert!(resolved.knockout_first.is_empty());
    }
}
