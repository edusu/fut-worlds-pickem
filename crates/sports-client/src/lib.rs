//! Thin wrapper around football-data.org (or any equivalent) used by the
//! `events` service to ingest fixtures and results.
//!
//! Keeping the upstream API behind a typed client lets us swap providers
//! later without touching ingestion logic. Errors live in `crate::error`.
//!
//! HTTP layering (top to bottom): `RetryingClient -> RateLimitedClient ->
//! reqwest::Client`. The rate limiter paces outgoing requests at the
//! free-tier ceiling so we never trigger the upstream 60-second cooldown,
//! and the retry layer absorbs `429` / `5xx` blips with exponential
//! backoff while honoring server-supplied `Retry-After` headers.

pub mod error;

pub use error::{SportsClientError, SportsClientReport, SportsResult};

use std::num::NonZeroU32;

use error_stack::{Report, ResultExt};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::StatusCode;
use rust_utils::network::{
    validate_and_parse_json, RateLimitWindow, RateLimitedClient, RetryPolicy, RetryingClient,
};
use rust_utils::secret::Secret;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Default upstream base URL.
const DEFAULT_BASE_URL: &str = "https://api.football-data.org/v4";

/// Free-tier rate limit. Going over results in a 60-second cooldown
/// enforced server-side, which is far more disruptive than waiting locally.
const RATE_LIMIT_PER_MINUTE: u32 = 10;

/// Total HTTP attempts (initial + retries). football-data.org occasionally
/// returns 502/503 during high-traffic events; 4 attempts (initial + 3
/// retries) absorbs those blips without hammering on a sustained outage.
const MAX_HTTP_ATTEMPTS: u32 = 4;

/// Hard cap on response body size before JSON decoding. The largest expected
/// payload (`/competitions/WC/matches` with the full World Cup bracket) is
/// ~30-40 KiB; 1 MiB is a wide safety margin that still rejects pathological
/// inputs before they tie up the parser.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Maximum JSON nesting depth accepted before deserialization. Upstream
/// payloads are flat (competition → matches → teams/score), so 32 levels
/// is far above any legitimate response.
const MAX_JSON_DEPTH: usize = 32;

/// HTTP statuses that should trigger a retry. `429` is the rate-limit
/// signal (paired with `Retry-After`); `5xx` are upstream blips that
/// usually clear within a few seconds.
const RETRIABLE_STATUSES: &[StatusCode] = &[
    StatusCode::TOO_MANY_REQUESTS,
    StatusCode::INTERNAL_SERVER_ERROR,
    StatusCode::BAD_GATEWAY,
    StatusCode::SERVICE_UNAVAILABLE,
    StatusCode::GATEWAY_TIMEOUT,
];

/// Football-data.org API client.
///
/// The API key is sent on the `X-Auth-Token` header on every request.
/// `Client` itself is `Send + Sync` and the inner HTTP client is cheap to
/// share — wrap it in an `Arc` if multiple consumers need it concurrently.
pub struct Client {
    http: RetryingClient<RateLimitedClient>,
    api_key: Secret<String>,
    base_url: String,
}

impl Client {
    /// Build a new client with rate limiting and retry policies wired in.
    ///
    /// Returns `Err(SportsClientError::Config)` if the rate-limit window
    /// rejects construction (would only happen if the constants in this
    /// module are mis-edited to a zero value).
    pub fn new(api_key: Secret<String>) -> SportsResult<Self> {
        let rpm = NonZeroU32::new(RATE_LIMIT_PER_MINUTE)
            .expect("RATE_LIMIT_PER_MINUTE is non-zero by construction");
        let limited = RateLimitedClient::new(RateLimitWindow::PerMinute(rpm), None)
            .change_context(SportsClientError::Config)
            .attach_with(|| format!("rate limit window: {RATE_LIMIT_PER_MINUTE}/min"))?;

        let attempts = NonZeroU32::new(MAX_HTTP_ATTEMPTS)
            .expect("MAX_HTTP_ATTEMPTS is non-zero by construction");
        let policy = RetryPolicy::default()
            .max_attempts(attempts)
            .retry_statuses(RETRIABLE_STATUSES.iter().copied());
        let http = RetryingClient::new(limited, policy);

        Ok(Self {
            http,
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
        })
    }

