use once_cell::sync::OnceCell;
use opentelemetry::{KeyValue, global, trace::TracerProvider};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use std::time::Duration;
use thiserror::Error;
use tracing_subscriber::{Registry, layer::Layer};

static INIT_GUARD: OnceCell<()> = OnceCell::new();
static PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();

/// OTLP pipeline configuration for Greentic services.
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    pub endpoint: String,
    pub service_name: String,
    pub insecure: bool,
}

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("telemetry already initialized")]
    AlreadyInitialized,
    #[error("failed to build OTLP exporter: {0}")]
    Exporter(#[from] opentelemetry_otlp::ExporterBuildError),
    #[error("failed to install tracing subscriber: {0}")]
    SetGlobal(#[from] tracing::subscriber::SetGlobalDefaultError),
}

/// Builds and installs a tracing subscriber with an OTLP exporter attached.
pub fn init_otlp(
    cfg: OtlpConfig,
    extra_layers: Vec<Box<dyn Layer<Registry> + Send + Sync + 'static>>,
) -> Result<(), TelemetryError> {
    if INIT_GUARD.get().is_some() {
        return Err(TelemetryError::AlreadyInitialized);
    }

    let mut exporter_builder = SpanExporter::builder().with_tonic();
    exporter_builder = exporter_builder
        .with_endpoint(cfg.endpoint.clone())
        .with_timeout(Duration::from_secs(3));
    let exporter = exporter_builder.build().map_err(TelemetryError::Exporter)?;

    let resource = Resource::builder_empty()
        .with_attributes([KeyValue::new("service.name", cfg.service_name.clone())])
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("greentic-telemetry");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer).boxed();

    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();

    #[cfg(feature = "fmt")]
    {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_thread_ids(false)
            .with_thread_names(false)
            .boxed();
        layers.push(fmt_layer);
    }

    layers.push(otel_layer);
    layers.extend(extra_layers);

    let mut layer_iter = layers.into_iter();
    let mut combined = layer_iter
        .next()
        .unwrap_or_else(|| tracing_subscriber::layer::Identity::new().boxed());

    for layer in layer_iter {
        combined = combined.and_then(layer).boxed();
    }

    let subscriber = combined.with_subscriber(Registry::default());

    let dispatch = tracing::dispatcher::Dispatch::new(subscriber);
    tracing::dispatcher::set_global_default(dispatch)?;

    global::set_tracer_provider(provider.clone());
    let _ = PROVIDER.set(provider);
    let _ = INIT_GUARD.set(());

    Ok(())
}

/// Flushes any pending OTLP spans.
pub fn shutdown() {
    if let Some(provider) = PROVIDER.get() {
        let _ = provider.shutdown();
    }
}
