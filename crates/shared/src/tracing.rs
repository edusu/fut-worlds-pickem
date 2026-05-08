//! Tracing initialization.
//!
//! Each service calls [`init`] at the top of `main`. Two layers may be
//! installed depending on configuration:
//!
//! 1. **`fmt`** — JSON-formatted spans/events to stdout, filtered by
//!    `RUST_LOG` (default `info`). Always installed; this is what
//!    `journalctl` / `docker logs` consumers see.
//!
//! 2. **`opentelemetry`** — installed only when `otel_endpoint` is `Some`.
//!    Exports spans via OTLP gRPC to a collector (Jaeger, Tempo, ...). In
//!    development the endpoint typically points at `http://localhost:4317`
//!    (the Jaeger container in `infra/docker-compose.yml`).
//!
//! [`init`] returns a [`TracingGuard`] whose `Drop` flushes the OTLP batch
//! exporter synchronously. Hold the guard alive for the whole lifetime of
//! the program: `let _guard = shared::tracing::init(...)?`.

use opentelemetry::trace::TracerProvider;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// RAII guard that owns the `SdkTracerProvider` when OTLP is enabled. On
/// drop it calls `shutdown()`, which flushes any spans still sitting in
/// the batch queue so the last few seconds of activity before process exit
/// are not silently lost. When OTLP is disabled the guard is empty and
/// drop is a no-op.
///
/// Holding the guard is mandatory: `let _ = init(...)?;` (no binding)
/// flushes immediately and disables exports for the rest of the run. Use
/// the underscore-prefix convention `let _guard = init(...)?;` to silence
/// the unused-binding lint while keeping the guard alive.
pub struct TracingGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            // Best-effort shutdown. We log and swallow the error because
            // there is nothing useful a service can do at process exit —
            // dropping past this point happens during teardown.
            if let Err(err) = provider.shutdown() {
                eprintln!("tracing: provider shutdown failed: {err}");
            }
        }
    }
}

/// Install the global tracing subscriber.
///
/// Always installs the JSON `fmt` layer; additionally installs the OTLP
/// exporter when `otel_endpoint` is `Some`. `service_name` is recorded as
/// the OTel `service.name` resource attribute so traces are filterable by
/// service in the Jaeger UI; `otel_namespace` becomes `service.namespace`
/// (groups multiple services of the same project under one namespace, e.g.
/// `fut-worlds-pickem`).
pub fn init(
    service_name: &'static str,
    otel_endpoint: Option<&str>,
    otel_namespace: Option<&str>,
) -> anyhow::Result<TracingGuard> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(true).json();

    let provider = match otel_endpoint {
        Some(endpoint) => {
            let exporter = SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .build()
                .map_err(|e| anyhow::anyhow!("OTLP exporter init failed: {e}"))?;

            let mut resource_builder =
                Resource::builder().with_attribute(KeyValue::new("service.name", service_name));
            if let Some(ns) = otel_namespace {
                resource_builder = resource_builder
                    .with_attribute(KeyValue::new("service.namespace", ns.to_string()));
            }

            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource_builder.build())
                .build();

            let tracer = provider.tracer(service_name);
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(otel_layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing subscriber: {e}"))?;

            Some(provider)
        }
        None => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("failed to init tracing subscriber: {e}"))?;
            None
        }
    };

    info!(
        service = service_name,
        otlp_enabled = provider.is_some(),
        "tracing initialized"
    );
    Ok(TracingGuard { provider })
}
