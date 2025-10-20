use crate::export::{ExportConfig, ExportMode};
use anyhow::{Context, Result, anyhow};
use once_cell::sync::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tracing::{Dispatch, Event, Subscriber, dispatcher};
use tracing_log::LogTracer;
use tracing_subscriber::{
    EnvFilter, Registry,
    layer::{Context as LayerContext, Layer, SubscriberExt},
    registry::LookupSpan,
};

#[cfg(feature = "json-stdout")]
use std::collections::BTreeMap;
#[cfg(feature = "json-stdout")]
use std::io::{self, Write};
#[cfg(feature = "json-stdout")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
use opentelemetry_otlp::{self, WithExportConfig};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::TracerProvider;
#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
use opentelemetry_sdk::{
    resource::Resource,
    runtime::Tokio,
    trace::{self, BatchSpanProcessor},
};
#[cfg(feature = "otlp-grpc")]
use tonic::metadata::{AsciiMetadataKey, AsciiMetadataValue, MetadataMap};

const DEFAULT_CONTEXT_KEYS: &[&str] = &["tenant", "team", "flow", "run_id"];

pub struct TelemetryInit {
    pub service_name: &'static str,
    pub service_version: &'static str,
    pub deployment_env: &'static str,
}

pub(crate) static TELEMETRY_STATE: OnceCell<Arc<SharedState>> = OnceCell::new();

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
static OTLP_ACTIVE: OnceCell<()> = OnceCell::new();

pub(crate) struct SharedState {
    pub service_name: &'static str,
    pub service_version: &'static str,
    pub deployment_env: &'static str,
    context_keys: Vec<&'static str>,
    context_lookup: HashSet<&'static str>,
    context_values: RwLock<HashMap<&'static str, String>>,
}

