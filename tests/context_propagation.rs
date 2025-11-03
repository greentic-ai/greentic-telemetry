use greentic_telemetry::{CtxLayer, TelemetryCtx};
use tracing::{Level, info, span};
use tracing_subscriber::{Registry, layer::SubscriberExt};

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
        .with(CtxLayer(|| Some(build_ctx())))
        .with(capture_layer);

    let _guard = tracing::subscriber::set_default(subscriber);

    let root = span!(
        Level::INFO,
        "node_execute",
        "gt.tenant" = tracing::field::Empty,
        "gt.session" = tracing::field::Empty,
        "gt.flow" = tracing::field::Empty,
        "gt.node" = tracing::field::Empty,
        "gt.provider" = tracing::field::Empty
    );
    {
        let _root_enter = root.enter();
        let child = span!(
            Level::DEBUG,
            "tool_call",
            tool = "embedding",
            "gt.node" = tracing::field::Empty
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
        assert_eq!(span.ctx.tenant.as_deref(), Some("acme"));
        assert_eq!(span.ctx.session.as_deref(), Some("sess-123"));
        assert_eq!(span.ctx.flow.as_deref(), Some("flow-xyz"));
        assert_eq!(span.ctx.provider.as_deref(), Some("messaging.telegram"));
        assert_eq!(span.ctx.node.as_deref(), Some("qa-1"));
    }
}
