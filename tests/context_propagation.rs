use greentic_telemetry::{CtxLayer, TelemetryCtx};
use tracing::{info, span, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

#[test]
fn ctx_is_recorded_on_spans() {
    fn build_ctx() -> TelemetryCtx {
        TelemetryCtx::default()
            .with_tenant("acme")
            .with_session("sess-123")
            .with_flow("flow-xyz")
            .with_provider("messaging.telegram")
            .with_node("qa-1")
    }

    let (capture_layer, store) = greentic_telemetry::testutil::span_recorder();
    let subscriber = Registry::default()
        .with(CtxLayer::new(build_ctx))
        .with(capture_layer);

    let _guard = tracing::subscriber::set_default(subscriber);

    let root = span!(
        Level::INFO,
        "node_execute",
        "greentic.tenant" = tracing::field::Empty,
        "greentic.session" = tracing::field::Empty,
        "greentic.flow" = tracing::field::Empty,
        "greentic.node" = tracing::field::Empty,
        "greentic.provider" = tracing::field::Empty
    );
    {
        let _root_enter = root.enter();
        let child = span!(
            Level::DEBUG,
            "tool_call",
            tool = "embedding",
            "greentic.node" = tracing::field::Empty
        );
        let _child_enter = child.enter();
        info!("testing context propagation");
    }

    drop(root);

    let captured = store.lock().expect("capture lock").clone();
    assert!(
        captured.len() >= 2,
        "expected at least 2 spans to be captured, got {}",
        captured.len()
    );

    for span in &captured {
        assert_eq!(span.ctx.tenant_id.as_deref(), Some("acme"));
        assert_eq!(span.ctx.session_id.as_deref(), Some("sess-123"));
        assert_eq!(span.ctx.flow_id.as_deref(), Some("flow-xyz"));
        assert_eq!(span.ctx.provider.as_deref(), Some("messaging.telegram"));
        assert_eq!(span.ctx.node_id.as_deref(), Some("qa-1"));
    }
}
