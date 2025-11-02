use crate::ctx::TelemetryCtx;
use std::sync::{Arc, Mutex};
use tracing::Subscriber;
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

#[derive(Debug, Clone)]
pub struct RecordedSpan {
    pub name: &'static str,
    pub ctx: TelemetryCtx,
}

/// Layer capturing closed spans for assertions in tests.
pub struct CaptureLayer {
    spans: Arc<Mutex<Vec<RecordedSpan>>>,
}

impl CaptureLayer {
    pub fn new(store: Arc<Mutex<Vec<RecordedSpan>>>) -> Self {
        Self { spans: store }
    }

    pub fn store(&self) -> Arc<Mutex<Vec<RecordedSpan>>> {
        Arc::clone(&self.spans)
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            if let Some(tctx) = span.extensions().get::<TelemetryCtx>() {
                if let Ok(mut guard) = self.spans.lock() {
                    guard.push(RecordedSpan {
                        name: span.metadata().name(),
                        ctx: tctx.clone(),
                    });
                }
            }
        }
    }
}

/// Returns a capture layer and shared storage to inspect spans in tests.
pub fn span_recorder() -> (CaptureLayer, Arc<Mutex<Vec<RecordedSpan>>>) {
    let storage = Arc::new(Mutex::new(Vec::new()));
    let layer = CaptureLayer::new(Arc::clone(&storage));
    (layer, storage)
}
