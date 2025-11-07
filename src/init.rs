#[cfg(feature = "otlp")]
use once_cell::sync::OnceCell;
use thiserror::Error;
#[cfg(feature = "otlp")]
use tracing_subscriber::{EnvFilter, prelude::*};
use tracing_subscriber::{Registry, layer::Layer};

#[cfg(feature = "otlp")]
static INIT_GUARD: OnceCell<()> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct OtlpConfig {
    pub service_name: String,
    pub endpoint: Option<String>,
    pub sampling_rate: Option<f64>,
}

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

    use opentelemetry::global;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::resource::Resource;
    use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};

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

#[cfg(not(feature = "otlp"))]
pub fn init_otlp(
    cfg: OtlpConfig,
    extra_layers: Vec<Box<dyn Layer<Registry> + Send + Sync>>,
) -> Result<(), TelemetryError> {
    let _ = (cfg, extra_layers);
    Err(TelemetryError::Init("built without `otlp` feature".into()))
}