impl SharedState {
    fn new(init: TelemetryInit, ctx_keys: &[&'static str]) -> Self {
        let mut keys: HashSet<&'static str> = DEFAULT_CONTEXT_KEYS.iter().copied().collect();
        keys.extend(ctx_keys.iter().copied());

        let mut ordered: Vec<&'static str> = keys.iter().copied().collect();
        ordered.sort();

        Self {
            service_name: init.service_name,
            service_version: init.service_version,
            deployment_env: init.deployment_env,
            context_lookup: keys,
            context_keys: ordered,
            context_values: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) fn context_snapshot(&self) -> Vec<(&'static str, Option<String>)> {
        let values = self.context_values.read().expect("context lock poisoned");
        self.context_keys
            .iter()
            .map(|key| (*key, values.get(key).cloned()))
            .collect()
    }

    pub(crate) fn set_context_value(&self, key: &'static str, value: Option<&str>) {
        if !self.context_lookup.contains(&key) {
            return;
        }

        let mut values = self.context_values.write().expect("context lock poisoned");

        match value {
            Some(v) => {
                values.insert(key, v.to_owned());
            }
            None => {
                values.remove(key);
            }
        }
    }
}

pub fn init(init: TelemetryInit, ctx_keys: &[&'static str]) -> Result<()> {
    if let Some(existing) = TELEMETRY_STATE.get() {
        if existing.service_name != init.service_name
            || existing.service_version != init.service_version
            || existing.deployment_env != init.deployment_env
        {
            tracing::warn!(
                "telemetry already initialized for service={}, version={}, env={}, ignoring new values",
                existing.service_name,
                existing.service_version,
                existing.deployment_env
            );
        }
        return Ok(());
    }

    let filter = env_filter_from_env()?;

    let mut warnings = Vec::new();
    let requested_config = ExportConfig::from_env().unwrap_or_else(|err| {
        warnings.push(format!(
            "invalid telemetry environment configuration: {err:#}. falling back to json-stdout"
        ));
        ExportConfig::default()
    });
    let mut active_mode = requested_config.mode;

    let state = Arc::new(SharedState::new(init, ctx_keys));
    crate::redaction::init_from_env();

    if let Err(err) = LogTracer::init() {
        warnings.push(format!("log tracer already initialized: {err}"));
    }

    global::set_text_map_propagator(TraceContextPropagator::new());

    #[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
    if matches!(
        requested_config.mode,
        ExportMode::OtlpGrpc | ExportMode::OtlpHttp
    ) && requested_config.endpoint.is_none()
    {
        warnings.push(
            "OTLP_ENDPOINT is required for otlp exporters; falling back to json-stdout".to_string(),
        );
        active_mode = ExportMode::JsonStdout;
    }

    #[cfg(not(any(feature = "otlp-grpc", feature = "otlp-http")))]
    if matches!(
        requested_config.mode,
        ExportMode::OtlpGrpc | ExportMode::OtlpHttp
    ) {
        warnings.push(
            "otlp exporters requested but the crate was compiled without otlp features; falling back to json-stdout"
                .to_string(),
        );
        active_mode = ExportMode::JsonStdout;
    }

    let install_result: Result<(), anyhow::Error> = match active_mode {
        ExportMode::JsonStdout => {
            #[cfg(feature = "json-stdout")]
            {
                install_json(&filter, &state)
            }
            #[cfg(not(feature = "json-stdout"))]
            {
                warnings.push(
                    "json-stdout exporter requested but the crate was compiled without the json-stdout feature; logs will not be emitted"
                        .to_string(),
                );
                install_json(&filter, &state)
            }
        }
        ExportMode::OtlpGrpc | ExportMode::OtlpHttp => {
            #[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
            {
                match install_otlp(&filter, &state, &requested_config) {
                    Ok(()) => {
                        let _ = OTLP_ACTIVE.set(());
                        Ok(())
                    }
                    Err(err) => {
                        warnings.push(format!(
                            "failed to configure OTLP exporter ({requested:?}): {err:#}. falling back to json-stdout",
                            requested = requested_config.mode
                        ));
                        active_mode = ExportMode::JsonStdout;
                        #[cfg(feature = "json-stdout")]
                        {
                            install_json(&filter, &state)
                        }
                        #[cfg(not(feature = "json-stdout"))]
                        {
                            warnings.push(
                                "json-stdout exporter requested but the crate was compiled without the json-stdout feature; logs will not be emitted"
                                    .to_string(),
                            );
                            install_json(&filter, &state)
                        }
                    }
                }
            }
            #[cfg(not(any(feature = "otlp-grpc", feature = "otlp-http")))]
            {
                // this branch is unreachable due to earlier cfg guard, but keep for completeness
                warnings.push(
                    "otlp exporters requested but the crate was compiled without otlp features; falling back to json-stdout"
                        .to_string(),
                );
                active_mode = ExportMode::JsonStdout;
                #[cfg(feature = "json-stdout")]
                {
                    install_json(&filter, &state)
                }
                #[cfg(not(feature = "json-stdout"))]
                {
                    warnings.push(
                        "json-stdout exporter requested but the crate was compiled without the json-stdout feature; logs will not be emitted"
                            .to_string(),
                    );
                    install_json(&filter, &state)
                }
            }
        }
    };

    install_result?;

    TELEMETRY_STATE
        .set(state.clone())
        .map_err(|_| anyhow!("telemetry already initialized"))?;

    for warning in warnings {
        tracing::warn!("{warning}");
    }

    tracing::debug!(
        service.name = state.service_name,
        service.version = state.service_version,
        env = state.deployment_env,
        exporter = ?active_mode,
        "telemetry initialized"
    );

    Ok(())
}

fn env_filter_from_env() -> Result<EnvFilter> {
    if let Ok(spec) = std::env::var("RUST_LOG") {
        EnvFilter::try_new(spec).context("invalid RUST_LOG value")
    } else if let Ok(spec) = std::env::var("LOG_LEVEL") {
        EnvFilter::try_new(spec).context("invalid LOG_LEVEL value")
    } else {
        EnvFilter::try_new("info").context("invalid default log level")
    }
}

pub fn shutdown() {
    #[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
    {
        if OTLP_ACTIVE.get().is_some() {
            global::shutdown_tracer_provider();
        }
    }
}

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
fn install_otlp(
    filter: &EnvFilter,
    state: &Arc<SharedState>,
    config: &ExportConfig,
) -> Result<(), anyhow::Error> {
    let sampler = config.sampling.into_sampler();
    let resource = Resource::new(vec![
        KeyValue::new("service.name", state.service_name.to_string()),
        KeyValue::new("service.version", state.service_version.to_string()),
        KeyValue::new("deployment.environment", state.deployment_env.to_string()),
    ]);

    let trace_config = trace::Config::default()
        .with_sampler(sampler)
        .with_resource(resource.clone());

    #[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
    install_otlp_metrics(&resource, config)?;

    let exporter = match config.mode {
        ExportMode::OtlpGrpc => {
            #[cfg(feature = "otlp-grpc")]
            {
                let mut builder = opentelemetry_otlp::new_exporter().tonic();
                if let Some(endpoint) = &config.endpoint {
                    builder = builder.with_endpoint(endpoint.clone());
                }
                if !config.headers.is_empty() {
                    let metadata = build_metadata_map(&config.headers)?;
                    builder = builder.with_metadata(metadata);
                }
                builder
                    .build_span_exporter()
                    .context("failed to build gRPC OTLP exporter")?
            }
            #[cfg(not(feature = "otlp-grpc"))]
            {
                return Err(anyhow!("otlp-grpc feature not enabled"));
            }
        }
        ExportMode::OtlpHttp => {
            #[cfg(feature = "otlp-http")]
            {
                let mut builder = opentelemetry_otlp::new_exporter().http();
                if let Some(endpoint) = &config.endpoint {
                    builder = builder.with_endpoint(endpoint.clone());
                }
                if !config.headers.is_empty() {
                    builder = builder.with_headers(config.headers.clone());
                }
                builder
                    .build_span_exporter()
                    .context("failed to build HTTP OTLP exporter")?
            }
            #[cfg(not(feature = "otlp-http"))]
            {
                return Err(anyhow!("otlp-http feature not enabled"));
            }
        }
        ExportMode::JsonStdout => {
            return Err(anyhow!("json exporter cannot configure OTLP layer"));
        }
    };

    let processor = BatchSpanProcessor::builder(exporter, Tokio).build();

    let provider = TracerProvider::builder()
        .with_config(trace_config)
        .with_span_processor(processor)
        .build();

    let tracer = provider.tracer(state.service_name);
    global::set_tracer_provider(provider);

    let subscriber = Registry::default()
        .with(filter.clone())
        .with(tracing_opentelemetry::layer().with_tracer(tracer));

    dispatcher::set_global_default(Dispatch::new(subscriber))
        .map_err(|err| anyhow!("failed to install tracing subscriber: {err}"))
}

#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
fn install_otlp_metrics(resource: &Resource, config: &ExportConfig) -> Result<()> {
    match config.mode {
        ExportMode::OtlpGrpc => {
            #[cfg(feature = "otlp-grpc")]
            {
                let mut exporter = opentelemetry_otlp::new_exporter().tonic();
                if let Some(endpoint) = &config.endpoint {
                    exporter = exporter.with_endpoint(endpoint.clone());
                }
                if !config.headers.is_empty() {
                    let metadata = build_metadata_map(&config.headers)?;
                    exporter = exporter.with_metadata(metadata);
                }

                opentelemetry_otlp::new_pipeline()
                    .metrics(Tokio)
                    .with_exporter(exporter)
                    .with_resource(resource.clone())
                    .build()
                    .map(|_| ())
                    .map_err(|err| anyhow!("failed to install OTLP metrics pipeline: {err}"))
            }
            #[cfg(not(feature = "otlp-grpc"))]
            {
                Ok(())
            }
        }
        ExportMode::OtlpHttp => {
            #[cfg(feature = "otlp-http")]
            {
                let mut exporter = opentelemetry_otlp::new_exporter().http();
                if let Some(endpoint) = &config.endpoint {
                    exporter = exporter.with_endpoint(endpoint.clone());
                }
                if !config.headers.is_empty() {
                    exporter = exporter.with_headers(config.headers.clone());
                }

                opentelemetry_otlp::new_pipeline()
                    .metrics(Tokio)
                    .with_exporter(exporter)
                    .with_resource(resource.clone())
                    .build()
                    .map(|_| ())
                    .map_err(|err| anyhow!("failed to install OTLP metrics pipeline: {err}"))
            }
            #[cfg(not(feature = "otlp-http"))]
            {
                Ok(())
            }
        }
        ExportMode::JsonStdout => Ok(()),
    }
}

#[cfg(feature = "otlp-grpc")]
fn build_metadata_map(headers: &HashMap<String, String>) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();
    for (key, value) in headers {
        let parsed_key = key
            .parse::<AsciiMetadataKey>()
            .map_err(|err| anyhow!("invalid OTLP_HEADERS key '{key}': {err}"))?;
        let parsed_value = value
            .parse::<AsciiMetadataValue>()
            .map_err(|err| anyhow!("invalid OTLP_HEADERS value for '{key}': {err}"))?;
        metadata.insert(parsed_key, parsed_value);
    }
    Ok(metadata)
}

#[cfg(feature = "json-stdout")]
fn install_json(filter: &EnvFilter, state: &Arc<SharedState>) -> Result<(), anyhow::Error> {
    let provider = TracerProvider::builder().build();
    let tracer = provider.tracer(state.service_name);
    global::set_tracer_provider(provider);

    let subscriber = Registry::default()
        .with(filter.clone())
        .with(JsonLayer::new(state.clone()))
        .with(tracing_opentelemetry::layer().with_tracer(tracer));
    dispatcher::set_global_default(Dispatch::new(subscriber))
        .map_err(|err| anyhow!("failed to install tracing subscriber: {err}"))
}

#[cfg(not(feature = "json-stdout"))]
fn install_json(filter: &EnvFilter, state: &Arc<SharedState>) -> Result<(), anyhow::Error> {
    let provider = TracerProvider::builder().build();
    let tracer = provider.tracer(state.service_name);
    global::set_tracer_provider(provider);

    let subscriber = Registry::default()
        .with(filter.clone())
        .with(tracing_opentelemetry::layer().with_tracer(tracer));
    dispatcher::set_global_default(Dispatch::new(subscriber))
        .map_err(|err| anyhow!("failed to install tracing subscriber: {err}"))
}

#[cfg(feature = "json-stdout")]
struct JsonLayer {
    state: Arc<SharedState>,
}

#[cfg(feature = "json-stdout")]
impl JsonLayer {
    fn new(state: Arc<SharedState>) -> Self {
        Self { state }
    }
}

#[cfg(feature = "json-stdout")]
impl<S> Layer<S> for JsonLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) {
        let metadata = event.metadata();
        let mut fields = FieldVisitor::default();
        event.record(&mut fields);