    /// Override the upstream base URL (mainly for tests against a mock server).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Fetch all fixtures for a given competition (e.g. "WC" for FIFA World Cup).
    ///
    /// The returned payload includes every match the upstream knows about for
    /// the current edition of the competition: scheduled, in-play, finished,
    /// postponed, cancelled. Filtering by status is the caller's job — the
    /// ingester wants the full list so it can detect transitions on its own.
    pub async fn get_competition_matches(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionMatches> {
        let path = format!("/competitions/{competition_code}/matches");
        self.get_json(&path).await
    }

    /// Fetch every team participating in a competition.
    ///
    /// For national-team competitions (World Cup, EUROs, Copa America) this
    /// yields the participating countries; the response carries `area.code`
    /// (3-letter FIFA country code) which is what the ingester maps onto
    /// `teams.country_code` and uses to derive a flag emoji.
    pub async fn get_competition_teams(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionTeams> {
        let path = format!("/competitions/{competition_code}/teams");
        self.get_json(&path).await
    }

    /// Fetch the standings of a competition.
    ///
    /// Used at tournament-data seed time to discover the "Group A..L"
    /// breakdown of the World Cup group stage: each entry in `standings`
    /// carries a `stage` (e.g. `GROUP_STAGE`) and a `group` (e.g. `GROUP_A`)
    /// alongside the per-team `table` rows, so we can populate
    /// `tournament_groups` + `tournament_group_teams` from a single call.
    pub async fn get_competition_standings(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionStandings> {
        let path = format!("/competitions/{competition_code}/standings");
        self.get_json(&path).await
    }

    /// Fetch a single match by its upstream id.
    pub async fn get_match_by_id(&self, match_id: &str) -> SportsResult<MatchDto> {
        let path = format!("/matches/{match_id}");
        self.get_json(&path).await
    }

    /// Issue a `GET` against `{base_url}{path}` with the auth header,
    /// validate the response status, and decode the body as `T`.
    ///
    /// Decoding goes through `validate_and_parse_json` so an oversized or
    /// pathologically nested payload is rejected before `serde_json` sees
    /// it — defends the worker against a hostile or buggy upstream.
    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> SportsResult<T> {
        let url = format!("{}{}", self.base_url, path);

        // `reqwest::Url` parsing is fallible (unlikely with our composed
        // strings, but the type insists). Treat it as a `Config` failure
        // since it would only fire on an obviously malformed `base_url`.
        let parsed_url: reqwest::Url = url
            .parse()
            .change_context(SportsClientError::Config)
            .attach_with(|| format!("could not parse URL: {url}"))?;

        let mut request = reqwest::Request::new(reqwest::Method::GET, parsed_url);
        let header_name = HeaderName::from_static("x-auth-token");
        let header_value = HeaderValue::from_str(self.api_key.expose())
            .change_context(SportsClientError::Config)
            .attach("X-Auth-Token contains characters not allowed in HTTP headers")?;
        request.headers_mut().insert(header_name, header_value);

        // The retry layer always returns the last observed result, so
        // a non-success status surfaces here as `Ok(response)` rather than
        // an error — we still need to inspect `.status()` ourselves.
        let response = self
            .http
            .execute(request)
            .await
            .change_context(SportsClientError::Http)
            .attach_with(|| format!("GET {url}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(Report::new(SportsClientError::Upstream)
                .attach(format!("status {status}, GET {url}")));
        }

        let body = response
            .bytes()
            .await
            .change_context(SportsClientError::Decode)
            .attach_with(|| format!("failed to read body, GET {url}"))?;

        validate_and_parse_json::<T>(&body, MAX_RESPONSE_BYTES, MAX_JSON_DEPTH)
            .change_context(SportsClientError::Decode)
            .attach_with(|| format!("decode failed, GET {url}, {} bytes", body.len()))
    }
}

// =============================================================================
// DTOs — wire-format types that mirror what football-data.org actually returns.
//
// Conventions:
//   - All structs are `#[serde(rename_all = "camelCase")]` so Rust snake_case
//     fields map onto the upstream's camelCase keys (`utcDate`, `fullTime`...).
//   - Anything the ingester does not strictly require is `Option<T>` — even
//     fields that "always" exist according to the docs — because the upstream
//     occasionally ships partial documents during reschedules.
//   - Enums for `status` and `score.duration` are typed (not raw strings) so
//     the adapter can pattern-match without re-doing the validation.
// =============================================================================

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

/// Team reference embedded inside a match. Lighter than the standalone
/// `TeamDto` (no `area`, no `coach`) because the matches endpoint does not
/// ship those.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRefDto {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub short_name: Option<String>,
    pub tla: Option<String>,
    pub crest: Option<String>,
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

/// Top-level shape of `/v4/competitions/{code}/teams`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetitionTeams {
    pub count: Option<i32>,
    pub teams: Vec<TeamDto>,
}

/// Standalone team document. Fields beyond those used by the ingester
/// (`coach`, `squad`, `runningCompetitions`, `venue`, ...) are intentionally
/// dropped — extending the DTO is cheap when a new ingestion path needs them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamDto {
    pub id: i64,
    pub name: String,
    pub short_name: Option<String>,
    /// Three-letter FIFA-style code (e.g. "ARG", "ESP").
    pub tla: Option<String>,
    pub crest: Option<String>,
    pub area: Option<AreaDto>,
}

/// Geographical area (country, region, continent) the team belongs to.
/// For national teams this aligns 1:1 with the team itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaDto {
    pub id: Option<i64>,
    pub name: Option<String>,
    /// 3-letter FIFA / IOC-style country code. Used as `teams.country_code`.
    pub code: Option<String>,
    /// Crest URL (SVG/PNG). Not an emoji.
    pub flag: Option<String>,
}

