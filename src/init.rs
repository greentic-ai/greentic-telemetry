use anyhow::Result;
use once_cell::sync::OnceCell;
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry_sdk::{Resource, trace as sdktrace};
#[cfg(feature = "dev")]
use tracing_appender::rolling;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

static INITED: OnceCell<()> = OnceCell::new();

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
        // pretty to stdout + rolling file output
        let file_appender = rolling::daily(".dev-logs", format!("{}.log", cfg.service_name));
        let (nb, _guard) = tracing_appender::non_blocking(file_appender);

        let layer_stdout = fmt::layer()
            .with_target(true)
            .pretty()
            .with_ansi(atty::is(atty::Stream::Stdout));
        let layer_file = fmt::layer().with_writer(nb).with_ansi(false).json();

        tracing_subscriber::registry()
            .with(filter)
            .with(layer_stdout)
            .with(layer_file)
            .init();
    }

    #[cfg(all(not(feature = "dev"), feature = "prod-json"))]
    {
        let filter = filter;
        let layer_json = fmt::layer()
            .json()
            .with_target(true)
            .with_current_span(true)
            .with_span_list(true);
        tracing_subscriber::registry()
            .with(filter)
            .with(layer_json)
            .init();
    }

    #[cfg(feature = "dev-console")]
    {
        console_subscriber::init();
    }

    // ----- tracing (OTLP) -----
    // Respect OTEL_* env; default to no-op if endpoint not set.
    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        let resource = Resource::new([KeyValue::new("service.name", cfg.service_name.clone())]);

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(opentelemetry_otlp::new_exporter().tonic())
            .with_trace_config(sdktrace::Config::default().with_resource(resource))
            .install_batch(opentelemetry_sdk::runtime::Tokio)?;
        let _ = tracer; // global registered by install_batch
    }

    let _ = INITED.set(());
    Ok(())
}

pub fn shutdown() {
    let _ = global::shutdown_tracer_provider();
}
