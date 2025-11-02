use crate::ctx::TelemetryCtx;
use std::sync::Arc;
use tracing::{field, Subscriber};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

/// [`Layer`] injecting [`TelemetryCtx`] attributes into spans.
#[derive(Clone)]
pub struct CtxLayer {
    ctx_getter: Arc<dyn Fn() -> TelemetryCtx + Send + Sync>,
}

impl CtxLayer {
    pub fn new<F>(get_ctx: F) -> Self
    where
        F: Fn() -> TelemetryCtx + Send + Sync + 'static,
    {
        Self {
            ctx_getter: Arc::new(get_ctx),
        }
    }

    fn snapshot(&self) -> TelemetryCtx {
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
        if let Some(span_ref) = ctx.span(id) {
            let telemetry = self.snapshot();
            span_ref.extensions_mut().insert(telemetry);
        }
    }

    fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span_ref) = ctx.span(id) {
            let telemetry = span_ref
                .extensions()
                .get::<TelemetryCtx>()
                .cloned()
                .unwrap_or_else(|| {
                    let snapshot = self.snapshot();
                    span_ref.extensions_mut().insert(snapshot.clone());
                    snapshot
                });

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

    for (key, value) in ctx.iter_pairs() {
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

    for (key, value) in ctx.iter_pairs() {
        span.set_attribute(key, value.to_string());
    }
}