        let timestamp = now_timestamp();

        let mut writer = JsonWriter::new();
        writer.field_str("timestamp", &timestamp);
        writer.field_str("level", metadata.level().as_str());
        writer.field_str("target", metadata.target());
        writer.field_str("service.name", self.state.service_name);
        writer.field_str("service.version", self.state.service_version);
        writer.field_str("env", self.state.deployment_env);

        for (key, value) in self.state.context_snapshot() {
            match value {
                Some(val) => {
                    let masked = crate::redaction::redact_field(key, &val);
                    writer.field_str(key, &masked)
                }
                None => writer.field_null(key),
            }
        }

        writer.field_object("fields", &fields.values);

        if let Some(scope) = ctx.event_scope(event) {
            let span_names: Vec<String> = scope
                .from_root()
                .map(|span| span.name().to_string())
                .collect();
            if !span_names.is_empty() {
                writer.field_array("spans", &span_names);
            }
        }

        writer.finish();
        let bytes = writer.into_bytes();
        crate::dev::maybe_capture(&bytes);
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(&bytes);
    }
}

#[cfg(feature = "json-stdout")]
fn now_timestamp() -> String {
    if let Some(fixed) = crate::dev::fixed_timestamp() {
        return fixed;
    }
    let now = SystemTime::now();
    match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let secs = duration.as_secs();
            let millis = duration.subsec_millis();
            format!("{secs}.{millis:03}")
        }
        Err(_) => "0.000".to_string(),
    }
}

