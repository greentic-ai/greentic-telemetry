# Connecting to OpenTelemetry Backends

## Overview

`greentic-telemetry` supports exporting traces, metrics, and logs to any
OpenTelemetry-compatible backend via OTLP gRPC or HTTP. Configuration is
handled through `TelemetryProviderConfig`, which can be set manually or
resolved from a telemetry provider pack (`telemetry-otlp`).

### Signals exported

| Signal | OTLP (grpc/http) | Azure App Insights | AWS X-Ray | GCP Cloud Trace |
|--------|:---:|:---:|:---:|:---:|
| Traces | ✓ | ✓ | ✓ | ✓ |
| Metrics | ✓ | ✓ | ✓ | — |
| Logs | ✓ | — | — | — |

Logs are bridged from `tracing` events to OTLP **log records** via
`opentelemetry-appender-tracing` (`OpenTelemetryTracingBridge`), composed onto
the same subscriber layer as the span tracer. They therefore share the single
global `EnvFilter` — `RUST_LOG` governs trace, span, and log output uniformly.
Log export is wired only for the OTLP modes today; the vendor cloud exporters
(Azure/AWS/GCP) still emit traces/metrics only.

## Export Modes

| Mode | Description | Default Port |
|------|-------------|-------------|
| `otlp-grpc` | OTLP over gRPC (Protobuf) | 4317 |
| `otlp-http` | OTLP over HTTP (Protobuf) | 4318 |
| `json-stdout` | JSON logs to stdout (no collector needed) | — |
| `none` | Alias for `json-stdout` | — |

## Presets

Presets provide default endpoint + headers for common backends.
Set `preset` in config and the library fills in sensible defaults:

| Preset | Mode | Default Endpoint |
|--------|------|-----------------|
| `jaeger` | otlp-grpc | `http://localhost:4317` |
| `zipkin` | otlp-http | `http://localhost:9411` |
| `honeycomb` | otlp-grpc | `https://api.honeycomb.io:443` |
| `datadog` | otlp-grpc | `http://localhost:4317` |
| `newrelic` | otlp-grpc | `https://otlp.nr-data.net:4317` |
| `grafana-tempo` | otlp-grpc | `http://localhost:4317` |
| `elastic` | otlp-grpc | `http://localhost:4317` |
| `aws` | otlp-grpc | `http://localhost:4317` |
| `gcp` | otlp-grpc | `http://localhost:4317` |
| `azure` | otlp-grpc | `http://localhost:4317` |
| `otlp-grpc` | otlp-grpc | `http://localhost:4317` |
| `otlp-http` | otlp-http | `http://localhost:4318` |
| `stdout` | json-stdout | — |

Explicit fields always override preset defaults.

## Headers and Secrets

Auth headers (API keys, tokens) should be sourced from secrets, not stored
in plain config. The validation layer flags headers with sensitive names:

- `authorization`, `api-key`, `x-api-key`
- `x-honeycomb-team`, `dd_api_key`, `dd-api-key`

## TLS / mTLS

For backends requiring client certificates:

```json
{
  "tls_config": {
    "ca_cert_pem": "-----BEGIN CERTIFICATE-----...",
    "client_cert_pem": "-----BEGIN CERTIFICATE-----...",
    "client_key_pem": "-----BEGIN PRIVATE KEY-----..."
  }
}
```

Both `client_cert_pem` and `client_key_pem` must be present together.

## Sampling

| Ratio | Behaviour |
|-------|-----------|
| `0.0` | AlwaysOff — no traces exported |
| `0.0 < r < 1.0` | TraceIdRatio — probabilistic sampling |
| `1.0` | AlwaysOn — all traces exported (default) |

## Compression

Set `"compression": "gzip"` to enable gzip compression on OTLP exports.

---

## Telemetry Initialization Flow

The operator initializes telemetry in two stages:

### Stage 1: Env-var bootstrap (main.rs)

