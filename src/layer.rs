use crate::context::TelemetryCtx;
use crate::tasklocal::with_current_telemetry_ctx;
use std::sync::Arc;
use tracing::{Subscriber, field};
use tracing_subscriber::{
    Registry,
    layer::{Context, Layer},
    registry::LookupSpan,
};

/// [`Layer`] injecting [`TelemetryCtx`] attributes into spans.
#[derive(Clone)]
pub struct CtxLayer {
    ctx_getter: Arc<dyn Fn() -> Option<TelemetryCtx> + Send + Sync>,
}

impl CtxLayer {
    pub fn new<F>(get_ctx: F) -> Self
    where
        F: Fn() -> Option<TelemetryCtx> + Send + Sync + 'static,
    {
        Self {
            ctx_getter: Arc::new(get_ctx),
        }
    }

    fn snapshot(&self) -> Option<TelemetryCtx> {
        (self.ctx_getter)()
    }
}

impl<S> Layer<S> for CtxLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if let Some(span_ref) = ctx.span(id)
            && let Some(telemetry) = self.snapshot()
            && !telemetry.is_empty()
        {
            span_ref.extensions_mut().insert(telemetry);
        }
    }

    fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span_ref) = ctx.span(id) {
            let telemetry = if let Some(existing) = span_ref.extensions().get::<TelemetryCtx>() {
                existing.clone()
            } else if let Some(snapshot) = self.snapshot() {
                if snapshot.is_empty() {
                    return;
                }
                span_ref.extensions_mut().insert(snapshot.clone());
                snapshot
            } else {
                return;
            };

            record_fields(&telemetry);

            #[cfg(feature = "otlp")]
            apply_otel_attributes(&telemetry);
        }
    }
}

fn record_fields(ctx: &TelemetryCtx) {
    let span = tracing::Span::current();
    if span.is_disabled() {
        return;
    }

    for (key, value) in ctx.to_span_kv() {
        span.record(key, field::display(value));
    }
}

#[cfg(feature = "otlp")]
fn apply_otel_attributes(ctx: &TelemetryCtx) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    let span = tracing::Span::current();
    if span.is_disabled() {
        return;
    }

    for (key, value) in ctx.to_span_kv() {
        span.set_attribute(key, value);
    }
}

/// Layer capturing telemetry context from a task-local slot.
pub fn layer_from_task_local() -> impl Layer<Registry> + Send + Sync + 'static {
    layer_with(|| with_current_telemetry_ctx(|ctx| ctx))
}

/// Layer that captures telemetry context via a caller-provided closure.
pub fn layer_with<F>(provider: F) -> CtxLayer
where
    F: Fn() -> Option<TelemetryCtx> + Send + Sync + 'static,
{
    CtxLayer::new(provider)
}
