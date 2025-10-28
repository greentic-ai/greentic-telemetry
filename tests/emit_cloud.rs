use greentic_telemetry::{init_telemetry, shutdown, TelemetryConfig};
use std::time::Duration;
use tracing::{info, span, Level};

#[tokio::test]
async fn emit_marker_to_cloud() -> anyhow::Result<()> {
    let service = std::env::var("SERVICE_NAME")
        .unwrap_or_else(|_| "greentic-telemetry-ci".into());
    let marker = std::env::var("TEST_MARKER")
        .unwrap_or_else(|_| format!("marker-{}", uuid::Uuid::new_v4()));

    init_telemetry(TelemetryConfig { service_name: service })?;

    let span = span!(Level::INFO, "ci_emit", marker = %marker);
    let _guard = span.enter();
    info!("CI emitting telemetry with marker={}", marker);

    tokio::time::sleep(Duration::from_millis(500)).await;
    shutdown();
    Ok(())
}
