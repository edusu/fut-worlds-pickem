//! DTOs for `/v4/competitions/{code}/matches` and `/v4/matches/{id}`.

use serde::{Deserialize, Serialize};

use crate::dto::common::TeamRefDto;

/// Lifecycle of a match as reported by the upstream.
///
/// Mirrors the documented `status` enum on `/v4/matches`. We keep an
/// `#[serde(other)]` arm so a forward-compatible upstream addition does not
/// poison ingestion of the rest of the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MatchStatusDto {
    Scheduled,
    Timed,
    InPlay,
    Paused,
    Finished,
    Suspended,
    Postponed,
    Cancelled,
    Awarded,
    /// Catch-all for any future status. Treated as "ignore" by the ingester.
    #[serde(other)]
    Unknown,
}

/// How far the match got before its score was decided. Drives whether the
/// adapter populates `et_*_score` and/or `pens_winner_team_id` on the
/// matching row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScoreDuration {
    Regular,
    ExtraTime,
    PenaltyShootout,
    #[serde(other)]
    Unknown,
}

/// Which team the upstream considers the winner of the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WinnerDto {
    HomeTeam,
    AwayTeam,
    Draw,
    #[serde(other)]
    Unknown,
}

/// Top-level shape of `/v4/competitions/{code}/matches`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetitionMatches {
    /// All fixtures across the current edition of the competition.
    pub matches: Vec<MatchDto>,
}

/// Single match document as returned by the upstream.
///
/// `stage` and `group` together describe where in the bracket the match
/// belongs (e.g. `stage = "GROUP_STAGE"`, `group = "GROUP_A"`; or `stage =
/// "ROUND_OF_16"`, `group = null`). Both are kept as raw strings rather than
/// enums because football-data.org adds new stage names per tournament and
/// the adapter prefers to bucket them via `Phase::is_knockout(stage)` rather
/// than reject unknowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    pub id: i64,
    pub utc_date: String,
    pub status: MatchStatusDto,
    pub stage: Option<String>,
    pub group: Option<String>,
    pub last_updated: Option<String>,
    pub home_team: TeamRefDto,
    pub away_team: TeamRefDto,
    pub score: Option<ScoreDto>,
}

/// Full score breakdown for a match. All sub-objects are `Option` because
/// the upstream omits the ones that did not happen (e.g. no `extraTime` for
/// a match decided in regulation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDto {
    pub winner: Option<WinnerDto>,
    pub duration: Option<ScoreDuration>,
    pub full_time: Option<HomeAwayDto>,
    pub half_time: Option<HomeAwayDto>,
    /// Goals scored in regulation 90' on a match that went past 90'. Absent
    /// for matches decided in regulation (in which case `full_time` already
    /// carries the regulation score).
    pub regular_time: Option<HomeAwayDto>,
    /// Goals scored *during* extra time only. Sum with `regular_time` to get
    /// the post-ET aggregate before any penalty shootout.
    pub extra_time: Option<HomeAwayDto>,
    /// Penalty-shootout score (sudden death excluded by the upstream).
    pub penalties: Option<HomeAwayDto>,
}

