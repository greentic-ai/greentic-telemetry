#![cfg(feature = "otlp")]

use greentic_telemetry::{OtlpConfig, init_otlp};

#[tokio::test(flavor = "current_thread")]
async fn otlp_pipeline_initializes() {
    let cfg = OtlpConfig {
        service_name: "greentic-telemetry-test".into(),
        endpoint: Some("http://localhost:4317".into()),
        sampling_rate: Some(1.0),
    };

    init_otlp(cfg, Vec::new()).expect("otlp init succeeds");
}
