#![cfg(feature = "otlp")]

use greentic_telemetry::{OtlpConfig, init_otlp, shutdown};

#[tokio::test(flavor = "current_thread")]
async fn otlp_pipeline_initializes() {
    let cfg = OtlpConfig {
        endpoint: "http://localhost:4317".into(),
        service_name: "greentic-telemetry-test".into(),
        insecure: true,
    };

    init_otlp(cfg, Vec::new()).expect("otlp init succeeds");
    shutdown();
}
