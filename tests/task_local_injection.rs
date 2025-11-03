use greentic_telemetry::{
    layer_from_task_local, set_current_telemetry_ctx, set_current_tenant_ctx,
    with_current_telemetry_ctx, with_task_local,
};
use greentic_types::{EnvId, TenantCtx, TenantId};
use tracing::{Level, info, span};
use tracing_subscriber::{Registry, layer::SubscriberExt};

#[tokio::test]
async fn task_local_layer_records_context() {
    with_task_local(async {
        let tenant_ctx = TenantCtx::new(EnvId::from("prod"), TenantId::from("acme"));
        set_current_tenant_ctx(tenant_ctx);

        with_current_telemetry_ctx(|base| {
            let enriched = base
                .unwrap_or_default()
                .with_session("sess-123")
                .with_flow("flow-xyz")
                .with_node("node-456")
                .with_provider("messaging.telegram");
            set_current_telemetry_ctx(enriched);
        });

        let (capture_layer, store) = greentic_telemetry::testutil::span_recorder();
        let subscriber = Registry::default()
            .with(layer_from_task_local())
            .with(capture_layer);

        let _guard = tracing::subscriber::set_default(subscriber);

        let span = span!(
            Level::INFO,
            "task-local",
            "gt.tenant" = tracing::field::Empty,
            "gt.session" = tracing::field::Empty,
            "gt.flow" = tracing::field::Empty,
            "gt.node" = tracing::field::Empty,
            "gt.provider" = tracing::field::Empty
        );
        {
            let _entered = span.enter();
            info!("recording task-local telemetry context");
        }
        drop(span);

        let captured = store.lock().expect("capture lock");
        let span = captured
            .iter()
            .find(|span| span.name == "task-local")
            .expect("span captured");

        assert_eq!(span.ctx.tenant.as_deref(), Some("acme"));
        assert_eq!(span.ctx.session.as_deref(), Some("sess-123"));
        assert_eq!(span.ctx.flow.as_deref(), Some("flow-xyz"));
        assert_eq!(span.ctx.node.as_deref(), Some("node-456"));
        assert_eq!(span.ctx.provider.as_deref(), Some("messaging.telegram"));
    })
    .await;
}
