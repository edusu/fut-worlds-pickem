use error_stack::ResultExt;
use serde::Deserialize;

use crate::error::{SharedError, SharedResult};

/// Configuration shared across services. Each service reads it from env vars
/// at startup; values come from `.env` in development and from the
/// orchestrator (Compose / k8s) in production.
///
/// Use `Config::from_env()` rather than reading env vars ad-hoc — that gives
/// us a single source of truth and a single failure point.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub database_url: String,
    pub nats_url: String,
    pub telegram_bot_token: String,
    pub telegram_webhook_url: Option<String>,
    pub football_api_key: String,
    pub api_bind_addr: String,
    pub otel_endpoint: Option<String>,
    pub otel_service_namespace: Option<String>,
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
            database_url: required("DATABASE_URL")?,
            nats_url: required("NATS_URL")?,
            telegram_bot_token: required("TELEGRAM_BOT_TOKEN")?,
            telegram_webhook_url: optional("TELEGRAM_WEBHOOK_URL"),
            football_api_key: required("FOOTBALL_API_KEY")?,
            api_bind_addr: required("API_BIND_ADDR")?,
            otel_endpoint: optional("OTEL_EXPORTER_OTLP_ENDPOINT"),
            otel_service_namespace: optional("OTEL_SERVICE_NAMESPACE"),
        })
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
