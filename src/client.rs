use anyhow::Result;
use once_cell::sync::{Lazy, OnceCell};
use opentelemetry::{
    KeyValue, global,
    metrics::Histogram,
    trace::{Span as _, SpanKind, TraceId, Tracer as _, TracerProvider},
};
use opentelemetry_otlp::{MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    resource::Resource,
    trace::{BatchSpanProcessor, SdkTracerProvider},
};
use serde_json::{Map, Value, json};
use std::{collections::HashMap, sync::Mutex};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

static CLIENT_STATE: OnceCell<ClientMode> = OnceCell::new();
static TRACE_ID: Lazy<Mutex<Option<TraceId>>> = Lazy::new(|| Mutex::new(None));
static HISTOGRAMS: Lazy<Mutex<HashMap<String, Histogram<f64>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static CLIENT_TRACER_PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();
static CLIENT_METER_PROVIDER: OnceCell<SdkMeterProvider> = OnceCell::new();

#[derive(Clone, Copy)]
enum ClientMode {
    Otel,
    JsonOnly,
}

/// Initialise the lightweight telemetry client.
///
/// If `otlp_endpoint` is provided, spans and metrics are exported via OTLP.
/// Otherwise, structured JSON logs are emitted to stdout.
pub fn init(otlp_endpoint: Option<&str>) -> Result<()> {
    if CLIENT_STATE.get().is_some() {
        return Ok(());
    }

    let service_name =
        std::env::var("SERVICE_NAME").unwrap_or_else(|_| "greentic-telemetry-client".into());
    let resource = Resource::builder()
        .with_service_name(service_name.clone())
        .build();

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    // Install propagator once.
    global::set_text_map_propagator(TraceContextPropagator::new());

    let mode = if let Some(endpoint) = otlp_endpoint {
        let mut span_exporter_builder = SpanExporter::builder().with_tonic();
        span_exporter_builder = span_exporter_builder.with_endpoint(endpoint.to_string());
        let span_exporter = span_exporter_builder.build()?;

        let span_processor = BatchSpanProcessor::builder(span_exporter).build();
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_span_processor(span_processor)
            .build();
        let tracer = tracer_provider.tracer("greentic-telemetry-client");
        global::set_tracer_provider(tracer_provider.clone());
        let _ = CLIENT_TRACER_PROVIDER.set(tracer_provider);

        let mut metric_exporter_builder = MetricExporter::builder().with_tonic();
        metric_exporter_builder = metric_exporter_builder.with_endpoint(endpoint.to_string());
        let metric_exporter = metric_exporter_builder.build()?;
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource)
            .with_periodic_exporter(metric_exporter)
            .build();
        global::set_meter_provider(meter_provider.clone());
        let _ = CLIENT_METER_PROVIDER.set(meter_provider);

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        let fmt_layer = fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_target(true);

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(telemetry)
            .with(fmt_layer)
            .try_init();

        ClientMode::Otel
    } else {
        let fmt_layer = fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_target(true);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init();
        ClientMode::JsonOnly
    };

    let _ = CLIENT_STATE.set(mode);
    Ok(())
}

/// Record a short-lived span with optional attributes.
pub fn span(name: &str, attrs: &[(&str, &str)]) {
    if CLIENT_STATE.get().is_none() {
        tracing::warn!("greentic telemetry client not initialised; span dropped");
        return;
    }

    let attr_vec: Vec<KeyValue> = attrs
        .iter()
        .map(|(k, v)| KeyValue::new((*k).to_string(), (*v).to_string()))
        .collect();

    match CLIENT_STATE.get().copied().unwrap_or(ClientMode::JsonOnly) {
        ClientMode::Otel => {
            let tracer = global::tracer("greentic-telemetry-client");
            let mut builder = tracer
                .span_builder(name.to_string())
                .with_kind(SpanKind::Internal)
                .with_attributes(attr_vec.clone());

            if let Some(trace_id) = current_trace_id() {
                builder = builder.with_trace_id(trace_id);
            }

            let mut span = tracer.build(builder);
            span.end();
        }
        ClientMode::JsonOnly => {
            let mut attr_map = Map::new();
            for (k, v) in attrs {
                attr_map.insert((*k).to_string(), Value::String((*v).to_string()));
            }
            let payload = json!({ "span": name, "attributes": attr_map });
            tracing::event!(
                target: "greentic.telemetry.span",
                Level::INFO,
                span_name = name,
                payload = %payload
            );
        }
    }
}

/// Record a metric value with optional attributes.
pub fn metric(name: &str, value: f64, attrs: &[(&str, &str)]) {
    if CLIENT_STATE.get().is_none() {
        tracing::warn!("greentic telemetry client not initialised; metric dropped");
        return;
    }

    match CLIENT_STATE.get().copied().unwrap_or(ClientMode::JsonOnly) {
        ClientMode::Otel => {
            let meter = global::meter("greentic-telemetry-client");
            let mut instruments = HISTOGRAMS.lock().expect("histogram lock");
            let histogram = instruments
                .entry(name.to_string())
                .or_insert_with(|| meter.f64_histogram(name.to_string()).build())
                .clone();

            let attr_vec: Vec<KeyValue> = attrs
                .iter()
                .map(|(k, v)| KeyValue::new((*k).to_string(), (*v).to_string()))
                .collect();
            histogram.record(value, &attr_vec);
        }
        ClientMode::JsonOnly => {
            let mut attr_map = Map::new();
            for (k, v) in attrs {
                attr_map.insert((*k).to_string(), Value::String((*v).to_string()));
            }
            let payload = json!({ "metric": name, "value": value, "attributes": attr_map });
            tracing::event!(
                target: "greentic.telemetry.metric",
                Level::INFO,
                metric_name = name,
                metric_value = value,
                payload = %payload
            );
        }
    }
}

/// Pin a trace identifier for subsequent spans.
pub fn set_trace_id(id: &str) {
    let trace_id = TraceId::from_hex(id).ok();
    let mut guard = TRACE_ID.lock().expect("trace id lock");
    *guard = trace_id;
}

fn current_trace_id() -> Option<TraceId> {
    TRACE_ID
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().copied())
}
