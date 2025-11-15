use anyhow::Result;
use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::global;
#[cfg(feature = "otlp")]
use opentelemetry_otlp::{MetricExporter, SpanExporter, WithExportConfig};
#[cfg(feature = "otlp")]
use opentelemetry_sdk::{
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    resource::Resource,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
};
#[cfg(feature = "otlp")]
use thiserror::Error;
#[cfg(feature = "dev")]
use tracing_appender::rolling;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp"))]
use tracing_subscriber::EnvFilter;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use tracing_subscriber::fmt;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp"))]
use tracing_subscriber::prelude::*;
#[cfg(feature = "otlp")]
use tracing_subscriber::{Registry, layer::Layer};

static INITED: OnceCell<()> = OnceCell::new();
#[cfg(feature = "otlp")]
static TRACER_PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();
#[cfg(feature = "otlp")]
static METER_PROVIDER: OnceCell<SdkMeterProvider> = OnceCell::new();
#[cfg(feature = "otlp")]
static INIT_GUARD: OnceCell<()> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    /// e.g. "greentic-telemetry" or caller crate name
    pub service_name: String,
}

pub fn init_telemetry(cfg: TelemetryConfig) -> Result<()> {
    if INITED.get().is_some() {
        return Ok(());
    }

    #[cfg(any(feature = "dev", feature = "prod-json"))]
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    #[cfg(feature = "dev")]
    {
        let filter = filter.clone();
        let file_appender = rolling::daily(".dev-logs", format!("{}.log", cfg.service_name));
        let (nb, _guard) = tracing_appender::non_blocking(file_appender);

        let layer_stdout = fmt::layer()
            .with_target(true)
            .pretty()
            .with_ansi(atty::is(atty::Stream::Stdout));
        let layer_file = fmt::layer().with_writer(nb).with_ansi(false).json();

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(layer_stdout)
            .with(layer_file)
            .try_init();
    }

    #[cfg(all(not(feature = "dev"), feature = "prod-json"))]
    {
        let filter = filter;
        let layer_json = fmt::layer()
            .json()
            .with_target(true)
            .with_current_span(true)
            .with_span_list(true);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(layer_json)
            .try_init();
    }

    #[cfg(feature = "dev-console")]
    {
        if std::env::var_os("TOKIO_CONSOLE").is_some()
            && std::panic::catch_unwind(console_subscriber::init).is_err()
        {
            tracing::warn!(
                "dev-console feature enabled but tokio_unstable not set; skipping console subscriber init"
            );
        }
    }

    configure_otlp(&cfg.service_name)?;

    let _ = INITED.set(());
    Ok(())
}

#[cfg(feature = "otlp")]
fn configure_otlp(service_name: &str) -> Result<()> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let resource = Resource::builder()
            .with_service_name(service_name.to_string())
            .build();
        install_otlp(&endpoint, resource)?;
    }

    Ok(())
}

#[cfg(not(feature = "otlp"))]
fn configure_otlp(service_name: &str) -> Result<()> {
    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        tracing::warn!(
            service = %service_name,
            "otlp feature disabled; ignoring OTEL_EXPORTER_OTLP_ENDPOINT"
        );
    }
    Ok(())
}

#[cfg(feature = "otlp")]
fn install_otlp(endpoint: &str, resource: Resource) -> Result<()> {
    let mut span_exporter_builder = SpanExporter::builder().with_tonic();
    span_exporter_builder = span_exporter_builder.with_endpoint(endpoint.to_string());
    let span_exporter = span_exporter_builder.build()?;

    let span_processor = BatchSpanProcessor::builder(span_exporter).build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_span_processor(span_processor)
        .build();
    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    let mut metric_exporter_builder = MetricExporter::builder().with_tonic();
    metric_exporter_builder = metric_exporter_builder.with_endpoint(endpoint.to_string());
    let metric_exporter = metric_exporter_builder.build()?;
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER_PROVIDER.set(meter_provider);

    Ok(())
}

#[cfg(feature = "otlp")]
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
    if let Some(provider) = METER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

#[cfg(not(feature = "otlp"))]
pub fn shutdown() {}

/// ----- Legacy OTLP wiring kept for backwards compatibility -----
#[cfg(feature = "otlp")]
#[derive(Clone, Debug)]
pub struct OtlpConfig {
    pub service_name: String,
    pub endpoint: Option<String>,
    pub sampling_rate: Option<f64>,
}

#[cfg(feature = "otlp")]
#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("init error: {0}")]
    Init(String),
}

#[cfg(feature = "otlp")]
pub fn init_otlp(
    cfg: OtlpConfig,
    extra_layers: Vec<Box<dyn Layer<Registry> + Send + Sync>>,
) -> Result<(), TelemetryError> {
    if INIT_GUARD.get().is_some() {
        return Ok(());
    }

    use opentelemetry_otlp::WithExportConfig;

    let endpoint = cfg
        .endpoint
        .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
        .unwrap_or_else(|| "http://localhost:4317".into());

    let mut exporter_builder = opentelemetry_otlp::SpanExporter::builder().with_tonic();
    exporter_builder = exporter_builder.with_endpoint(endpoint);
    let exporter = exporter_builder
        .build()
        .map_err(|e| TelemetryError::Init(e.to_string()))?;

    let resource = Resource::builder()
        .with_service_name(cfg.service_name)
        .build();

    let sampler = match cfg.sampling_rate.unwrap_or(1.0) {
        x if (0.0..1.0).contains(&x) && x < 1.0 => Sampler::TraceIdRatioBased(x),
        _ => Sampler::AlwaysOn,
    };

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(sampler)
        .with_resource(resource)
        .build();

    use opentelemetry::trace::TracerProvider as _;

    let tracer = provider.tracer("greentic-telemetry");
    global::set_tracer_provider(provider);

    let extra_layer = combine_layers(extra_layers)
        .unwrap_or_else(|| tracing_subscriber::layer::Identity::new().boxed());

    let subscriber = Registry::default().with(extra_layer);

    let subscriber = subscriber
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")));

    let subscriber = subscriber.with(tracing_opentelemetry::layer().with_tracer(tracer));

    #[cfg(feature = "fmt")]
    let subscriber = subscriber.with(if std::env::var("GT_TELEMETRY_FMT").as_deref() == Ok("1") {
        Some(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true),
        )
    } else {
        None
    });

    subscriber
        .try_init()
        .map_err(|e: tracing_subscriber::util::TryInitError| TelemetryError::Init(e.to_string()))?;

    let _ = INIT_GUARD.set(());

    Ok(())
}

#[cfg(feature = "otlp")]
fn combine_layers(
    mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>>,
) -> Option<Box<dyn Layer<Registry> + Send + Sync>> {
    let mut iter = layers.drain(..);
    let mut combined = match iter.next() {
        Some(layer) => layer.boxed(),
        None => return None,
    };

    for layer in iter {
        combined = combined.and_then(layer).boxed();
    }

    Some(combined)
}
