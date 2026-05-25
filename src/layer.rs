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
                // Store the context for capture/inspection layers only. Do NOT
                // record gt.* fields here: `Span::record` is a silent no-op for
                // fields the callsite didn't declare, and `Span::current()` is
                // the parent span, not the one being entered — so the old record
                // loop attributed nothing. OTLP attribute export goes through
                // `annotate_span`.
                let _ = span.extensions_mut().replace(tctx);
            }
        }
    }
}

/// Attach a [`TelemetryCtx`]'s attributes to `span` as real OpenTelemetry span
/// attributes (the `gt.*` keys from [`TelemetryCtx::kv`]), so they reach OTLP
/// export regardless of whether the span's callsite declared those fields.
///
/// This is the **export primitive** for context attribution. `Span::record`
/// only affects fields declared at the callsite, and `tracing_opentelemetry`'s
/// `OtelData` builder is private to that crate — so the supported way to attach
/// callsite-unknown attributes is `OpenTelemetrySpanExt::set_attribute` on an
/// owned [`tracing::Span`]. A producer that has a span handle (e.g. the runner
/// creating an invocation span) calls this right after creating the span.
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
pub fn annotate_span(span: &tracing::Span, tctx: &TelemetryCtx) {
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    for (key, value) in tctx.kv() {
        if let Some(v) = value {
            span.set_attribute(key, v.to_string());
        }
    }
}

/// [`annotate_span`] using the task-local [`TelemetryCtx`] (set via
/// `set_current_telemetry_ctx` / the tenant-ctx bridge). No-op when none is set.
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
pub fn annotate_current_span(span: &tracing::Span) {
    match with_current_telemetry_ctx(|ctx| ctx.cloned()) {
        Some(tctx) => annotate_span(span, &tctx),
        // No task-local TelemetryCtx in scope (called outside `with_task_local`
        // or before `set_current_telemetry_ctx`). The span then exports with no
        // `gt.*` attribution — surface it at debug so unattributed spans are
        // diagnosable rather than silently lost.
        None => tracing::debug!(
            "annotate_current_span: no task-local TelemetryCtx in scope; \
             span exported without gt.* attributes"
        ),
    }
}

/// Layer that attaches the task-local [`TelemetryCtx`] to spans (stored in span
/// extensions, and recorded onto any declared `gt.*` fields). For OTLP
/// attribute export of callsite-unknown context, use [`annotate_span`] —
/// `tracing_opentelemetry` 0.32 keeps its span builder private, so a layer
/// cannot inject attributes after the fact.
pub fn layer_from_task_local() -> impl Layer<tracing_subscriber::Registry> + Clone {
    let provider = Arc::new(|| with_current_telemetry_ctx(|ctx| ctx.cloned()));
    ContextLayer::new(provider)
}

/// Like [`layer_from_task_local`] but with a caller-supplied context provider.
pub fn layer_with_provider(
    provider: impl Fn() -> Option<TelemetryCtx> + Send + Sync + 'static,
) -> impl Layer<tracing_subscriber::Registry> + Clone {
    ContextLayer::new(Arc::new(provider))
}