#[cfg(feature = "json-stdout")]
#[derive(Default)]
struct FieldVisitor {
    values: BTreeMap<String, FieldValue>,
}

#[cfg(feature = "json-stdout")]
impl tracing::field::Visit for FieldVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.values
            .insert(field.name().to_string(), FieldValue::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.values.insert(
            field.name().to_string(),
            FieldValue::Number(value.to_string()),
        );
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.values.insert(
            field.name().to_string(),
            FieldValue::Number(value.to_string()),
        );
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.values.insert(
            field.name().to_string(),
            FieldValue::Number(value.to_string()),
        );
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let masked = crate::redaction::redact_field(field.name(), value);
        self.values
            .insert(field.name().to_string(), FieldValue::String(masked));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        let masked = crate::redaction::redact_field(field.name(), &value.to_string());
        self.values
            .insert(field.name().to_string(), FieldValue::String(masked));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let masked = crate::redaction::redact_field(field.name(), &format!("{value:?}"));
        self.values
            .insert(field.name().to_string(), FieldValue::String(masked));
    }

    fn record_bytes(&mut self, field: &tracing::field::Field, value: &[u8]) {
        let masked = crate::redaction::redact_field(field.name(), &format!("{value:?}"));
        self.values
            .insert(field.name().to_string(), FieldValue::String(masked));
    }
}

