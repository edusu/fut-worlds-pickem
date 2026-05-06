//! Adapter that converts upstream football-data.org payloads into our
//! domain types. Lives here (not in `crates/sports-client`) because the
//! mapping is a service-local concern: domain semantics like "this stage is
//! a knockout" or "this team belongs to Group A" do not belong inside the
//! generic HTTP client.
//!
//! All conversions are pure (no I/O, no async) so they unit-test without
//! infrastructure and so the CLI can reuse them without spinning up a
//! database connection.

use chrono::{DateTime, Utc};
use domain::{Match, MatchStatus, Phase, Team};
use shared::flag_emoji;
use sports_client::{
    AreaDto, MatchDto, MatchStatusDto, ScoreDto, ScoreDuration, StandingDto, TeamDto, WinnerDto,
};
use uuid::Uuid;

/// Stage strings the upstream uses for the World Cup knockout bracket.
///
/// Anything else (`GROUP_STAGE`, `LEAGUE`, `LEAGUE_STAGE`, ...) is treated as
/// the group phase. Listed verbatim from the football-data.org docs.
const KNOCKOUT_STAGES: &[&str] = &[
    // The 48-team World Cup 2026 introduces a Round of 32 between the group
    // stage and the Round of 16 — the upstream labels it `LAST_32`.
    "LAST_32",
    "LAST_16",
    "ROUND_OF_32",
    "ROUND_OF_16",
    "QUARTER_FINALS",
    "SEMI_FINALS",
    "THIRD_PLACE",
    "FINAL",
    "PRELIMINARY_ROUND",
    "PLAY_OFFS",
    "PLAYOFFS",
    "PLAYOFF_ROUND_1",
    "PLAYOFF_ROUND_2",
    "PLAYOFF_ROUND_3",
];

/// Map an upstream `stage` string onto the domain `Phase` enum.
///
/// `None` (no stage in the payload) defaults to `Phase::Group` because the
/// matches endpoint sometimes ships fixtures without a stage during the
/// pre-tournament window — treating them as group-stage matches is the safer
/// default (we never accidentally emit "knockout" semantics for an unknown
/// match).
pub fn phase_from_stage(stage: Option<&str>) -> Phase {
    let Some(stage) = stage else {
        return Phase::Group;
    };
    let upper = stage.to_ascii_uppercase();
    if KNOCKOUT_STAGES.iter().any(|s| *s == upper) {
        Phase::Knockout
    } else {
        Phase::Group
    }
}

/// Map an upstream `group` string ("GROUP_A", "GROUP_B", ...) to the human
/// label we store in `tournament_groups.name`. Returns `None` when no group
/// is associated with the row (knockout matches, league competitions).
pub fn group_label_from_upstream(group: Option<&str>) -> Option<String> {
    let raw = group?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Upstream uses `GROUP_A` / `Group A` / `A`. Normalise to `Group A` so
    // the same value round-trips between teams and matches consistently.
    let token = trimmed
        .strip_prefix("GROUP_")
        .or_else(|| trimmed.strip_prefix("group_"))
        .or_else(|| trimmed.strip_prefix("Group "))
        .or_else(|| trimmed.strip_prefix("group "))
        .unwrap_or(trimmed);
    Some(format!("Group {}", token.trim().to_ascii_uppercase()))
}

/// Map the upstream `status` enum onto our domain `MatchStatus`.
///
/// `Timed` collapses into `Scheduled` (it is the upstream's "kickoff time
/// confirmed but not yet started" sub-state). `InPlay` and `Paused` collapse
/// into `Live`. `Suspended` and `Awarded` are bucketed into `Postponed` —
/// these are rare administrative states the bot does not need to distinguish.
pub fn match_status_from_dto(status: MatchStatusDto) -> MatchStatus {
    match status {
        MatchStatusDto::Scheduled | MatchStatusDto::Timed => MatchStatus::Scheduled,
        MatchStatusDto::InPlay | MatchStatusDto::Paused => MatchStatus::Live,
        MatchStatusDto::Finished => MatchStatus::Finished,
        MatchStatusDto::Postponed | MatchStatusDto::Suspended | MatchStatusDto::Awarded => {
            MatchStatus::Postponed
        }
        MatchStatusDto::Cancelled => MatchStatus::Cancelled,
        // Forward-compatible: anything we have not seen before is bucketed
        // as scheduled so the scorer never misfires on it.
        MatchStatusDto::Unknown => MatchStatus::Scheduled,
    }
}