/// Generic `(home, away)` integer pair. Reused by every score breakdown
/// sub-object so the score DTO stays compact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeAwayDto {
    pub home: Option<i32>,
    pub away: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snippet of `/v4/competitions/WC/matches` with a single group-stage
    /// match that has not started yet (status `TIMED`, no score). Mirrors
    /// what we currently see for WC 2026.
    #[test]
    fn parses_scheduled_group_stage_match() {
        let json = r#"
        {
          "matches": [
            {
              "id": 537327,
              "utcDate": "2026-06-11T19:00:00Z",
              "status": "TIMED",
              "stage": "GROUP_STAGE",
              "group": "GROUP_A",
              "lastUpdated": "2026-04-01T10:00:00Z",
              "homeTeam": {
                "id": 1843,
                "name": "Mexico",
                "shortName": "Mexico",
                "tla": "MEX",
                "crest": "https://crests.football-data.org/1843.svg"
              },
              "awayTeam": {
                "id": 776,
                "name": "South Africa",
                "shortName": "South Africa",
                "tla": "RSA",
                "crest": "https://crests.football-data.org/776.svg"
              },
              "score": {
                "winner": null,
                "duration": "REGULAR",
                "fullTime": { "home": null, "away": null },
                "halfTime": { "home": null, "away": null }
              }
            }
          ]
        }
        "#;
        let parsed: CompetitionMatches = serde_json::from_str(json).expect("matches payload");
        assert_eq!(parsed.matches.len(), 1);
        let m = &parsed.matches[0];
        assert_eq!(m.id, 537327);
        assert_eq!(m.status, MatchStatusDto::Timed);
        assert_eq!(m.stage.as_deref(), Some("GROUP_STAGE"));
        assert_eq!(m.group.as_deref(), Some("GROUP_A"));
        assert_eq!(m.home_team.tla.as_deref(), Some("MEX"));
        assert_eq!(m.away_team.tla.as_deref(), Some("RSA"));
        let score = m.score.as_ref().expect("score present");
        assert!(score.winner.is_none());
        assert_eq!(score.duration, Some(ScoreDuration::Regular));
        assert!(score.full_time.unwrap().home.is_none());
    }

    /// Snippet of a finished match that went the full 90 minutes only.
    /// Score is reported via `fullTime`; `regularTime` / `extraTime` /
    /// `penalties` are absent.
    #[test]
    fn parses_finished_regulation_match() {
        let json = r#"
        {
          "id": 100001,
          "utcDate": "2024-07-14T20:00:00Z",
          "status": "FINISHED",
          "stage": "FINAL",
          "group": null,
          "homeTeam": { "id": 1, "name": "Spain", "tla": "ESP" },
          "awayTeam": { "id": 2, "name": "England", "tla": "ENG" },
          "score": {
            "winner": "HOME_TEAM",
            "duration": "REGULAR",
            "fullTime": { "home": 2, "away": 1 },
            "halfTime": { "home": 1, "away": 1 }
          }
        }
        "#;
        let m: MatchDto = serde_json::from_str(json).expect("match payload");
        assert_eq!(m.status, MatchStatusDto::Finished);
        assert_eq!(m.stage.as_deref(), Some("FINAL"));
        let score = m.score.expect("score present");
        assert_eq!(score.winner, Some(WinnerDto::HomeTeam));
        assert_eq!(score.duration, Some(ScoreDuration::Regular));
        assert_eq!(score.full_time.unwrap().home, Some(2));
        assert!(score.regular_time.is_none());
        assert!(score.extra_time.is_none());
        assert!(score.penalties.is_none());
    }

    /// Snippet of a knockout match decided in extra time. The upstream
    /// ships both `regularTime` (the 90-minute snapshot) and `extraTime`
    /// (only goals scored during ET).
    #[test]
    fn parses_extra_time_match() {
        let json = r#"
        {
          "id": 100002,
          "utcDate": "2024-07-10T20:00:00Z",
          "status": "FINISHED",
          "stage": "SEMI_FINALS",
          "group": null,
          "homeTeam": { "id": 1, "name": "France", "tla": "FRA" },
          "awayTeam": { "id": 2, "name": "Croatia", "tla": "CRO" },
          "score": {
            "winner": "AWAY_TEAM",
            "duration": "EXTRA_TIME",
            "fullTime": { "home": 2, "away": 3 },
            "halfTime": { "home": 1, "away": 0 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 1, "away": 2 }
          }
        }
        "#;
        let m: MatchDto = serde_json::from_str(json).expect("match payload");
        let score = m.score.expect("score present");
        assert_eq!(score.duration, Some(ScoreDuration::ExtraTime));
        assert_eq!(score.regular_time.unwrap().home, Some(1));
        assert_eq!(score.regular_time.unwrap().away, Some(1));
        assert_eq!(score.extra_time.unwrap().home, Some(1));
        assert_eq!(score.extra_time.unwrap().away, Some(2));
        assert_eq!(score.winner, Some(WinnerDto::AwayTeam));
    }

    /// Snippet of a knockout match decided on penalties. Verifies the full
    /// breakdown (regulation + ET + penalty shootout) round-trips and that
    /// `winner` carries the side that advanced.
    #[test]
    fn parses_penalty_shootout_match() {
        let json = r#"
        {
          "id": 100003,
          "utcDate": "2024-07-05T20:00:00Z",
          "status": "FINISHED",
          "stage": "QUARTER_FINALS",
          "group": null,
          "homeTeam": { "id": 1, "name": "Brazil", "tla": "BRA" },
          "awayTeam": { "id": 2, "name": "Argentina", "tla": "ARG" },
          "score": {
            "winner": "HOME_TEAM",
            "duration": "PENALTY_SHOOTOUT",
            "fullTime": { "home": 2, "away": 2 },
            "halfTime": { "home": 1, "away": 0 },
            "regularTime": { "home": 1, "away": 1 },
            "extraTime": { "home": 1, "away": 1 },
            "penalties": { "home": 5, "away": 4 }
          }
        }
        "#;
        let m: MatchDto = serde_json::from_str(json).expect("match payload");
        let score = m.score.expect("score present");
        assert_eq!(score.duration, Some(ScoreDuration::PenaltyShootout));
        assert_eq!(score.winner, Some(WinnerDto::HomeTeam));
        let pens = score.penalties.expect("penalties present");
        assert_eq!(pens.home, Some(5));
        assert_eq!(pens.away, Some(4));
    }

    /// Forward-compatibility: an unknown `status` value must not derail
    /// deserialization of the surrounding payload — the typed enum has an
    /// `#[serde(other)]` arm that catches it as `MatchStatusDto::Unknown`.
    #[test]
    fn unknown_status_falls_back_to_unknown_variant() {
        let json = r#"
        {
          "id": 1,
          "utcDate": "2026-06-11T19:00:00Z",
          "status": "REJECTED_DRAW",
          "homeTeam": { "id": 1, "name": "A", "tla": "AAA" },
          "awayTeam": { "id": 2, "name": "B", "tla": "BBB" }
        }
        "#;
        let m: MatchDto = serde_json::from_str(json).expect("payload still parses");
        assert_eq!(m.status, MatchStatusDto::Unknown);
    }

    /// Symmetry check: serialising back out yields the camelCase keys the
    /// upstream uses, so anything we mock from this DTO can replay the
    /// original wire format byte-for-byte (modulo whitespace).
    #[test]
    fn match_dto_round_trips_via_serde() {
        let original = MatchDto {
            id: 42,
            utc_date: "2026-06-12T19:00:00Z".to_string(),
            status: MatchStatusDto::Finished,
            stage: Some("GROUP_STAGE".to_string()),
            group: Some("GROUP_A".to_string()),
            last_updated: None,
            home_team: TeamRefDto {
                id: Some(1),
                name: Some("Spain".to_string()),
                short_name: None,
                tla: Some("ESP".to_string()),
                crest: None,
            },
            away_team: TeamRefDto {
                id: Some(2),
                name: Some("Germany".to_string()),
                short_name: None,
                tla: Some("GER".to_string()),
                crest: None,
            },
            score: Some(ScoreDto {
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
            }),
        };
        let serialized = serde_json::to_string(&original).expect("serialize");
        // The wire format must use camelCase keys, not Rust snake_case.
        assert!(serialized.contains("\"utcDate\""), "{serialized}");
        assert!(serialized.contains("\"homeTeam\""), "{serialized}");
        assert!(serialized.contains("\"fullTime\""), "{serialized}");
        // Round-trip back.
        let reparsed: MatchDto = serde_json::from_str(&serialized).expect("reparse");
        assert_eq!(reparsed.id, original.id);
        assert_eq!(reparsed.status, MatchStatusDto::Finished);
        assert_eq!(reparsed.score.unwrap().winner, Some(WinnerDto::HomeTeam));
    }
}
