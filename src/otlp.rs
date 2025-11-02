use anyhow::Result;
use once_cell::sync::OnceCell;
use opentelemetry::{global, trace::TracerProvider, KeyValue};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{trace::SdkTracerProvider, Resource};
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, Registry};

static OTLP_PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();

/// OTLP pipeline configuration for Greentic services.
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    pub endpoint: String,
    pub service_name: String,
    pub insecure: bool,
}

/// Builds and installs a tracing subscriber with an OTLP exporter attached.
pub fn init_otlp(cfg: OtlpConfig) -> Result<tracing::dispatcher::Dispatch> {
    let exporter_builder = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(cfg.endpoint.clone())
        .with_timeout(Duration::from_secs(3));

    let exporter = exporter_builder.build()?;

    let resource = Resource::builder_empty()
        .with_attributes([KeyValue::new("service.name", cfg.service_name.clone())])
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("greentic-telemetry");

    if OTLP_PROVIDER.set(provider.clone()).is_err() {
        tracing::warn!("otlp provider already initialized; skipping overwrite");
    }
    global::set_tracer_provider(provider);

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false);

    let subscriber = Registry::default().with(fmt_layer).with(otel_layer);
    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::set_global_default(dispatch.clone())?;
    Ok(dispatch)
}

/// Flushes any pending OTLP spans.
pub fn shutdown() {
    if let Some(provider) = OTLP_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

pub(crate) fn register_provider(provider: SdkTracerProvider) {
    let _ = OTLP_PROVIDER.set(provider);
}