/// Resolved view of a DTO team — what the adapter cares about after
/// looking the country code up against the local `teams` table.
pub struct ResolvedTeam {
    /// `tla` from the team DTO. Used as the human-readable label in CLI
    /// output and as the fallback when the team is not yet in the local DB.
    pub label: String,
    /// 3-letter country code, when available.
    pub country_code: Option<String>,
    /// UUID assigned to the team in our DB. `None` when the team has not been
    /// seeded yet — callers must decide whether that's an error (for
    /// `pens_winner_team_id` resolution) or a soft-fail (for printing).
    pub local_id: Option<Uuid>,
}

/// Build a `Team` row from a standalone team DTO returned by
/// `/competitions/{code}/teams`.
///
/// `country_code` is taken from `area.code` because the upstream's `tla`
/// is club-specific (e.g. "MUN" for Manchester United) and not necessarily
/// the country code for national teams. We fall back to `tla` when `area`
/// is absent so the function never panics on partial documents.
pub fn dto_to_team(dto: &TeamDto, fallback_id: Uuid) -> Option<Team> {
    let country_code = team_country_code(dto)?;
    Some(Team {
        id: fallback_id,
        name: dto.name.clone(),
        flag_emoji: flag_emoji(&country_code),
        country_code,
    })
}

/// Extract the country-code we use as natural key for a team.
///
/// We prefer `tla` (FIFA 3-letter) over `area.code` because the matches
/// endpoint references teams by `tla` only — using anything else here would
/// break the join when later mapping match.home_team back to a `teams.id`.
///
/// `area.code` is still useful as a flag-emoji input (it is more often a
/// real ISO 3166-1 alpha-3), so callers who only need the flag fall through
/// to `flag_emoji_for_team` which prefers `area.code` over `tla`.
///
/// Returns `None` only when both are absent / blank, in which case the team
/// cannot be persisted (we have no natural key for upsert idempotency).
pub fn team_country_code(dto: &TeamDto) -> Option<String> {
    if let Some(tla) = dto.tla.as_ref() {
        let trimmed = tla.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_ascii_uppercase());
        }
    }
    dto.area.as_ref().and_then(code_from_area)
}

/// Pick the best flag-emoji input for a team.
///
/// The `area.code` (when present) is more likely to be a real ISO code than
/// `tla` is — `tla` collides with club abbreviations on club competitions —
/// so we try `area.code` first and fall back to `tla`.
pub fn flag_emoji_for_team(dto: &TeamDto) -> String {
    if let Some(area) = dto.area.as_ref() {
        if let Some(code) = code_from_area(area) {
            let emoji = flag_emoji(&code);
            if !emoji.is_empty() {
                return emoji;
            }
        }
    }
    dto.tla
        .as_deref()
        .map(flag_emoji)
        .unwrap_or_else(|| flag_emoji(""))
}

fn code_from_area(area: &AreaDto) -> Option<String> {
    let code = area.code.as_ref()?.trim();
    if code.is_empty() {
        None
    } else {
        Some(code.to_ascii_uppercase())
    }
}

/// Walk the matches list and produce `(group_label, country_code)` pairs.
///
/// Used as the primary source for tournament group assignments because, for
/// pre-tournament feeds, `/standings` is empty until the competition starts
/// while `/matches` already lists every group-stage fixture with its `group`
/// field populated — both team `tla`s of every group-stage match name a
/// member of that group.
///
/// Output is deduplicated implicitly when the caller passes it through the
/// `assign_team` `ON CONFLICT DO NOTHING` upsert.
pub fn group_assignments_from_matches(matches: &[MatchDto]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for m in matches {
        // Only group-stage matches carry a meaningful `group` value.
        if !matches!(phase_from_stage(m.stage.as_deref()), Phase::Group) {
            continue;
        }
        let Some(label) = group_label_from_upstream(m.group.as_deref()) else {
            continue;
        };
        for team_ref in [&m.home_team, &m.away_team] {
            let Some(code) = team_ref_country_code(team_ref) else {
                continue;
            };
            out.push((label.clone(), code));
        }
    }
    out
}

