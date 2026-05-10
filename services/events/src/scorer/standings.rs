//! Pure helpers + (later) dispatchers for the end-of-phase scoring
//! flow. The pure helpers translate upstream data (`/standings`,
//! ingested R16 matches) into the inputs that
//! `domain::scoring::{score_group_standings, score_best_thirds}` expect.
//!
//! The dispatchers (`dispatch_group_standings`, `dispatch_best_thirds`)
//! land in subsequent steps of #5 and live in this same module — keep
//! the file path stable so callers don't churn.

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Context};
use chrono::Utc;
use domain::repository::{
    BestThirdsPredictionRepository, GroupRepository, KnockoutPhaseRepository, MatchRepository,
    ScoringAuditRepository, ScoringRuleRepository, StandingsPredictionRepository, TeamRepository,
    TournamentGroupRepository, TournamentRepository,
};
use domain::scoring::{score_best_thirds, score_group_standings};
use domain::{
    AuditKind, KnockoutStage, Match, MatchStatus, ScoringAudit, ScoringRule, TournamentGroupState,
};
use persistence::repositories::{
    PgBestThirdsPredictionRepository, PgGroupRepository, PgKnockoutPhaseRepository,
    PgMatchRepository, PgScoringAuditRepository, PgScoringRuleRepository,
    PgStandingsPredictionRepository, PgTeamRepository, PgTournamentGroupRepository,
    PgTournamentRepository,
};
use sports_client::dto::CompetitionStandings;
use sports_client::SportsClientLike;
use sqlx::PgPool;
use tracing::{debug, info};
use uuid::Uuid;

/// Namespace UUID for synthesising best-thirds audit `prediction_id`
/// values. Best-thirds rows have no native scalar id (the prediction
/// is one row per team in `best_thirds_predictions`), so we hash
/// `(pickem_group_id, user_id)` into a stable v5 UUID for audit
/// purposes. Constant chosen as a random v4 UUID and frozen.
const BEST_THIRDS_AUDIT_NAMESPACE: Uuid =
    Uuid::from_u128(0x6f6c_5a64_7c92_4e7f_b1ba_a6c2_6f0c_7f88);

/// Upstream competition code for the v1 tournament. Aliases
/// `shared::V1_TOURNAMENT_EXTERNAL_ID` so the indirection is explicit:
/// the `/standings` endpoint is keyed by this code.
const COMPETITION_CODE: &str = shared::V1_TOURNAMENT_EXTERNAL_ID;

/// Resolve `[1st, 2nd, 3rd, 4th]` team UUIDs of a tournament group from
/// the `/standings` payload. The football-data.org API applies the
/// official competition tiebreakers (FIFA full ladder for the World
/// Cup) and surfaces the result as the `position` field — the helper
/// just trusts it.
///
/// Returns `None` whenever the data is not yet usable for scoring:
///
/// - The standings payload has no entry for `group_code`.
/// - Any of positions 1..4 is missing or out of range.
/// - A team's `tla` is missing in the payload (we have no key to map it
///   against `team_index`).
/// - A `tla` cannot be resolved against `team_index` (means the team
///   was not ingested yet).
///
/// Each `None` outcome is a transient gate, not an error: the caller
/// (the reconciliator) re-tries on the next tick.
pub fn compute_group_ordering(
    standings: &CompetitionStandings,
    group_code: &str,
    team_index: &HashMap<String, Uuid>,
) -> Option<[Uuid; 4]> {
    let table = standings
        .standings
        .iter()
        .find(|s| {
            s.stage.as_deref() == Some("GROUP_STAGE") && s.group.as_deref() == Some(group_code)
        })?
        .table
        .as_slice();

    let mut slots: [Option<Uuid>; 4] = [None; 4];
    for row in table {
        let position = row.position?;
        if !(1..=4).contains(&position) {
            continue;
        }
        let tla = row.team.tla.as_ref()?;
        let id = team_index.get(&tla.to_ascii_uppercase()).copied()?;
        slots[(position - 1) as usize] = Some(id);
    }

    Some([slots[0]?, slots[1]?, slots[2]?, slots[3]?])
}

