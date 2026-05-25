use std::time::Duration;

use error_stack::ResultExt;
use rust_utils::secret::Secret;

use crate::error::{SharedError, SharedResult};

/// Default cadence for the events scorer's reconcile loop.
const DEFAULT_SCORER_RECONCILE_INTERVAL_SECS: u64 = 600;

/// Default cadence for the events reminder scheduler tick.
const DEFAULT_REMINDER_TICK_INTERVAL_SECS: u64 = 600;
/// Default lower bound of the reminder window (hours before deadline).
const DEFAULT_REMINDER_WINDOW_HOURS_MIN: u64 = 6;
/// Default upper bound of the reminder window (hours before deadline).
const DEFAULT_REMINDER_WINDOW_HOURS_MAX: u64 = 24;

/// Configuration shared across services. Each service reads it from env vars
/// at startup; values come from `.env` in development and from the
/// orchestrator (Compose / k8s) in production.
///
/// Use `Config::from_env()` rather than reading env vars ad-hoc — that gives
/// us a single source of truth and a single failure point.
///
/// Secret-bearing fields (`database_url`, `telegram_bot_token`,
/// `football_api_key`) are wrapped in `Secret<String>` so a stray
/// `tracing!("{:?}", config)` or panic backtrace cannot leak credentials.
/// Call sites that need the raw value use `.expose()`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Postgres connection string. Includes the password in the URL form
    /// `postgres://user:pass@host:5432/db`, hence `Secret`.
    pub database_url: Secret<String>,
    /// NATS connection URL. Not a secret in our deployments (no inline
    /// credentials), so kept as a plain `String` for ergonomics.
    pub nats_url: String,
    /// Telegram bot token from @BotFather. Anyone with this token can
    /// impersonate the bot.
    pub telegram_bot_token: Secret<String>,
    pub telegram_webhook_url: Option<String>,
    /// football-data.org API key. Tied to a paid quota — leaking it lets a
    /// third party drain the quota or get the account suspended.
    pub football_api_key: Secret<String>,
    pub api_bind_addr: String,
    /// Origin allowed by CORS for the Mini App. Defaults to the Vite dev
    /// server (`http://localhost:5173`) when the env var is unset, so a
    /// fresh checkout works without extra configuration.
    pub miniapp_origin: String,
    /// Public URL of the Mini App that Telegram launches when the user
    /// taps the bot's menu button or an inline `web_app` button.
    /// Telegram requires HTTPS in production. Defaults to the Vite dev
    /// server so a fresh checkout works without extra configuration.
    pub miniapp_url: String,
    pub otel_endpoint: Option<String>,
    pub otel_service_namespace: Option<String>,
    /// Cadence for the events scorer's reconcile loop, in seconds.
    /// `None` falls back to [`DEFAULT_SCORER_RECONCILE_INTERVAL_SECS`].
    /// Use [`Config::scorer_reconcile_interval`] to consume — it
    /// already handles the default.
    pub scorer_reconcile_interval_secs: Option<u64>,
    /// Cadence for the events reminder scheduler tick, in seconds.
    /// `None` falls back to [`DEFAULT_REMINDER_TICK_INTERVAL_SECS`].
    pub reminder_tick_interval_secs: Option<u64>,
    /// Lower bound (hours from `now`) of the deadline window that fires
    /// reminder DMs. `None` falls back to
    /// [`DEFAULT_REMINDER_WINDOW_HOURS_MIN`].
    pub reminder_window_hours_min: Option<u64>,
    /// Upper bound (hours from `now`) of the deadline window. `None`
    /// falls back to [`DEFAULT_REMINDER_WINDOW_HOURS_MAX`].
    pub reminder_window_hours_max: Option<u64>,
}