/// Walk a standings document and produce the (group_label, country_code)
/// pairs needed to populate `tournament_group_teams`.
///
/// Skips any standing whose `stage` is not `GROUP_STAGE` (a single
/// `/standings` call may include the bracket too) and any row whose team
/// lacks a usable country code, so the seed CLI never assigns a team it
/// cannot match against `teams`. Returns an empty vector before the
/// tournament starts — callers should fall back to
/// `group_assignments_from_matches` in that case.
pub fn group_assignments_from_standings(standings: &[StandingDto]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in standings {
        let stage_is_group = entry
            .stage
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("GROUP_STAGE"))
            .unwrap_or(false);
        if !stage_is_group {
            continue;
        }
        let Some(label) = group_label_from_upstream(entry.group.as_deref()) else {
            continue;
        };
        for row in &entry.table {
            // The team ref inside standings carries `tla` but not `area`,
            // so we only have the 3-letter code as the country code candidate.
            let Some(code) = row
                .team
                .tla
                .as_ref()
                .map(|t| t.trim().to_ascii_uppercase())
                .filter(|t| !t.is_empty())
            else {
                continue;
            };
            out.push((label.clone(), code));
        }
    }
    out
}

/// Resolved scores extracted from a match DTO.
///
/// Captures both regulation goals and the optional ET breakdown, mirroring
/// the columns on the `matches` table.
#[derive(Debug, Clone, Default)]
pub struct ResolvedScores {
    pub home_score: Option<i32>,
    pub away_score: Option<i32>,
    pub et_home_score: Option<i32>,
    pub et_away_score: Option<i32>,
    /// Which side of the match won the penalty shootout, when one happened.
    /// The caller resolves this to a real `teams.id` via the team registry
    /// (the DTO does not carry team UUIDs).
    pub pens_winner: Option<PensWinner>,
}

/// Side of a match the upstream considers as winning the penalty shootout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PensWinner {
    Home,
    Away,
}

/// Reduce the upstream score breakdown to the columns we store.
///
/// Rules:
///   - When `duration = REGULAR` (or absent), `home_score` / `away_score`
///     come from `fullTime`.
///   - When `duration = EXTRA_TIME`, `home_score` / `away_score` come from
///     `regularTime` (the 90-minute snapshot) and `et_*_score` from
///     `extraTime` (only the goals scored *during* ET).
///   - When `duration = PENALTY_SHOOTOUT`, the ET values are populated as
///     above and `pens_winner` is derived from the `winner` enum (the
///     upstream marks the side that advanced via PKs as the match winner).
pub fn resolve_scores(score: Option<&ScoreDto>) -> ResolvedScores {
    let mut resolved = ResolvedScores::default();
    let Some(score) = score else {
        return resolved;
    };

    let duration = score.duration.unwrap_or(ScoreDuration::Regular);
    match duration {
        ScoreDuration::Regular | ScoreDuration::Unknown => {
            if let Some(ft) = score.full_time {
                resolved.home_score = ft.home;
                resolved.away_score = ft.away;
            }
        }
        ScoreDuration::ExtraTime => {
            if let Some(reg) = score.regular_time {
                resolved.home_score = reg.home;
                resolved.away_score = reg.away;
            } else if let Some(ft) = score.full_time {
                // Fallback if upstream omitted regularTime: we lose the
                // 90-minute snapshot but at least we record the post-ET
                // aggregate so the row is not left blank.
                resolved.home_score = ft.home;
                resolved.away_score = ft.away;
            }
            if let Some(et) = score.extra_time {
                resolved.et_home_score = et.home;
                resolved.et_away_score = et.away;
            }
        }
        ScoreDuration::PenaltyShootout => {
            if let Some(reg) = score.regular_time {
                resolved.home_score = reg.home;
                resolved.away_score = reg.away;
            }
            if let Some(et) = score.extra_time {
                resolved.et_home_score = et.home;
                resolved.et_away_score = et.away;
            }
            resolved.pens_winner = match score.winner {
                Some(WinnerDto::HomeTeam) => Some(PensWinner::Home),
                Some(WinnerDto::AwayTeam) => Some(PensWinner::Away),
                _ => None,
            };
        }
    }
    resolved
}

