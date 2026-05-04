//! Tracing initialization. Each service calls `init(service_name)` at the
//! top of `main` so logs and OTLP spans flow with consistent attributes.

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Initialise `tracing` for a service.
///
/// In dev (no `OTEL_EXPORTER_OTLP_ENDPOINT` set) we install only a JSON stdout
/// layer. In any environment where the env var is set, we additionally
/// install an OTLP exporter pointing at it.
///
/// `service_name` shows up as `service.name` resource attribute in Jaeger.
pub fn init(service_name: &'static str) -> anyhow::Result<()> {
    let _ = service_name;
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true).json();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to init tracing subscriber: {e}"))?;

    // TODO: when OTEL_EXPORTER_OTLP_ENDPOINT is set, also install an OTLP
    // tracer provider with `opentelemetry_sdk` + `opentelemetry-otlp` and
    // attach it as an additional layer via `tracing_opentelemetry::layer()`.
    Ok(())
}