impl Config {
    /// Load the configuration from process env vars.
    ///
    /// Required keys: `DATABASE_URL`, `NATS_URL`, `TELEGRAM_BOT_TOKEN`,
    /// `FOOTBALL_API_KEY`, `API_BIND_ADDR`. Missing values produce a
    /// `SharedError::MissingConfig` so the service refuses to start with a
    /// half-baked configuration. The offending env-var name is attached to
    /// the report chain via `attach_with`.
    pub fn from_env() -> SharedResult<Self> {
        Ok(Self {
            database_url: Secret::new(required("DATABASE_URL")?),
            nats_url: required("NATS_URL")?,
            telegram_bot_token: Secret::new(required("TELEGRAM_BOT_TOKEN")?),
            telegram_webhook_url: optional("TELEGRAM_WEBHOOK_URL"),
            football_api_key: Secret::new(required("FOOTBALL_API_KEY")?),
            api_bind_addr: required("API_BIND_ADDR")?,
            miniapp_origin: optional_with_default("MINIAPP_ORIGIN", "http://localhost:5173"),
            miniapp_url: optional_with_default("MINIAPP_URL", "http://localhost:5173"),
            otel_endpoint: optional("OTEL_EXPORTER_OTLP_ENDPOINT"),
            otel_service_namespace: optional("OTEL_SERVICE_NAMESPACE"),
            scorer_reconcile_interval_secs: optional_u64("SCORER_RECONCILE_INTERVAL_SECS")?,
            reminder_tick_interval_secs: optional_u64("REMINDER_TICK_INTERVAL_SECS")?,
            reminder_window_hours_min: optional_u64("REMINDER_WINDOW_HOURS_MIN")?,
            reminder_window_hours_max: optional_u64("REMINDER_WINDOW_HOURS_MAX")?,
        })
    }

    /// Resolve the configured scorer reconcile interval, applying the
    /// 600s default when the env var is unset.
    pub fn scorer_reconcile_interval(&self) -> Duration {
        Duration::from_secs(
            self.scorer_reconcile_interval_secs
                .unwrap_or(DEFAULT_SCORER_RECONCILE_INTERVAL_SECS),
        )
    }

    /// Resolve the configured reminder tick cadence, applying the 600s
    /// default when the env var is unset.
    pub fn reminder_tick_interval(&self) -> Duration {
        Duration::from_secs(
            self.reminder_tick_interval_secs
                .unwrap_or(DEFAULT_REMINDER_TICK_INTERVAL_SECS),
        )
    }

    /// Resolve the reminder window bounds as a `(min, max)` pair of
    /// hour offsets from `now`. Falls back to the 6h/24h defaults.
    ///
    /// # Panics
    /// Panics if `min >= max` — an inverted window would silently
    /// disable all reminders via the SQL `BETWEEN` clause.
    pub fn reminder_window_hours(&self) -> (u64, u64) {
        let min = self
            .reminder_window_hours_min
            .unwrap_or(DEFAULT_REMINDER_WINDOW_HOURS_MIN);
        let max = self
            .reminder_window_hours_max
            .unwrap_or(DEFAULT_REMINDER_WINDOW_HOURS_MAX);
        assert!(
            min < max,
            "REMINDER_WINDOW_HOURS_MIN ({min}) must be less than REMINDER_WINDOW_HOURS_MAX ({max})"
        );
        (min, max)
    }
}

/// Read a required env var, returning a `SharedError::MissingConfig` report
/// with the offending key attached if it is unset or empty.
fn required(key: &'static str) -> SharedResult<String> {
    std::env::var(key)
        .change_context(SharedError::MissingConfig)
        .attach_with(|| format!("env var: {key}"))
}

/// Read an optional env var, returning `None` if unset or empty.
fn optional(key: &'static str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Read an optional env var, falling back to a static default. Wraps the
/// recurring `optional(...).unwrap_or_else(...)` so the default lives next
/// to the key in the call site.
fn optional_with_default(key: &'static str, default: &str) -> String {
    optional(key).unwrap_or_else(|| default.to_string())
}

/// Read an optional env var as a `u64`. Returns `None` when unset or
/// empty; surfaces parse failures as `SharedError::MissingConfig`
/// (with the env-var name attached) so a typo'd value fails closed
/// instead of silently falling back to the default.
fn optional_u64(key: &'static str) -> SharedResult<Option<u64>> {
    let Some(raw) = optional(key) else {
        return Ok(None);
    };
    raw.parse::<u64>().map(Some).map_err(|_| {
        error_stack::Report::new(SharedError::MissingConfig)
            .attach(format!("env var {key} not parseable as u64: {raw}"))
    })
}