#[cfg(feature = "json-stdout")]
enum FieldValue {
    String(String),
    Number(String),
    Bool(bool),
}

#[cfg(feature = "json-stdout")]
struct JsonWriter {
    buffer: Vec<u8>,
    first: bool,
}

#[cfg(feature = "json-stdout")]
impl JsonWriter {
    fn new() -> Self {
        Self {
            buffer: vec![b'{'],
            first: true,
        }
    }

    fn field_str(&mut self, key: &str, value: &str) {
        self.write_field_prefix(key);
        let _ = write_json_string(&mut self.buffer, value);
    }

    fn field_null(&mut self, key: &str) {
        self.write_field_prefix(key);
        let _ = self.buffer.write_all(b"null");
    }

    fn field_object(&mut self, key: &str, fields: &BTreeMap<String, FieldValue>) {
        self.write_field_prefix(key);
        let _ = self.buffer.write_all(b"{");
        let mut first = true;
        for (field_key, value) in fields {
            if !first {
                let _ = self.buffer.write_all(b",");
            }
            first = false;
            let _ = write_json_string(&mut self.buffer, field_key);
            let _ = self.buffer.write_all(b":");
            match value {
                FieldValue::String(s) => {
                    let _ = write_json_string(&mut self.buffer, s);
                }
                FieldValue::Number(n) => {
                    let _ = self.buffer.write_all(n.as_bytes());
                }
                FieldValue::Bool(b) => {
                    let _ = self.buffer.write_all(if *b { b"true" } else { b"false" });
                }
            }
        }
        let _ = self.buffer.write_all(b"}");
    }

    fn field_array(&mut self, key: &str, values: &[String]) {
        self.write_field_prefix(key);
        let _ = self.buffer.write_all(b"[");
        let mut first = true;
        for value in values {
            if !first {
                let _ = self.buffer.write_all(b",");
            }
            first = false;
            let _ = write_json_string(&mut self.buffer, value);
        }
        let _ = self.buffer.write_all(b"]");
    }

    fn finish(&mut self) {
        let _ = self.buffer.write_all(b"}\n");
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    fn write_field_prefix(&mut self, key: &str) {
        if !self.first {
            let _ = self.buffer.write_all(b",");
        }
        self.first = false;
        let _ = write_json_string(&mut self.buffer, key);
        let _ = self.buffer.write_all(b":");
    }
}

#[cfg(feature = "json-stdout")]
fn write_json_string<W: Write>(writer: &mut W, value: &str) -> io::Result<()> {
    writer.write_all(b"\"")?;
    for ch in value.chars() {
        match ch {
            '"' => writer.write_all(b"\\\"")?,
            '\\' => writer.write_all(b"\\\\")?,
            '\n' => writer.write_all(b"\\n")?,
            '\r' => writer.write_all(b"\\r")?,
            '\t' => writer.write_all(b"\\t")?,
            c if (c as u32) < 0x20 => {
                let escape = format!("\\u{:04x}", c as u32);
                writer.write_all(escape.as_bytes())?;
            }
            c => {
                let mut buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut buf);
                writer.write_all(encoded.as_bytes())?;
            }
        }
    }
    writer.write_all(b"\"")
}
