//! Generic observer-sink bridge for tracing events.
//!
//! `ObserverBridge` is a `tracing_subscriber::Layer` that fans every tracing
//! event out to a set of registered `ObserverSink`s. The sink trait is
//! deliberately generic (one method, owned strings) so this crate doesn't
//! couple to the observer-pack contract — adapters from concrete observer
//! impls (e.g. `greentic-dw-observer::Observer`) live in the consumer.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// One tracing event materialised into a sink-friendly owned shape.
#[derive(Clone, Debug)]
pub struct ObserverEvent {
    pub level: Level,
    pub target: String,
    pub message: String,
    pub timestamp_ms: u64,
    pub fields: BTreeMap<String, String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Trait implemented by anything that wants to receive tracing events.
/// Sync, infallible — the bridge swallows any work the sink wants to do.
pub trait ObserverSink: Send + Sync {
    fn observe(&self, event: &ObserverEvent);
}

/// Holds the list of registered sinks. Cloneable so the same registry can
/// be shared between init code and the tracing layer.
#[derive(Clone, Default)]
pub struct ObserverBridge {
    sinks: Arc<RwLock<Vec<Arc<dyn ObserverSink>>>>,
}

impl ObserverBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a sink. Subsequent tracing events fan out to it.
    pub fn register(&self, sink: Arc<dyn ObserverSink>) {
        if let Ok(mut guard) = self.sinks.write() {
            guard.push(sink);
        }
    }

    /// Returns a `tracing_subscriber::Layer` that emits to every registered sink.
    pub fn layer<S>(&self) -> ObserverBridgeLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        ObserverBridgeLayer {
            sinks: self.sinks.clone(),
        }
    }

    #[cfg(test)]
    fn sink_count(&self) -> usize {
        self.sinks.read().map(|s| s.len()).unwrap_or(0)
    }
}

pub struct ObserverBridgeLayer {
    sinks: Arc<RwLock<Vec<Arc<dyn ObserverSink>>>>,
}

impl<S> Layer<S> for ObserverBridgeLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let Ok(sinks) = self.sinks.read() else {
            return;
        };
        if sinks.is_empty() {
            return;
        }
        let owned = materialise(event);
        for sink in sinks.iter() {
            sink.observe(&owned);
        }
    }
}

fn materialise(event: &tracing::Event<'_>) -> ObserverEvent {
    let meta = event.metadata();
    let mut visitor = FieldVisitor::default();
    event.record(&mut visitor);
    let message = visitor
        .message
        .unwrap_or_else(|| meta.name().to_string());
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    ObserverEvent {
        level: *meta.level(),
        target: meta.target().to_string(),
        message,
        timestamp_ms,
        fields: visitor.fields,
        file: meta.file().map(str::to_string),
        line: meta.line(),
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: BTreeMap<String, String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(rendered);
        } else {
            self.fields.insert(field.name().to_string(), rendered);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(field.name().to_string(), value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tracing::Level;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<ObserverEvent>>,
    }

    impl ObserverSink for RecordingSink {
        fn observe(&self, event: &ObserverEvent) {
            self.events.lock().unwrap().push(event.clone());
        }
    }

    #[test]
    fn bridge_starts_empty() {
        let b = ObserverBridge::new();
        assert_eq!(b.sink_count(), 0);
    }

    #[test]
    fn register_appends_sink() {
        let b = ObserverBridge::new();
        b.register(Arc::new(RecordingSink::default()));
        b.register(Arc::new(RecordingSink::default()));
        assert_eq!(b.sink_count(), 2);
    }

    #[test]
    fn tracing_event_reaches_registered_sinks() {
        let bridge = ObserverBridge::new();
        let sink = Arc::new(RecordingSink::default());
        bridge.register(sink.clone());

        let subscriber = Registry::default().with(bridge.layer::<Registry>());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "test", subject = "world", "hello {}", "world");
        });

        let events = sink.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.level, Level::INFO);
        assert_eq!(event.target, "test");
        assert!(event.message.contains("hello world"));
        assert_eq!(event.fields.get("subject").map(String::as_str), Some("world"));
        assert!(event.timestamp_ms > 0);
    }

    #[test]
    fn no_registered_sink_drops_events_silently() {
        let bridge = ObserverBridge::new();
        let subscriber = Registry::default().with(bridge.layer::<Registry>());
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!("nobody listening");
        });
    }

    #[test]
    fn fan_out_delivers_event_to_every_registered_sink() {
        let bridge = ObserverBridge::new();
        let a = Arc::new(RecordingSink::default());
        let b = Arc::new(RecordingSink::default());
        bridge.register(a.clone());
        bridge.register(b.clone());

        let subscriber = Registry::default().with(bridge.layer::<Registry>());
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!("boom");
        });

        assert_eq!(a.events.lock().unwrap().len(), 1);
        assert_eq!(b.events.lock().unwrap().len(), 1);
    }
}