At startup the operator calls `init_telemetry()`. If `OTEL_EXPORTER_OTLP_ENDPOINT`
is set, the full OTel pipeline (traces + metrics) is configured immediately.
Otherwise, only structured logging to stdout/file is active.

```rust
greentic_telemetry::init_telemetry(TelemetryConfig {
    service_name: "greentic-operator".into(),
})
```

### Stage 2: Capability upgrade (demo start)

After the demo bundle is loaded, the operator calls `try_upgrade_telemetry()`:

1. Resolves `greentic.cap.telemetry.v1` from the capability registry
2. Invokes the provider WASM component with op `telemetry.configure`
3. The component reads secrets (endpoint, API key) and state (preset, sampling, etc.)
4. Returns a `TelemetryProviderConfig` JSON payload
5. Operator validates the config (`validate_telemetry_config`)
6. Calls `init_from_provider_config()` to upgrade the OTel SDK
7. Derives `OperationSubsConfig` and stores it on the runner host

When no telemetry pack is deployed, stage 1 config stays active.

---

## Demo Bundle: Full Observability Stack

The `demo-bundle/` directory includes a pre-configured Docker Compose stack
with Jaeger, Grafana Tempo, Prometheus, Grafana, and an OpenTelemetry Collector.

### Architecture

```
greentic-operator
  │
  │  OTLP gRPC (port 4319)
  ▼
┌────────────────────┐
│  OTel Collector     │
│  (fan-out hub)      │
└──┬──────────┬───────┘
   │          │
   ▼          ▼
┌─────────┐ ┌──────────┐     ┌────────────┐
│ Jaeger  │ │  Tempo   │────▶│ Prometheus │
│ :16686  │ │  :3200   │     │  :9090     │
└─────────┘ └──────────┘     └────────────┘
                  │                  │
                  ▼                  ▼
              ┌─────────────────────────┐
              │       Grafana :3001     │
              │  (Tempo + Prometheus +  │
              │   Jaeger datasources)   │
              └─────────────────────────┘
```

### Services

| Service | Image | Ports | Purpose |
|---------|-------|-------|---------|
| **Jaeger** | `jaegertracing/all-in-one` | 4317 (OTLP gRPC), 4318 (OTLP HTTP), 16686 (UI) | Standalone trace backend |
| **Tempo** | `grafana/tempo` | 4320→4317 (OTLP gRPC), 3200 (Query API) | Grafana-native trace store |
| **Prometheus** | `prom/prometheus` | 9090 (UI) | Metrics scrape + query |
| **Grafana** | `grafana/grafana` | 3001→3000 (UI) | Unified dashboards |
| **OTel Collector** | `otel/opentelemetry-collector-contrib` | 4319→4317 (OTLP gRPC), 8889 (metrics) | Trace fan-out + metrics export |
| **Redis** | `redis:7-alpine` | 6379 | State backend |

### Port Map

| Port | Service | Protocol |
|------|---------|----------|
| 4317 | Jaeger (direct OTLP) | gRPC |
| 4318 | Jaeger (direct OTLP) | HTTP |
| 4319 | OTel Collector (unified ingress) | gRPC |
| 4320 | Tempo (direct OTLP) | gRPC |
| 3200 | Tempo Query API | HTTP |
| 8889 | OTel Collector metrics | HTTP (Prometheus scrape) |
| 9090 | Prometheus | HTTP |
| 3001 | Grafana | HTTP |
| 16686 | Jaeger UI | HTTP |
| 6379 | Redis | TCP |

### OTel Collector Pipeline

The collector (`demo-bundle/infra/otel-collector.yaml`) acts as a central
ingress point and fans out to multiple backends simultaneously:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: "0.0.0.0:4317"

exporters:
  otlp/jaeger:            # traces → Jaeger
    endpoint: "jaeger:4317"
    tls: { insecure: true }
  otlp/tempo:             # traces → Tempo
    endpoint: "tempo:4317"
    tls: { insecure: true }
  prometheus:             # metrics → Prometheus scrape endpoint
    endpoint: "0.0.0.0:8889"
    namespace: greentic

