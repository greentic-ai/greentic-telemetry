# greentic-telemetry

Tenant-aware telemetry utilities for Greentic services built on top of [`tracing`], [`opentelemetry`], and the shared [`greentic-types`] domain crate.

[`tracing`]: https://github.com/tokio-rs/tracing
[`opentelemetry`]: https://opentelemetry.io/
[`greentic-types`]: https://github.com/greentic-ai/greentic-types

## Highlights

- `TelemetryCtx`: structured context carrying `{tenant, team, user, session, flow, node, provider}`.
- `CtxLayer`: a `tracing` layer that injects `TelemetryCtx` fields into spans and OTLP attributes.
- `init_otlp`: opinionated OTLP pipeline wiring with fmt logging + OpenTelemetry exporter.
- Helpers for integration tests (`testutil::span_recorder`) and Elastic/Kibana developer bundle.
- Existing `init_telemetry` bootstrap preserved for legacy callers.

## Quickstart

```rust
use greentic_telemetry::{CtxLayer, TelemetryCtx, init_otlp, OtlpConfig};
use tracing::{info, span, Level};
use tracing_subscriber::{layer::SubscriberExt, Registry};

fn telemetry_ctx() -> TelemetryCtx {
    TelemetryCtx::default()
        .with_tenant("tenant-acme")
        .with_session("sess-123")
        .with_flow("flow-intake")
        .with_provider("runner")
        .with_node("node-parse")
}

fn main() -> anyhow::Result<()> {
    // Wire OTLP + fmt logging once at startup (Tokio runtime assumed for batches).
    init_otlp(OtlpConfig {
        endpoint: "http://localhost:4317".into(),
        service_name: "greentic-runner".into(),
        insecure: true,
    })?;

    let ctx_layer = CtxLayer::new(telemetry_ctx);
    let subscriber = Registry::default().with(ctx_layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let span = span!(
        Level::INFO,
        "node_execute",
        "greentic.tenant" = tracing::field::Empty,
        "greentic.session" = tracing::field::Empty,
        "greentic.flow" = tracing::field::Empty,
        "greentic.node" = tracing::field::Empty,
        "greentic.provider" = tracing::field::Empty,
    );
    let _enter = span.enter();
    info!("executing node with injected telemetry context");

    Ok(())
}
```

Spans automatically receive the Greentic attributes (as tracing fields and OTLP attributes), ensuring the collector exports `{tenant, session, flow, node, provider}` consistently.

### Bridging `greentic-types`

`TelemetryCtx` implements `From<&greentic_types::TenantCtx>`, `From<&InvocationEnvelope>`, and `From<&telemetry::SpanContext>`, so existing domain payloads can be mapped without manual string conversions:

```rust
use greentic_telemetry::TelemetryCtx;
use greentic_types::{InvocationEnvelope, TenantCtx};

fn span_ctx(env: &InvocationEnvelope) -> TelemetryCtx {
    TelemetryCtx::from(env)
}

fn tenant_ctx(ctx: &TenantCtx) -> TelemetryCtx {
    TelemetryCtx::from(ctx)
}
```

## OTLP wiring

`init_otlp` produces a `tracing` `Dispatch` with:

- `tracing_subscriber::fmt` layer (target + thread info disabled by default)
- `tracing_opentelemetry::layer` connected to an OTLP gRPC exporter
- Resource set with `service.name` from `OtlpConfig`

It installs the `Dispatch` as the global default and returns a clone so callers can reinstall or inspect it. Use `otlp::shutdown()` (or the legacy `shutdown()` re-export) on graceful shutdown to flush spans.

### Legacy bootstrap

`init_telemetry` / `TelemetryConfig` continue to provide the previous logging preset (stdout/file appenders + OTLP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set). New services should prefer `init_otlp` for explicit configuration.

## Testing utilities

`testutil::span_recorder()` returns a `(CaptureLayer, Arc<Mutex<Vec<RecordedSpan>>>)` pair for asserting that spans carry `TelemetryCtx`. See `tests/context_propagation.rs` for an end-to-end example exercising propagation across nested spans.

## Dev Elastic bundle

A ready-to-run Elastic/Kibana/OpenTelemetry Collector stack lives in `dev/elastic-compose/`.

```bash
docker compose -f dev/elastic-compose/docker-compose.yml up -d
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

Then open Kibana at <http://localhost:5601/>. The default collector config writes spans/metrics to stdout for quick inspection—customise `otel-config.yaml` if you want to forward to Elastic APM.

The existing `dev/docker-compose.elastic.yml` + Filebeat setup remains available if you need the legacy log ingestion pipeline.

## Verification

This crate must pass:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

The new context propagation integration test (`tests/context_propagation.rs`) asserts that `CtxLayer` injects the Greentic attributes across spans.
