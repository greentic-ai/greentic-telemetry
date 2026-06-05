#![cfg(feature = "otlp")]

//! Verifies the OTLP **logs** pipeline: `tracing` events must be bridged to OTLP
//! log records via `OpenTelemetryTracingBridge` — the mechanism greentic-telemetry
//! composes into its subscriber so consumers (e.g. greentic-runner) export logs,
//! not just traces/metrics.

use greentic_telemetry::export::{ExportConfig, ExportMode};
use greentic_telemetry::{TelemetryConfig, init_telemetry_from_config};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
use tracing_subscriber::prelude::*;

/// The bridge turns `tracing` events into OTLP log records. This is the unit of
/// behaviour the crate's `build_otel_layer()` relies on, tested in isolation
/// (no global subscriber / network) against an in-memory exporter.
#[test]
fn tracing_events_become_otlp_log_records() {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let bridge = OpenTelemetryTracingBridge::new(&provider);
    let subscriber = tracing_subscriber::registry().with(bridge);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(answer = 42, "hello otlp logs");
        tracing::warn!("second record");
    });

    provider.force_flush().expect("force_flush");
    let logs = exporter.get_emitted_logs().expect("emitted logs");
    assert_eq!(
        logs.len(),
        2,
        "expected both tracing events as OTLP log records, got {}",
        logs.len()
    );
}

/// The OTLP HTTP path must also wire the logs pipeline without error. (gRPC is
/// covered by `otlp_smoke.rs`; second inits in this binary would hit the
/// once-guard, so the HTTP variant lives here, alone.)
#[tokio::test(flavor = "current_thread")]
async fn otlp_http_logs_pipeline_initializes() {
    let mut export = ExportConfig::default();
    export.mode = ExportMode::OtlpHttp;
    export.endpoint = Some("http://localhost:4318".into());

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-logs-test".into(),
        },
        export,
    )
    .expect("otlp http init succeeds with logs wired");

    // Exercise the installed bridge end-to-end (drains via the leaked runtime;
    // export fails to connect but must not panic).
    tracing::info!(test = "otlp_http_logs", "log via installed pipeline");
    greentic_telemetry::shutdown();
}
