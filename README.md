# greentic-telemetry

Tenant-aware telemetry utilities for Greentic services built on top of [`tracing`], [`opentelemetry`], and the shared [`greentic-types`] domain crate.

[`tracing`]: https://github.com/tokio-rs/tracing
[`opentelemetry`]: https://opentelemetry.io/
[`greentic-types`]: https://github.com/greentic-ai/greentic-types

## Highlights

- `TelemetryCtx`: lightweight context carrying `{tenant, session, flow, node, provider}`.
- `layer_from_task_local`: grab the context from a Tokio task-local without wiring closures.
- `CtxLayer` (`layer_with`): legacy closure-based path kept for backwards compatibility.
- `init_otlp`: install an OTLP pipeline (with optional `fmt` layer) and flush on shutdown.
- Utilities for integration testing (`testutil::span_recorder`) and task-local helpers.

## Quickstart

```rust
use greentic_telemetry::{
    init_otlp, layer_from_task_local, set_current_tenant_ctx, set_current_telemetry_ctx,
    with_current_telemetry_ctx, with_task_local, OtlpConfig,
};
use greentic_types::{EnvId, TenantCtx, TenantId};
use tracing::{info, span, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    with_task_local(async {
        set_current_tenant_ctx(TenantCtx::new(
            EnvId::from("prod"),
            TenantId::from("acme"),
        ));
        with_current_telemetry_ctx(|base| {
            let enriched = base
                .unwrap_or_default()
                .with_session("sess-123")
                .with_flow("flow-intake")
                .with_node("node-parse")
                .with_provider("runner");
            set_current_telemetry_ctx(enriched);
        });

        init_otlp(
            OtlpConfig {
                endpoint: "http://localhost:4317".into(),
                service_name: "greentic-runner".into(),
                insecure: true,
            },
            vec![Box::new(layer_from_task_local())],
        )?;

        let span = span!(
            Level::INFO,
            "node_execute",
            "gt.tenant" = tracing::field::Empty,
            "gt.session" = tracing::field::Empty,
            "gt.flow" = tracing::field::Empty,
            "gt.node" = tracing::field::Empty,
            "gt.provider" = tracing::field::Empty,
        );
        let _enter = span.enter();
        info!("executing node with injected telemetry context");

        greentic_telemetry::shutdown();
        Ok(())
    })
    .await
}
```

Spans automatically receive the Greentic attributes (as tracing fields and OTLP attributes), ensuring the collector exports `{tenant, session, flow, node, provider}` consistently via the task-local path.

### Bridging `greentic-types`

`TelemetryCtx` implements `From<&greentic_types::TenantCtx>`, so existing tenant metadata can be attached without manual string conversions:

```rust
use greentic_telemetry::TelemetryCtx;
use greentic_types::TenantCtx;

fn tenant_ctx(ctx: &TenantCtx) -> TelemetryCtx {
    TelemetryCtx::from(ctx)
}
```

## OTLP wiring

`init_otlp` installs a `tracing` subscriber composed of:

- `tracing_subscriber::fmt` layer (behind the `fmt` feature flag)
- `tracing_opentelemetry::layer` connected to an OTLP gRPC exporter
- `service.name` populated from `OtlpConfig`

The subscriber becomes the global default; call `shutdown()` during graceful shutdown to flush spans.

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
