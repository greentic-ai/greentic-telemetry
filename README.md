# greentic-telemetry

Tenant-aware telemetry utilities for Greentic services built on top of [`tracing`] and [`opentelemetry`].

[`tracing`]: https://github.com/tokio-rs/tracing
[`opentelemetry`]: https://opentelemetry.io/

## Highlights

- `TelemetryCtx`: lightweight context carrying `{tenant, session, flow, node, provider}`.
- `layer_from_task_local`: grab the context from a Tokio task-local without wiring closures.
- `CtxLayer` (`layer_with`): legacy closure-based path kept for backwards compatibility.
- `init_otlp`: install an OTLP pipeline (with optional `fmt` layer when `GT_TELEMETRY_FMT=1`).
- Utilities for integration testing (`testutil::span_recorder`) and task-local helpers.

## Quickstart

```rust
use greentic_telemetry::{
    init_otlp, layer_from_task_local, set_current_telemetry_ctx, with_task_local, OtlpConfig,
    TelemetryCtx,
};
use tracing::{info, span, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    with_task_local(async {
        set_current_telemetry_ctx(
            TelemetryCtx::new("tenant-acme")
                .with_session("sess-123")
                .with_flow("flow-intake")
                .with_node("node-parse")
                .with_provider("runner"),
        );

        init_otlp(
            OtlpConfig {
                service_name: "greentic-runner".into(),
                endpoint: Some("http://localhost:4317".into()),
                sampling_rate: Some(1.0),
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

        Ok(())
    })
    .await
}
```

Spans automatically receive the Greentic attributes (as tracing fields and OTLP attributes), ensuring the collector exports `{tenant, session, flow, node, provider}` consistently via the task-local path.

## OTLP wiring

`init_otlp` installs a `tracing` subscriber composed of:

- `tracing_subscriber::fmt` layer (behind the `fmt` feature flag)
- `tracing_opentelemetry::layer` connected to an OTLP gRPC exporter
- `service.name` populated from `OtlpConfig`

The subscriber becomes the global default; use `opentelemetry::global::shutdown_tracer_provider()` during graceful shutdown to flush spans.

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

## Local CI checks

Run `ci/local_check.sh` before pushing to mirror the GitHub Actions matrix locally. The script is offline by default; opt in to extra checks via:

- `LOCAL_CHECK_ONLINE=1` — run networked steps (cargo publish dry-run, cloud telemetry loops, schema curls).
- `LOCAL_CHECK_STRICT=1` — treat skipped steps as failures and require every optional tool/env to be present.
- `LOCAL_CHECK_VERBOSE=1` — echo each command for easier debugging.

The generated `.git/hooks/pre-push` hook invokes the script automatically; remove or edit it if you prefer to run the checks manually.
