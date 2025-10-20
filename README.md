# greentic-telemetry

Structured JSON logging helpers built on top of `tracing` for Greentic services.

## Quickstart

```rust
use greentic_telemetry::{init, set_context, shutdown, CloudCtx, TelemetryInit, prelude::*};

fn main() -> anyhow::Result<()> {
    init(
        TelemetryInit {
            service_name: "example-service",
            service_version: "1.0.0",
            deployment_env: "staging",
        },
        &["tenant", "team", "flow", "run_id"],
    )?;

    set_context(CloudCtx {
        tenant: Some("tenant-42"),
        team: Some("growth"),
        flow: Some("onboarding"),
        run_id: Some("run-17"),
    });

    info!("service booted");

    shutdown(); // flush OTLP batches before exiting

    Ok(())
}
```

Run the included example to view JSON output:

```bash
cargo run --example stdout
```

## Configuration

Control exporters and sampling via environment variables:

- `TELEMETRY_EXPORT`: `json-stdout` (default), `otlp-grpc`, or `otlp-http`.
- `OTLP_ENDPOINT`: collector URL (e.g. `http://otel-collector:4317` or `http://collector:4318`).
- `OTLP_HEADERS`: comma-separated `key=value` pairs forwarded to the collector.
- `TELEMETRY_SAMPLING`: `parent` (default) or `traceidratio:<ratio>` (e.g. `traceidratio:0.1`).

When OTLP configuration fails, the crate logs a warning once and keeps emitting JSON to stdout (if enabled).

## Context Propagation

Use `inject_carrier` / `extract_carrier` to round-trip span context and the Greentic cloud IDs across message boundaries:

```rust
struct Headers(HashMap<String, String>);

impl greentic_telemetry::Carrier for Headers {
    fn set(&mut self, key: &str, value: String) { self.0.insert(key.into(), value); }
    fn get(&self, key: &str) -> Option<String> { self.0.get(key).cloned() }
}

let mut headers = Headers(HashMap::new());

let span = tracing::info_span!("publish");
let _guard = span.enter();
greentic_telemetry::inject_carrier(&mut headers);

// Later, on the consumer side:
let span = tracing::info_span!("handle");
let _guard = span.enter();
greentic_telemetry::extract_carrier(&headers);
```

`inject_carrier` emits W3C `traceparent` / `tracestate` headers and the `x-tenant`, `x-team`, `x-flow`, `x-run-id` identifiers. `extract_carrier` restores the span parentage and rehydrates the context so subsequent logs include the inherited IDs.

## Cloud Presets

Set `CLOUD_PRESET` for quick-start wiring. Presets only prefill defaults—you can still override env vars manually.

| Preset | Default `OTLP_ENDPOINT` | Notes |
| --- | --- | --- |
| `aws` | `http://aws-otel-collector:4317` | Targets AWS Distro for OpenTelemetry collector.
| `gcp` | `http://otc-collector:4317` | Example for Google Ops Agent’s OTLP receiver.
| `azure` | `http://otel-collector-azure:4317` | Collector forwarding to Azure Monitor exporter.
| `datadog` | `http://datadog-agent:4317` | If `DD_API_KEY` present, auto-inserts `OTLP_HEADERS=DD_API_KEY=...`.
| `loki` | N/A | Keeps `json-stdout`; ship through Vector/Grafana Agent for Loki/Tempo.

`TELEMETRY_EXPORT` remains respected. If unset, presets select `otlp-grpc` (except `loki`, which leaves JSON stdout).

### Collector snippets

AWS ADOT sidecar (logs/traces):

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317
exporters:
  awsxray:
    local_mode: true
  awscloudwatchlogs:
    log_group_name: /greentic/services
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [awsxray]
    logs:
      receivers: [otlp]
      exporters: [awscloudwatchlogs]
```

GCP Ops Agent OTLP collector (forward to Cloud Trace / Logging):

```yaml
receivers:
  otlp:
    protocols:
      grpc:
exporters:
  googlecloud:
    project: ${PROJECT_ID}
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [googlecloud]
    logs:
      receivers: [otlp]
      exporters: [googlecloud]
```

Azure Monitor exporter via standalone collector:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
exporters:
  azuremonitor:
    instrumentation_key: ${APP_INSIGHTS_KEY}
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [azuremonitor]
    logs:
      receivers: [otlp]
      exporters: [azuremonitor]
```

Datadog agent OTLP:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
exporters:
  otlphttp:
    endpoint: https://api.datadoghq.com
    headers:
      x-api-key: ${DD_API_KEY}
service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [otlphttp]
    logs:
      receivers: [otlp]
      exporters: [otlphttp]
```

Loki + Tempo via Vector:

```yaml
sources:
  otlp_grpc:
    type: otlp
    address: 0.0.0.0:4317
sinks:
  loki:
    type: loki
    inputs: [otlp_grpc]
    endpoint: http://loki:3100
  tempo:
    type: tempo
    inputs: [otlp_grpc]
    endpoint: http://tempo:4317
```

## Metrics

- Counters, gauges, and histograms are exposed via `greentic_telemetry::metrics`.
- When `TELEMETRY_EXPORT` resolves to an OTLP exporter, measurements are forwarded over the same gRPC channel. With `json-stdout`, metrics default to no-ops so instrumentation never needs guard clauses.

```rust
let requests = greentic_telemetry::metrics::counter("service.requests");
let latency = greentic_telemetry::metrics::histogram("service.request.duration_ms");

requests.add(1.0);
latency.record(elapsed_ms);
```

Every data point automatically includes `service.name`, `service.version`, `deployment.environment`, and the active cloud context (`tenant`, `team`, `flow`, `run_id`). If a tracing span is in scope, exemplar hints (`trace_id`, `span_id`) ride along so compatible collectors can correlate metrics back to traces.

## PII Redaction

- Configure `PII_REDACTION_MODE=off|strict|allowlist` to mask sensitive values before they reach collectors.
- `strict` masks common tokens, emails, and phone numbers by default; `allowlist` keeps only the fields in `PII_ALLOWLIST_FIELDS` unchanged.
- Extend masking with `PII_MASK_REGEXES` (comma-separated regexes) to scrub custom patterns.