/// Best-thirds = the third-placed teams that **actually advance** to
/// the round of 16. We read that fact from the published bracket
/// instead of computing FIFA's last-resort tiebreakers (fair play,
/// drawing of lots — unrepresentable in our schema).
///
/// `r16_matches` are the 16 fixtures of the round-of-16 phase, fully
/// ingested or partially populated; `third_place_team_ids` is the set
/// of 12 teams that finished 3rd in their groups.
///
/// Implementation: collect every non-null `home_team_id` /
/// `away_team_id` from the R16 fixtures, intersect with the
/// third-place set, return the intersection.
///
/// When the bracket is not fully populated yet (any team_id is
/// `None`), the returned set will have fewer than 8 elements. The
/// caller checks `result.len() == 8` to decide whether to score now or
/// defer to the next reconcile tick.
pub fn compute_best_thirds_from_r16(
    r16_matches: &[Match],
    third_place_team_ids: &HashSet<Uuid>,
) -> HashSet<Uuid> {
    let mut bracket_teams: HashSet<Uuid> = HashSet::with_capacity(32);
    for m in r16_matches {
        if let Some(id) = m.home_team_id {
            bracket_teams.insert(id);
        }
        if let Some(id) = m.away_team_id {
            bracket_teams.insert(id);
        }
    }
    bracket_teams
        .intersection(third_place_team_ids)
        .copied()
        .collect()
}

/// Translate the human-readable `tournament_group.name` ("Group A") into
/// the upstream code used by `/standings` ("GROUP_A"). The DB has no
/// dedicated column for the upstream form because the bidirectional
/// mapping is mechanical: uppercase + space-to-underscore.
fn group_name_to_upstream_code(name: &str) -> String {
    name.to_ascii_uppercase().replace(' ', "_")
}