processors:
  batch:
    timeout: 5s
    send_batch_size: 1024

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [otlp/jaeger, otlp/tempo]
    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus]
```

### Tempo Auto-Metrics

Tempo (`demo-bundle/infra/tempo.yaml`) generates RED metrics
(Rate/Errors/Duration) from ingested spans automatically:

```yaml
metrics_generator:
  processor:
    service_graphs:
      dimensions: ["greentic.op.name", "greentic.provider.type"]
    span_metrics:
      dimensions: ["greentic.op.name", "greentic.provider.type", "greentic.op.status"]
  storage:
    path: /tmp/tempo/generator
    remote_write:
      - url: http://prometheus:9090/api/v1/write
```

This means you get histograms and counters in Prometheus from traces alone,
without needing explicit metric instrumentation.

### Grafana Datasources

All three backends are pre-provisioned (`demo-bundle/infra/grafana-datasources.yaml`):

| Datasource | Type | URL | Default |
|------------|------|-----|---------|
| Tempo | tempo | `http://tempo:3200` | Yes |
| Prometheus | prometheus | `http://prometheus:9090` | No |
| Jaeger | jaeger | `http://jaeger:16686` | No |

Tempo is configured with `tracesToMetrics` linked to Prometheus, enabling
"jump to metrics" from trace views.

---

## Quick Start: Demo Telemetry Walkthrough

### 1. Start the observability stack

```bash
cd demo-bundle
docker compose up -d
```

Verify all containers are running:

```bash
docker compose ps
```

### 2. Deploy the telemetry pack

Place `telemetry-otlp.gtpack` in the demo bundle providers directory:

```
demo-bundle/
├── providers/
│   └── messaging/
│       ├── telemetry-otlp.gtpack
│       ├── messaging-telegram.gtpack
│       └── ...
```

The telemetry pack contains `setup.yaml` with these configuration questions:

| Question | Default | Description |
|----------|---------|-------------|
| `preset` | `custom` | Backend preset (jaeger/honeycomb/datadog/etc.) |
| `otlp_endpoint` | — | Collector URL (e.g. `http://localhost:4319`) |
| `otlp_api_key` | — | API key (stored as secret) |
| `export_mode` | `otlp-grpc` | Export protocol |
| `sampling_ratio` | `1.0` | Trace sampling (0.0–1.0) |
| `min_log_level` | `info` | Log level filter |
| `exclude_ops` | — | Comma-separated ops to exclude |
| `enable_operation_subs` | `true` | Toggle operation telemetry |
| `include_denied_ops` | `true` | Include denied ops |
| `include_team_in_metrics` | `false` | Team ID in metrics |
| `hash_ids` | `false` | Hash tenant/team IDs |

### 3. Run the operator

**Option A: Use the OTel Collector (recommended)**

Point to the collector on port 4319 — traces go to both Jaeger and Tempo:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4319 \
GREENTIC_ENV=dev \
  gtc op demo start --bundle demo-bundle
```

**Option B: Direct to Jaeger**

Send traces directly to Jaeger on port 4317:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
GREENTIC_ENV=dev \
  gtc op demo start --bundle demo-bundle
```

**Option C: Stdout only (no Docker needed)**

```bash
GREENTIC_ENV=dev gtc op demo start --bundle demo-bundle
```

Spans appear as structured JSON in terminal output.

### 4. Generate traffic

Send a test message to produce traces:

```bash
gtc op demo send \
  --bundle demo-bundle \
  --provider messaging-telegram \
  --to 7951102355 \
  --text "telemetry test"
```

### 5. View traces in Jaeger

1. Open http://localhost:16686
2. Select service `greentic-operator`
3. Click "Find Traces"
4. Open a trace to see the span hierarchy:

```
greentic.op  (root span, ~50ms)
├── [event] operation.requested
├── greentic.op.pack_resolve          (catalog lookup)
├── greentic.op.runner_exec           (flow execution)
│   OR greentic.op.component_invoke   (WASM invocation)
├── [event] operation.completed
└── duration_ms, status recorded
```

