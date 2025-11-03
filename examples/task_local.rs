use greentic_telemetry::{
    OtlpConfig, init_otlp, layer_from_task_local, set_current_telemetry_ctx,
    set_current_tenant_ctx, with_current_telemetry_ctx, with_task_local,
};
use greentic_types::{EnvId, TenantCtx, TenantId};
use tracing::{Level, info, span};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    with_task_local(async {
        set_current_tenant_ctx(TenantCtx::new(EnvId::from("prod"), TenantId::from("acme")));

        with_current_telemetry_ctx(|base| {
            let enriched = base
                .unwrap_or_default()
                .with_session("sess-123")
                .with_flow("demo-flow")
                .with_node("node-42")
                .with_provider("messaging.telegram");
            set_current_telemetry_ctx(enriched);
        });

        init_otlp(
            OtlpConfig {
                endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                    .unwrap_or_else(|_| "http://localhost:4317".into()),
                service_name: "greentic-telemetry-demo".into(),
                insecure: true,
            },
            vec![Box::new(layer_from_task_local())],
        )?;

        let span = span!(
            Level::INFO,
            "task-local-demo",
            "gt.tenant" = tracing::field::Empty,
            "gt.session" = tracing::field::Empty,
            "gt.flow" = tracing::field::Empty,
            "gt.node" = tracing::field::Empty,
            "gt.provider" = tracing::field::Empty
        );
        let _entered = span.enter();
        info!("hello from task-local example");

        greentic_telemetry::shutdown();
        Ok(())
    })
    .await
}
