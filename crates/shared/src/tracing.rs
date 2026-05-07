//! Tracing initialization. Each service calls `init(service_name)` at the
//! top of `main` so logs flow through a consistent JSON formatter.

use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Install a JSON stdout subscriber driven by `RUST_LOG` (default `info`).
///
/// `service_name` is logged once at startup so multi-service stdout streams
/// can be disambiguated. OTLP wiring is intentionally absent until a real
/// exporter consumer (Jaeger, Tempo) is provisioned in deploy.
pub fn init(service_name: &'static str) -> anyhow::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true).json();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to init tracing subscriber: {e}"))?;

    info!(service = service_name, "tracing initialized");
    Ok(())
}