5. Click the root span to see events and fields:
   - `greentic.op.name` = `send_payload`
   - `greentic.provider.type` = `messaging.telegram`
   - `greentic.tenant.id` = `demo`
   - `otel.status_code` = `OK` or `ERROR`
   - `greentic.op.duration_ms` = total time in ms

6. Failed traces show `error.type` + `error.message` on the root span.

### 6. View traces in Grafana (Tempo)

1. Open http://localhost:3001
2. Go to Explore → select Tempo datasource
3. Search by service name `greentic-operator`
4. Or use TraceQL: `{ span.greentic.op.name = "send_payload" }`
5. Click "Jump to metrics" to see related Prometheus data

### 7. View metrics in Prometheus

1. Open http://localhost:9090
2. Useful queries:

```promql
# Operation count by provider
greentic_operation_count

# P99 operation duration
histogram_quantile(0.99, rate(greentic_operation_duration_ms_bucket[5m]))

# Error rate
rate(greentic_operation_error_count[5m])

# Tempo-generated span metrics (auto)
traces_spanmetrics_duration_seconds_bucket{span_name="greentic.op"}
```

3. Metric labels:
   - `greentic_op_name` — operation name (e.g. `send_payload`)
   - `greentic_provider_type` — provider type (e.g. `messaging.telegram`)
   - `greentic_op_status` — `ok`, `denied`, or `error`
   - `greentic_tenant_id` — tenant identifier
   - `greentic_op_error_code` — error classification (on error counter)

---

## Example: Stdout Dev

Zero-dependency local development — logs spans as JSON to stdout:

```json
{
  "export_mode": "json-stdout",
  "service_name": "greentic-dev",
  "enable_operation_subs": true,
  "min_log_level": "debug"
}
```

Or with the preset shorthand:

```json
{
  "preset": "stdout",
  "service_name": "greentic-dev",
  "enable_operation_subs": true
}
```

---

## Example: Direct Jaeger (Simplest OTLP Setup)

Single-container setup with just Jaeger:

```bash
docker run -d --name jaeger \
  -e COLLECTOR_OTLP_ENABLED=true \
  -p 4317:4317 \
  -p 16686:16686 \
  jaegertracing/all-in-one:latest
```

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
GREENTIC_ENV=dev \
  gtc op demo start --bundle demo-bundle
```

View: http://localhost:16686 → service `greentic-operator`

---

## Example: Honeycomb (Cloud)

```json
{
  "preset": "honeycomb",
  "headers": { "x-honeycomb-team": "from-secrets" },
  "service_name": "greentic-operator",
  "sampling_ratio": 0.5,
  "enable_operation_subs": true,
  "payload_policy": "hash_only"
}
```

The `x-honeycomb-team` header should come from secrets. The validation
layer will flag this as containing credentials — this is expected and
serves as a reminder to use secret-backed headers in production.

---

## Troubleshooting

### No traces appearing

1. Check `OTEL_EXPORTER_OTLP_ENDPOINT` is set and reachable
2. Verify the telemetry pack is installed: look for
   `telemetry upgraded from capability provider` in operator logs
3. Check Docker containers are running: `docker compose ps`
4. Try the collector health endpoint: `curl http://localhost:8889/metrics`

### Traces in Jaeger but not Tempo (or vice versa)

The OTel Collector fans out to both. If sending directly to one backend
(port 4317 for Jaeger, port 4320 for Tempo), the other won't receive data.
Use port 4319 (collector) for both.

### Metrics not appearing in Prometheus

1. Check Prometheus targets: http://localhost:9090/targets
2. The `otel-collector:8889` target should be UP
3. Verify the collector's prometheus exporter is configured with
   `namespace: greentic`

### High cardinality warnings

If `include_team_in_metrics: true` is enabled, each unique team_id creates
a new metric series. For multi-tenant deployments with many teams, this can
cause cardinality issues. Leave this `false` (default) unless needed.