/// Score every standings prediction targeting `tournament_group_id`,
/// transition the group to `scored`, then check whether the 12 group
/// stage tournament_groups are all scored — if so, kick off best-thirds.
///
/// Idempotent: the gate `state == Open` short-circuits when the group
/// has already been scored.
pub async fn dispatch_group_standings(
    pool: &PgPool,
    sports_client: &dyn SportsClientLike,
    tournament_group_id: Uuid,
) -> anyhow::Result<()> {
    let match_repo = PgMatchRepository::new(pool.clone());
    let group_repo = PgTournamentGroupRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());
    let standings_repo = PgStandingsPredictionRepository::new(pool.clone());
    let pickem_repo = PgGroupRepository::new(pool.clone());
    let rule_repo = PgScoringRuleRepository::new(pool.clone());
    let audit_repo = PgScoringAuditRepository::new(pool.clone());

    // 1. Load the tournament_group and gate on state. Re-running for a
    //    `scored` group is a clean no-op.
    let tg = group_repo
        .find(tournament_group_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading tournament_group")?
        .ok_or_else(|| anyhow!("tournament_group {} not found", tournament_group_id))?;
    if tg.state != TournamentGroupState::Open {
        debug!(
            tournament_group_id = %tournament_group_id,
            state = ?tg.state,
            "skip group standings: not open",
        );
        return Ok(());
    }

    // 2. Verify all matches in this group are finished.
    let group_matches = match_repo
        .list_by_tournament_group(tournament_group_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing matches for group")?;
    if group_matches.is_empty()
        || group_matches
            .iter()
            .any(|m| m.status != MatchStatus::Finished)
    {
        debug!(
            tournament_group_id = %tournament_group_id,
            "skip group standings: not all matches finished yet",
        );
        return Ok(());
    }

    // 3. Build team_index from teams catalog (small, ~50 rows).
    let teams = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing teams")?;
    let team_index: HashMap<String, Uuid> = teams
        .iter()
        .map(|t| (t.country_code.to_ascii_uppercase(), t.id))
        .collect();

    // 4. Pull /standings, compute the [1st, 2nd, 3rd, 4th] ordering for
    //    this group. If the API hasn't published `position` yet, defer.
    let standings_payload = sports_client
        .get_competition_standings(COMPETITION_CODE)
        .await
        .map_err(shared::report_to_anyhow)
        .context("fetching /standings")?;
    let upstream_code = group_name_to_upstream_code(&tg.name);
    let actual = match compute_group_ordering(&standings_payload, &upstream_code, &team_index) {
        Some(a) => a,
        None => {
            debug!(
                tournament_group_id = %tournament_group_id,
                upstream_code = %upstream_code,
                "skip group standings: /standings not yet usable",
            );
            return Ok(());
        }
    };

    // 5. Score every standings prediction across pickems for this group.
    let pickems = pickem_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing pickems")?;
    let mut rule_cache: HashMap<Uuid, ScoringRule> = HashMap::new();

    for pickem in &pickems {
        let predictions = standings_repo
            .list_by_pickem(pickem.id)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("listing standings predictions for pickem {}", pickem.id))?;
        let for_this_group = predictions
            .into_iter()
            .filter(|p| p.tournament_group_id == tournament_group_id);

        for prediction in for_this_group {
            let rule = lookup_rule(&rule_repo, &mut rule_cache, pickem.id).await?;
            let points = score_group_standings(&prediction, &actual, &rule);

            standings_repo
                .record_points(prediction.id, points)
                .await
                .map_err(shared::report_to_anyhow)
                .with_context(|| {
                    format!(
                        "persisting points for standings prediction {}",
                        prediction.id
                    )
                })?;

            audit_repo
                .record(&ScoringAudit {
                    id: Uuid::new_v4(),
                    scored_at: Utc::now(),
                    audit_kind: AuditKind::GroupStandings,
                    prediction_id: prediction.id,
                    pickem_group_id: pickem.id,
                    user_id: prediction.user_id,
                    points_awarded: points,
                    scoring_rule_id: rule.id,
                    was_changed: false,
                })
                .await
                .map_err(shared::report_to_anyhow)
                .with_context(|| {
                    format!(
                        "appending audit row for standings prediction {}",
                        prediction.id
                    )
                })?;
        }
    }

    // 6. Last step: transition the group to `scored`. If we crashed
    //    earlier, the reconciliator restarts cleanly.
    group_repo
        .set_state(tournament_group_id, TournamentGroupState::Scored)
        .await
        .map_err(shared::report_to_anyhow)
        .context("transitioning group to scored")?;

    info!(
        tournament_group_id = %tournament_group_id,
        upstream_code = %upstream_code,
        pickems = pickems.len(),
        "group standings scored",
    );

    // 7. Cascade: if this transition leaves all 12 groups in `scored`,
    //    fire best-thirds. The function gates internally on R16 being
    //    populated.
    if all_group_stage_groups_scored(&group_repo, tg.tournament_id).await? {
        if let Err(err) = dispatch_best_thirds(pool, sports_client).await {
            tracing::warn!(
                tournament_id = %tg.tournament_id,
                error = ?err,
                "best-thirds scoring failed; reconciler will retry",
            );
        }
    }

    Ok(())
}

