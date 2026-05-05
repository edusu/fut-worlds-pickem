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
    pub async fn get_competition_matches(
        &self,
        competition_code: &str,
    ) -> SportsResult<CompetitionMatches> {
        let path = format!("/competitions/{competition_code}/matches");
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

/// Subset of upstream payload we care about. Extend as ingestion needs grow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitionMatches {
    pub matches: Vec<MatchDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchDto {
    pub id: i64,
    pub home_team: TeamDto,
    pub away_team: TeamDto,
    pub status: String,
    pub utc_date: String,
    pub score: Option<ScoreDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamDto {
    pub name: String,
    pub tla: Option<String>,
    pub crest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreDto {
    pub full_time: Option<FullTimeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTimeDto {
    pub home: Option<i32>,
    pub away: Option<i32>,
}