/// Top-level shape of `/v4/competitions/{code}/standings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompetitionStandings {
    pub standings: Vec<StandingDto>,
}

/// One standings table. For the World Cup group stage there is one entry per
/// group (`group = Some("GROUP_A")`); for league competitions `group` is
/// `None` and `type` discriminates `TOTAL` / `HOME` / `AWAY`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingDto {
    pub stage: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub group: Option<String>,
    pub table: Vec<StandingsRowDto>,
}

/// Single row inside a standings table. We only consume `team` (to learn
/// which teams belong to the group); the rest is kept around for future
/// dashboards / debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StandingsRowDto {
    pub position: Option<i32>,
    pub team: TeamRefDto,
    pub played_games: Option<i32>,
    pub won: Option<i32>,
    pub draw: Option<i32>,
    pub lost: Option<i32>,
    pub points: Option<i32>,
    pub goals_for: Option<i32>,
    pub goals_against: Option<i32>,
    pub goal_difference: Option<i32>,
}

// =============================================================================
// Wire-format tests.
//
// Each test embeds a literal JSON snippet shaped after a real
// football-data.org response and verifies that our DTOs deserialize it
// without losing fields. They run offline so they do not consume the
// upstream rate budget, and they break loudly the moment the upstream
// renames a key — which is the failure mode we most want to catch early.
// =============================================================================
#[cfg(test)]
mod wire_format_tests {
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

    /// Snippet of `/v4/competitions/{code}/teams` with a national team
    /// (Argentina). Verifies `area` and its nested `code` deserialize.
    #[test]
    fn parses_competition_teams_with_area() {
        let json = r#"
        {
          "count": 1,
          "teams": [
            {
              "id": 759,
              "name": "Argentina",
              "shortName": "Argentina",
              "tla": "ARG",
              "crest": "https://crests.football-data.org/759.svg",
              "area": {
                "id": 2010,
                "name": "Argentina",
                "code": "ARG",
                "flag": "https://crests.football-data.org/759.svg"
              }
            }
          ]
        }
        "#;
        let parsed: CompetitionTeams = serde_json::from_str(json).expect("teams payload");
        assert_eq!(parsed.count, Some(1));
        assert_eq!(parsed.teams.len(), 1);
        let t = &parsed.teams[0];
        assert_eq!(t.tla.as_deref(), Some("ARG"));
        let area = t.area.as_ref().expect("area present");
        assert_eq!(area.code.as_deref(), Some("ARG"));
    }

    /// Snippet of `/v4/competitions/{code}/standings` with a single group
    /// table — the shape we expect to receive once the World Cup starts.
    /// Confirms `stage` / `type` / `group` / `table` all line up with our DTOs.
    #[test]
    fn parses_competition_standings_group_stage() {
        let json = r#"
        {
          "standings": [
            {
              "stage": "GROUP_STAGE",
              "type": "TOTAL",
              "group": "GROUP_A",
              "table": [
                {
                  "position": 1,
                  "team": { "id": 1843, "name": "Mexico", "tla": "MEX" },
                  "playedGames": 3,
                  "won": 2,
                  "draw": 1,
                  "lost": 0,
                  "points": 7,
                  "goalsFor": 5,
                  "goalsAgainst": 2,
                  "goalDifference": 3
                }
              ]
            }
          ]
        }
        "#;
        let parsed: CompetitionStandings = serde_json::from_str(json).expect("standings payload");
        assert_eq!(parsed.standings.len(), 1);
        let s = &parsed.standings[0];
        assert_eq!(s.stage.as_deref(), Some("GROUP_STAGE"));
        assert_eq!(s.type_.as_deref(), Some("TOTAL"));
        assert_eq!(s.group.as_deref(), Some("GROUP_A"));
        assert_eq!(s.table.len(), 1);
        let row = &s.table[0];
        assert_eq!(row.position, Some(1));
        assert_eq!(row.team.tla.as_deref(), Some("MEX"));
        assert_eq!(row.points, Some(7));
        assert_eq!(row.goal_difference, Some(3));
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
