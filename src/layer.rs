use crate::context::TelemetryCtx;
use crate::tasklocal::with_current_telemetry_ctx;
use std::sync::Arc;
use tracing::Subscriber;
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

#[derive(Clone)]
struct ContextLayer {
    provider: Arc<dyn Fn() -> Option<TelemetryCtx> + Send + Sync>,
}

impl ContextLayer {
    fn new(provider: Arc<dyn Fn() -> Option<TelemetryCtx> + Send + Sync>) -> Self {
        Self { provider }
    }
}

impl<S> Layer<S> for ContextLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        _attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if let Some(span) = ctx.span(id)
            && let Some(tctx) = (self.provider)()
        {
            let _ = span.extensions_mut().replace(tctx);
        }
    }

    fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let telemetry = span
                .extensions()
                .get::<TelemetryCtx>()
                .cloned()
                .or_else(|| (self.provider)());

            if let Some(tctx) = telemetry {
                let current = tracing::Span::current();
                for (key, value) in tctx.kv() {
                    if let Some(v) = value {
                        current.record(key, tracing::field::display(v));
                    }
                }
                let _ = span.extensions_mut().replace(tctx);
            }
        }
    }
}

pub fn layer_from_task_local() -> impl Layer<tracing_subscriber::Registry> + Clone {
    let provider = Arc::new(|| with_current_telemetry_ctx(|ctx| ctx.cloned()));
    ContextLayer::new(provider)
}

pub fn layer_with_provider(
    provider: impl Fn() -> Option<TelemetryCtx> + Send + Sync + 'static,
) -> impl Layer<tracing_subscriber::Registry> + Clone {
    ContextLayer::new(Arc::new(provider))
}
