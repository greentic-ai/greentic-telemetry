use greentic_telemetry::{init_telemetry, shutdown, TelemetryConfig};
use tracing::{info, span, Level};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_telemetry(TelemetryConfig {
        service_name: "greentic-telemetry".into(),
    })?;

    let marker = std::env::var("TEST_MARKER").unwrap_or_else(|_| "local-demo".into());
    let span = span!(Level::INFO, "demo", marker = %marker);
    let _guard = span.enter();

    info!("Hello from demo with marker={}", marker);

    // emit one metric for Prometheus dev if needed later
    let meter = opentelemetry::global::meter("greentic-demo");
    let counter = meter
        .u64_counter("demo_requests")
        .with_description("demo counter")
        .build();
    counter.add(1, &[]);

    shutdown();
    Ok(())
}
