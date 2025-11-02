use anyhow::Result;
use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::KeyValue;
#[cfg(feature = "otlp")]
use opentelemetry_sdk::trace::SdkTracerProvider;
#[cfg(feature = "otlp")]
use opentelemetry_sdk::Resource;
#[cfg(feature = "otlp")]
use std::time::Duration;
#[cfg(feature = "dev")]
use tracing_appender::rolling;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

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
        if std::env::var_os("TOKIO_CONSOLE").is_some() {
            if std::panic::catch_unwind(|| console_subscriber::init()).is_err() {
                tracing::warn!(
                    "dev-console feature enabled but tokio_unstable not set; skipping console subscriber init"
                );
            }
        }
    }

    // ----- tracing (OTLP) -----
    // Respect OTEL_* env; default to no-op if endpoint not set.
    #[cfg(feature = "otlp")]
    {
        use opentelemetry::global;
        use opentelemetry_otlp::{SpanExporter, WithExportConfig};

        if let Some(endpoint) = std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT") {
            let endpoint = endpoint.to_string_lossy().into_owned();
            let exporter_builder = SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(Duration::from_secs(3));

            let exporter = exporter_builder.build()?;

            let resource = Resource::builder_empty()
                .with_attributes([KeyValue::new("service.name", cfg.service_name.clone())])
                .build();

            let provider = SdkTracerProvider::builder()
                .with_resource(resource)
                .with_batch_exporter(exporter)
                .build();

            crate::otlp::register_provider(provider.clone());
            global::set_tracer_provider(provider);
        }
    }

    let _ = INITED.set(());
    Ok(())
}

pub fn shutdown() {
    #[cfg(feature = "otlp")]
    crate::otlp::shutdown();
}