/// Score every best-thirds prediction across pickems, gated on
/// (i) the 12 tournament_groups being in `scored`, and (ii) the 16 R16
/// fixtures having both team UUIDs populated. Re-running is safe: the
/// per-pickem gate (presence of any row in `best_thirds_scoring`)
/// short-circuits already-scored pickems.
pub async fn dispatch_best_thirds(
    pool: &PgPool,
    sports_client: &dyn SportsClientLike,
) -> anyhow::Result<()> {
    let group_repo = PgTournamentGroupRepository::new(pool.clone());
    let phase_repo = PgKnockoutPhaseRepository::new(pool.clone());
    let match_repo = PgMatchRepository::new(pool.clone());
    let team_repo = PgTeamRepository::new(pool.clone());
    let pickem_repo = PgGroupRepository::new(pool.clone());
    let rule_repo = PgScoringRuleRepository::new(pool.clone());
    let best_thirds_repo = PgBestThirdsPredictionRepository::new(pool.clone());
    let audit_repo = PgScoringAuditRepository::new(pool.clone());

    // Resolve the v1 tournament by its external code. v1 only ever
    // has a single tournament so the lookup is unambiguous; if the row
    // is missing the migrations / bootstrap haven't run, nothing to
    // score yet.
    let tournament_repo = PgTournamentRepository::new(pool.clone());
    let tournament_id = match tournament_repo
        .find_by_external_id(shared::V1_TOURNAMENT_EXTERNAL_ID)
        .await
        .map_err(shared::report_to_anyhow)
        .context("resolving v1 tournament id")?
    {
        Some(t) => t.id,
        None => {
            debug!("skip best-thirds: v1 tournament not seeded yet");
            return Ok(());
        }
    };

    // 1. Confirm all 12 groups in `scored`.
    if !all_group_stage_groups_scored(&group_repo, tournament_id).await? {
        debug!("skip best-thirds: not all tournament_groups are scored");
        return Ok(());
    }

    // 2. Load R16 phase + matches; gate on full team population.
    let r16 = phase_repo
        .find_by_stage(tournament_id, KnockoutStage::Last16)
        .await
        .map_err(shared::report_to_anyhow)
        .context("loading R16 phase")?
        .ok_or_else(|| anyhow!("R16 phase missing for tournament {tournament_id}"))?;
    let r16_matches = match_repo
        .list_by_knockout_phase(r16.id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing R16 matches")?;
    if r16_matches.len() < 16
        || r16_matches
            .iter()
            .any(|m| m.home_team_id.is_none() || m.away_team_id.is_none())
    {
        debug!(
            r16_count = r16_matches.len(),
            "skip best-thirds: R16 bracket not fully populated yet",
        );
        return Ok(());
    }

    // 3. Pull /standings, collect the 12 third-place teams. Build a
    //    team_index to map TLAs → uuids.
    let standings_payload = sports_client
        .get_competition_standings(COMPETITION_CODE)
        .await
        .map_err(shared::report_to_anyhow)
        .context("fetching /standings for best-thirds")?;
    let teams = team_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing teams")?;
    let team_index: HashMap<String, Uuid> = teams
        .iter()
        .map(|t| (t.country_code.to_ascii_uppercase(), t.id))
        .collect();

    let third_place_team_ids = collect_third_place_teams(&standings_payload, &team_index);
    if third_place_team_ids.len() != 12 {
        debug!(
            count = third_place_team_ids.len(),
            "skip best-thirds: third-place set not yet 12 teams",
        );
        return Ok(());
    }

    // 4. Intersect with the R16 bracket; expect exactly 8 teams.
    let actual = compute_best_thirds_from_r16(&r16_matches, &third_place_team_ids);
    if actual.len() != 8 {
        debug!(
            count = actual.len(),
            "skip best-thirds: bracket intersection != 8",
        );
        return Ok(());
    }

    // 5. Score each pickem that hasn't yet been scored.
    let pickems = pickem_repo
        .list_all()
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing pickems")?;
    let mut rule_cache: HashMap<Uuid, ScoringRule> = HashMap::new();
    let mut scored_pickems = 0usize;

    for pickem in &pickems {
        if best_thirds_already_scored(pool, pickem.id).await? {
            continue;
        }
        let members = pickem_repo
            .list_members(pickem.id)
            .await
            .map_err(shared::report_to_anyhow)
            .with_context(|| format!("listing members of pickem {}", pickem.id))?;

        let rule = lookup_rule(&rule_repo, &mut rule_cache, pickem.id).await?;

        for member in &members {
            let prediction = match best_thirds_repo
                .list_by_user(pickem.id, member.user_id)
                .await
            {
                Ok(p) => p,
                Err(report) => match report.current_context() {
                    domain::RepositoryError::NotFound => continue,
                    _ => return Err(shared::report_to_anyhow(report)),
                },
            };
            let points = score_best_thirds(&prediction, &actual, &rule);

            best_thirds_repo
                .record_score(pickem.id, member.user_id, points)
                .await
                .map_err(shared::report_to_anyhow)
                .with_context(|| {
                    format!(
                        "persisting best-thirds score for pickem={} user={}",
                        pickem.id, member.user_id.0
                    )
                })?;

            audit_repo
                .record(&ScoringAudit {
                    id: Uuid::new_v4(),
                    scored_at: Utc::now(),
                    audit_kind: AuditKind::BestThirds,
                    prediction_id: synthetic_best_thirds_prediction_id(pickem.id, member.user_id.0),
                    pickem_group_id: pickem.id,
                    user_id: member.user_id,
                    points_awarded: points,
                    scoring_rule_id: rule.id,
                    was_changed: false,
                })
                .await
                .map_err(shared::report_to_anyhow)
                .with_context(|| {
                    format!(
                        "appending audit row for best-thirds (pickem={} user={})",
                        pickem.id, member.user_id.0
                    )
                })?;
            // Bind the unused `prediction` so `_` doesn't lint, and to
            // make it explicit that we're loading it for completeness
            // (the future API may want to re-derive the team set from
            // it).
            let _ = &prediction;
        }

        scored_pickems += 1;
    }

    if scored_pickems > 0 {
        info!(
            scored_pickems,
            r16_phase_id = %r16.id,
            "best-thirds scored",
        );
    }

    Ok(())
}

// ---------- helpers ----------

async fn lookup_rule(
    rule_repo: &PgScoringRuleRepository,
    cache: &mut HashMap<Uuid, ScoringRule>,
    pickem_id: Uuid,
) -> anyhow::Result<ScoringRule> {
    if let Some(r) = cache.get(&pickem_id) {
        return Ok(r.clone());
    }
    let r = rule_repo
        .find_by_pickem_group(pickem_id)
        .await
        .map_err(shared::report_to_anyhow)
        .with_context(|| format!("loading scoring rule for pickem {pickem_id}"))?
        .ok_or_else(|| {
            anyhow!(
                "no scoring rule attached to pickem {pickem_id} \
                (groups.scoring_rule_id missing or stale)"
            )
        })?;
    cache.insert(pickem_id, r.clone());
    Ok(r)
}

async fn all_group_stage_groups_scored(
    group_repo: &PgTournamentGroupRepository,
    tournament_id: Uuid,
) -> anyhow::Result<bool> {
    let groups = group_repo
        .list_by_tournament(tournament_id)
        .await
        .map_err(shared::report_to_anyhow)
        .context("listing tournament_groups for end-of-stage check")?;
    Ok(!groups.is_empty()
        && groups
            .iter()
            .all(|g| g.state == TournamentGroupState::Scored))
}

fn collect_third_place_teams(
    standings: &CompetitionStandings,
    team_index: &HashMap<String, Uuid>,
) -> HashSet<Uuid> {
    let mut out = HashSet::with_capacity(12);
    for entry in &standings.standings {
        if entry.stage.as_deref() != Some("GROUP_STAGE") {
            continue;
        }
        for row in &entry.table {
            if row.position == Some(3) {
                if let Some(tla) = row.team.tla.as_ref() {
                    if let Some(id) = team_index.get(&tla.to_ascii_uppercase()) {
                        out.insert(*id);
                    }
                }
            }
        }
    }
    out
}

/// Build a stable v5 UUID from `(pickem_group_id, user_id)` to use as
/// `scoring_audit.prediction_id` for best-thirds rows. v5 is purely a
/// deterministic hash; it has no semantics beyond "same input → same
/// output", which is what the audit timeline needs.
fn synthetic_best_thirds_prediction_id(pickem_group_id: Uuid, user_id: i64) -> Uuid {
    let key = format!("{pickem_group_id}/{user_id}");
    Uuid::new_v5(&BEST_THIRDS_AUDIT_NAMESPACE, key.as_bytes())
}

async fn best_thirds_already_scored(pool: &PgPool, pickem_group_id: Uuid) -> anyhow::Result<bool> {
    let row = sqlx::query!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM best_thirds_scoring
            WHERE pickem_group_id = $1
        ) AS "exists!"
        "#,
        pickem_group_id,
    )
    .fetch_one(pool)
    .await
    .context("checking best_thirds_scoring presence")?;
    Ok(row.exists)
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::TimeZone;
    use domain::{Match, MatchStatus};
    use sports_client::dto::{StandingDto, StandingsRowDto, TeamRefDto};
    use uuid::Uuid;

    fn team_ref(tla: &str) -> TeamRefDto {
        TeamRefDto {
            id: Some(0),
            name: Some(tla.to_string()),
            short_name: None,
            tla: Some(tla.to_string()),
            crest: None,
        }
    }

    fn standings_for_group(group_code: &str, rows: Vec<(i32, &str)>) -> CompetitionStandings {
        CompetitionStandings {
            standings: vec![StandingDto {
                stage: Some("GROUP_STAGE".to_string()),
                type_: Some("TOTAL".to_string()),
                group: Some(group_code.to_string()),
                table: rows
                    .into_iter()
                    .map(|(pos, tla)| StandingsRowDto {
                        position: Some(pos),
                        team: team_ref(tla),
                        played_games: Some(3),
                        won: None,
                        draw: None,
                        lost: None,
                        points: None,
                        goals_for: None,
                        goals_against: None,
                        goal_difference: None,
                    })
                    .collect(),
            }],
        }
    }

    fn make_team_index(pairs: &[(&str, Uuid)]) -> HashMap<String, Uuid> {
        pairs
            .iter()
            .map(|(tla, id)| (tla.to_ascii_uppercase(), *id))
            .collect()
    }

    fn empty_match(home: Option<Uuid>, away: Option<Uuid>) -> Match {
        Match {
            id: Uuid::new_v4(),
            external_id: "x".to_string(),
            tournament_group_id: None,
            knockout_phase_id: Some(Uuid::new_v4()),
            home_team_id: home,
            away_team_id: away,
            kickoff_at: chrono::Utc.with_ymd_and_hms(2026, 6, 15, 18, 0, 0).unwrap(),
            home_score: None,
            away_score: None,
            et_home_score: None,
            et_away_score: None,
            pens_winner_team_id: None,
            status: MatchStatus::Scheduled,
        }
    }

    #[test]
    fn group_ordering_happy_path() {
        let mex = Uuid::new_v4();
        let usa = Uuid::new_v4();
        let can = Uuid::new_v4();
        let bel = Uuid::new_v4();
        let s = standings_for_group(
            "GROUP_A",
            vec![(1, "MEX"), (2, "USA"), (3, "CAN"), (4, "BEL")],
        );
        let idx = make_team_index(&[("MEX", mex), ("USA", usa), ("CAN", can), ("BEL", bel)]);

        let ordering = compute_group_ordering(&s, "GROUP_A", &idx).unwrap();
        assert_eq!(ordering, [mex, usa, can, bel]);
    }

    #[test]
    fn group_ordering_missing_position_returns_none() {
        let mex = Uuid::new_v4();
        let usa = Uuid::new_v4();
        let mut s = standings_for_group("GROUP_A", vec![(1, "MEX"), (2, "USA")]);
        // Two more rows but their position is not yet published.
        s.standings[0].table.push(StandingsRowDto {
            position: None,
            team: team_ref("CAN"),
            played_games: Some(3),
            won: None,
            draw: None,
            lost: None,
            points: None,
            goals_for: None,
            goals_against: None,
            goal_difference: None,
        });
        s.standings[0].table.push(StandingsRowDto {
            position: None,
            team: team_ref("BEL"),
            played_games: Some(3),
            won: None,
            draw: None,
            lost: None,
            points: None,
            goals_for: None,
            goals_against: None,
            goal_difference: None,
        });
        let idx = make_team_index(&[("MEX", mex), ("USA", usa)]);

        assert!(compute_group_ordering(&s, "GROUP_A", &idx).is_none());
    }

    #[test]
    fn group_ordering_unknown_tla_returns_none() {
        let mex = Uuid::new_v4();
        let s = standings_for_group(
            "GROUP_A",
            vec![(1, "MEX"), (2, "USA"), (3, "CAN"), (4, "BEL")],
        );
        // Only MEX is in the index — USA/CAN/BEL won't resolve.
        let idx = make_team_index(&[("MEX", mex)]);

        assert!(compute_group_ordering(&s, "GROUP_A", &idx).is_none());
    }

    #[test]
    fn group_ordering_other_group_returns_none() {
        let mex = Uuid::new_v4();
        let s = standings_for_group(
            "GROUP_A",
            vec![(1, "MEX"), (2, "USA"), (3, "CAN"), (4, "BEL")],
        );
        let idx = make_team_index(&[("MEX", mex)]);

        assert!(compute_group_ordering(&s, "GROUP_B", &idx).is_none());
    }

    #[test]
    fn group_ordering_empty_payload_returns_none() {
        let s = CompetitionStandings { standings: vec![] };
        let idx = make_team_index(&[]);
        assert!(compute_group_ordering(&s, "GROUP_A", &idx).is_none());
    }

    #[test]
    fn group_ordering_lowercase_tla_resolves() {
        let mex = Uuid::new_v4();
        let usa = Uuid::new_v4();
        let can = Uuid::new_v4();
        let bel = Uuid::new_v4();
        let s = standings_for_group(
            "GROUP_A",
            vec![(1, "mex"), (2, "Usa"), (3, "CaN"), (4, "BEL")],
        );
        let idx = make_team_index(&[("MEX", mex), ("USA", usa), ("CAN", can), ("BEL", bel)]);

        let ordering = compute_group_ordering(&s, "GROUP_A", &idx).unwrap();
        assert_eq!(ordering, [mex, usa, can, bel]);
    }

    #[test]
    fn best_thirds_full_bracket_returns_eight() {
        // Build a third-place set of 12 distinct teams, then craft 16 R16
        // matches that include 8 of them.
        let thirds: Vec<Uuid> = (0..12).map(|_| Uuid::new_v4()).collect();
        let other_teams: Vec<Uuid> = (0..24).map(|_| Uuid::new_v4()).collect();

        let advancing = &thirds[0..8];
        let mut matches = Vec::with_capacity(16);
        for i in 0..8 {
            matches.push(empty_match(Some(other_teams[i * 2]), Some(advancing[i])));
        }
        for i in 0..8 {
            matches.push(empty_match(
                Some(other_teams[16 + i % 8]),
                Some(other_teams[i + 8]),
            ));
        }

        let third_set: HashSet<Uuid> = thirds.iter().copied().collect();
        let result = compute_best_thirds_from_r16(&matches, &third_set);
        assert_eq!(result.len(), 8);
        for t in advancing {
            assert!(result.contains(t));
        }
    }

    #[test]
    fn best_thirds_partial_bracket_returns_partial_set() {
        let thirds: Vec<Uuid> = (0..12).map(|_| Uuid::new_v4()).collect();
        let advancing = &thirds[0..3];
        let other = (0..6).map(|_| Uuid::new_v4()).collect::<Vec<_>>();

        let mut matches = Vec::new();
        // 3 R16 matches with one third-place advancing each.
        matches.push(empty_match(Some(other[0]), Some(advancing[0])));
        matches.push(empty_match(Some(advancing[1]), Some(other[1])));
        matches.push(empty_match(Some(advancing[2]), Some(other[2])));
        // 13 R16 matches with `None` teams: bracket not yet published.
        for _ in 0..13 {
            matches.push(empty_match(None, None));
        }

        let third_set: HashSet<Uuid> = thirds.iter().copied().collect();
        let result = compute_best_thirds_from_r16(&matches, &third_set);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn best_thirds_empty_third_set_returns_empty() {
        let teams: Vec<Uuid> = (0..32).map(|_| Uuid::new_v4()).collect();
        let mut matches = Vec::new();
        for i in 0..16 {
            matches.push(empty_match(Some(teams[i * 2]), Some(teams[i * 2 + 1])));
        }
        let third_set: HashSet<Uuid> = HashSet::new();

        let result = compute_best_thirds_from_r16(&matches, &third_set);
        assert!(result.is_empty());
    }
}
