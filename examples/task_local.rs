use greentic_telemetry::{
    OtlpConfig, TelemetryCtx, init_otlp, layer_from_task_local, set_current_telemetry_ctx,
    with_task_local,
};

#[tokio::main]
async fn main() {
    with_task_local(async {
        let _ = init_otlp(
            OtlpConfig {
                service_name: "telemetry-demo".into(),
                endpoint: None,
                sampling_rate: Some(1.0),
            },
            vec![Box::new(layer_from_task_local())],
        );

        set_current_telemetry_ctx(
            TelemetryCtx::new("acme")
                .with_session("s1")
                .with_flow("onboard")
                .with_node("qa-1"),
        );

        tracing::info!("hello with tenant-aware fields");
    })
    .await;
}