/// Parse the upstream's `utcDate` (RFC3339) into a `chrono` UTC timestamp.
pub fn parse_kickoff(utc_date: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(utc_date)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Convert a match DTO into the domain `Match`, given the round it belongs
/// to and a function that resolves a country code into a local team UUID.
///
/// The team resolver is injected so the same conversion logic works in the
/// CLI (where the resolver hits Postgres) and in unit tests (where it is a
/// closure over a HashMap).
///
/// Returns `None` only when the kickoff timestamp cannot be parsed —
/// everything else (missing scores, missing teams, unknown stages) has a
/// sensible fallback.
pub fn dto_to_match<F>(
    dto: &MatchDto,
    round_id: Uuid,
    new_id: Uuid,
    mut resolve_team: F,
) -> Option<Match>
where
    F: FnMut(&str) -> Option<Uuid>,
{
    let kickoff_at = parse_kickoff(&dto.utc_date)?;
    let scores = resolve_scores(dto.score.as_ref());

    let home_label = team_label(&dto.home_team);
    let away_label = team_label(&dto.away_team);
    let home_code = team_ref_country_code(&dto.home_team);
    let away_code = team_ref_country_code(&dto.away_team);
    let home_flag = home_code
        .as_deref()
        .map(flag_emoji)
        .unwrap_or_else(|| flag_emoji(""));
    let away_flag = away_code
        .as_deref()
        .map(flag_emoji)
        .unwrap_or_else(|| flag_emoji(""));

    let pens_winner_team_id = scores.pens_winner.and_then(|side| match side {
        PensWinner::Home => home_code.as_deref().and_then(&mut resolve_team),
        PensWinner::Away => away_code.as_deref().and_then(&mut resolve_team),
    });

    Some(Match {
        id: new_id,
        round_id,
        external_id: dto.id.to_string(),
        home_team: home_label,
        away_team: away_label,
        home_flag,
        away_flag,
        kickoff_at,
        home_score: scores.home_score,
        away_score: scores.away_score,
        et_home_score: scores.et_home_score,
        et_away_score: scores.et_away_score,
        pens_winner_team_id,
        status: match_status_from_dto(dto.status),
        phase: phase_from_stage(dto.stage.as_deref()),
    })
}

/// Best-effort human label for an embedded team ref. Falls back to "TBD"
/// when neither `name` nor `tla` is present (common for placeholders like
/// "Winner of Round of 16 Match 47" in early bracket data).
pub fn team_label(team: &sports_client::TeamRefDto) -> String {
    team.name
        .clone()
        .or_else(|| team.tla.clone())
        .unwrap_or_else(|| "TBD".to_string())
}

/// Country code for an embedded team ref. The matches endpoint does not
/// ship `area`, so we can only fall back to `tla`.
pub fn team_ref_country_code(team: &sports_client::TeamRefDto) -> Option<String> {
    team.tla
        .as_ref()
        .map(|t| t.trim().to_ascii_uppercase())
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sports_client::{HomeAwayDto, MatchStatusDto, ScoreDto, ScoreDuration, TeamRefDto};

    fn match_dto_skeleton() -> MatchDto {
        MatchDto {
            id: 1,
            utc_date: "2026-06-12T18:00:00Z".to_string(),
            status: MatchStatusDto::Scheduled,
            stage: Some("GROUP_STAGE".to_string()),
            group: Some("GROUP_A".to_string()),
            last_updated: None,
            home_team: TeamRefDto {
                id: Some(1),
                name: Some("Argentina".to_string()),
                short_name: None,
                tla: Some("ARG".to_string()),
                crest: None,
            },
            away_team: TeamRefDto {
                id: Some(2),
                name: Some("Spain".to_string()),
                short_name: None,
                tla: Some("ESP".to_string()),
                crest: None,
            },
            score: None,
        }
    }

    #[test]
    fn phase_picks_knockout_for_known_brackets() {
        assert_eq!(phase_from_stage(Some("GROUP_STAGE")), Phase::Group);
        // WC 2026 introduces an extra Round of 32 — the upstream tag is
        // LAST_32 and we must classify it as knockout for the scorer to
        // dispatch correctly.
        assert_eq!(phase_from_stage(Some("LAST_32")), Phase::Knockout);
        assert_eq!(phase_from_stage(Some("LAST_16")), Phase::Knockout);
        assert_eq!(phase_from_stage(Some("QUARTER_FINALS")), Phase::Knockout);
        assert_eq!(phase_from_stage(Some("FINAL")), Phase::Knockout);
        assert_eq!(phase_from_stage(None), Phase::Group);
        assert_eq!(phase_from_stage(Some("WHATEVER")), Phase::Group);
    }

    #[test]
    fn group_label_normalises_upstream_variants() {
        assert_eq!(
            group_label_from_upstream(Some("GROUP_A")),
            Some("Group A".to_string())
        );
        assert_eq!(
            group_label_from_upstream(Some("Group B")),
            Some("Group B".to_string())
        );
        assert_eq!(group_label_from_upstream(None), None);
        assert_eq!(group_label_from_upstream(Some("")), None);
    }

    #[test]
    fn resolves_regulation_score() {
        let score = ScoreDto {
            winner: Some(WinnerDto::HomeTeam),
            duration: Some(ScoreDuration::Regular),
            full_time: Some(HomeAwayDto {
                home: Some(2),
                away: Some(1),
            }),
            half_time: None,
            regular_time: None,
            extra_time: None,
            penalties: None,
        };
        let resolved = resolve_scores(Some(&score));
        assert_eq!(resolved.home_score, Some(2));
        assert_eq!(resolved.away_score, Some(1));
        assert!(resolved.et_home_score.is_none());
        assert!(resolved.pens_winner.is_none());
    }

    #[test]
    fn resolves_extra_time_score() {
        let score = ScoreDto {
            winner: Some(WinnerDto::AwayTeam),
            duration: Some(ScoreDuration::ExtraTime),
            full_time: Some(HomeAwayDto {
                home: Some(2),
                away: Some(3),
            }),
            half_time: None,
            regular_time: Some(HomeAwayDto {
                home: Some(1),
                away: Some(1),
            }),
            extra_time: Some(HomeAwayDto {
                home: Some(1),
                away: Some(2),
            }),
            penalties: None,
        };
        let resolved = resolve_scores(Some(&score));
        assert_eq!(resolved.home_score, Some(1));
        assert_eq!(resolved.away_score, Some(1));
        assert_eq!(resolved.et_home_score, Some(1));
        assert_eq!(resolved.et_away_score, Some(2));
        assert!(resolved.pens_winner.is_none());
    }

    #[test]
    fn resolves_penalty_shootout_winner() {
        let score = ScoreDto {
            winner: Some(WinnerDto::HomeTeam),
            duration: Some(ScoreDuration::PenaltyShootout),
            full_time: Some(HomeAwayDto {
                home: Some(2),
                away: Some(2),
            }),
            half_time: None,
            regular_time: Some(HomeAwayDto {
                home: Some(1),
                away: Some(1),
            }),
            extra_time: Some(HomeAwayDto {
                home: Some(1),
                away: Some(1),
            }),
            penalties: Some(HomeAwayDto {
                home: Some(4),
                away: Some(2),
            }),
        };
        let resolved = resolve_scores(Some(&score));
        assert_eq!(resolved.pens_winner, Some(PensWinner::Home));
        assert_eq!(resolved.et_home_score, Some(1));
    }

    #[test]
    fn dto_to_match_resolves_team_ids_via_callback() {
        let dto = match_dto_skeleton();
        let round_id = Uuid::new_v4();
        let new_id = Uuid::new_v4();
        let m = dto_to_match(&dto, round_id, new_id, |_code| None).expect("kickoff parses");

        assert_eq!(m.round_id, round_id);
        assert_eq!(m.external_id, "1");
        assert_eq!(m.home_team, "Argentina");
        assert_eq!(m.away_team, "Spain");
        assert_eq!(m.phase, Phase::Group);
        assert_eq!(m.status, MatchStatus::Scheduled);
        assert!(m.pens_winner_team_id.is_none());
    }
}
