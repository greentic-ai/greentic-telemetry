# Plan: export logs to OTLP from greentic-telemetry

Branch: `feat/otlp-logs-export`
Date: 2026-06-05

## Problem

`greentic-telemetry` installs an OTLP **tracer** provider and **meter** provider
and adds a `tracing-opentelemetry` layer — which turns `tracing` events into
span *events*, NOT OTLP **log records**. There is no `SdkLoggerProvider` and no
`opentelemetry-appender-tracing` bridge anywhere in the crate. Consequences:

- Every consumer that inits via this crate (notably **greentic-runner** through
  the `greentic-types` `telemetry-autoinit` macro) emits traces + metrics to OTLP
  but **no logs**. An OTel Collector's `logs` pipeline (→ Loki) therefore receives
  nothing from those services.
- Meanwhile **greentic-start** rolled its own stack in
  `greentic-start/src/otlp_telemetry.rs` that DOES export logs
  (`SdkLoggerProvider` + `OpenTelemetryTracingBridge`). So today logs reach Loki
  only from greentic-start, under a different `service.name`, which is exactly the
  "Loki missing logs / some appear different" disconnect.

Goal: add an OTLP **logs** pipeline to greentic-telemetry so any consumer of the
crate exports logs uniformly, then let greentic-runner (and eventually
greentic-start) converge on this one implementation.

## Reference implementation

`greentic-start/src/otlp_telemetry.rs` already does precisely this and is the
template:
- deps: `opentelemetry = { features = ["logs"] }`, `opentelemetry-appender-tracing`,
  `opentelemetry-otlp = { features = [..., "logs"] }`, `opentelemetry_sdk = { features = [..., "logs"] }`.
- builds `LogExporter` (grpc/http) → `SdkLoggerProvider::builder().with_resource().with_batch_exporter().build()`.
- composes the subscriber layer as `tracer_layer.and_then(OpenTelemetryTracingBridge::new(&logger_provider)).boxed()`.

We port that into the crate's existing init flow.

## What we update

### 1. `Cargo.toml`
- Add the `logs` cargo-feature to the OTel deps used by the `otlp` feature:
  - `opentelemetry = { version = "0.31", features = ["trace", "metrics", "logs"], optional = true }`
  - `opentelemetry-otlp = { version = "0.31", features = ["grpc-tonic", "http-proto", "metrics", "logs"], optional = true }`
  - `opentelemetry_sdk = { version = "0.31", features = ["rt-tokio", "metrics", "logs"], optional = true }`
- Add new optional dep: `opentelemetry-appender-tracing = { version = "0.31", optional = true }`.
- Extend the `otlp` feature to pull it in:
  `otlp = ["opentelemetry", "opentelemetry-otlp", "opentelemetry_sdk", "tracing-opentelemetry", "dep:opentelemetry-appender-tracing"]`.
- Scope note: only wire logs for the **OTLP** path (OtlpGrpc/OtlpHttp). The
  vendor cloud exporters (azure App Insights / aws X-Ray / gcp Cloud Trace) keep
  traces/metrics only for now — logs-over-OTLP there is a follow-up.

### 2. `src/init.rs` — add a logs pipeline + bridge layer
All line numbers vs current HEAD (3d3c4d2).

a. **New static** beside the existing providers (init.rs:55-60):
   `static LOGGER_PROVIDER: OnceCell<SdkLoggerProvider> = OnceCell::new();`
   (cfg-gated on `feature = "otlp"`; cloud paths won't set it.)

b. **Centralize OTel layer construction.** Today the boxed OTel layer is built in
   four near-identical spots from `TRACER_PROVIDER` only:
   - `init_otel_subscriber()` init.rs:99-101
   - `init_fmt_layers` dev branch init.rs:155-159
   - `init_fmt_layers` prod-json branch init.rs:188-192
   - `init_fmt_layers` otel-only branch init.rs:221-225

   Replace all four with one helper:
   ```rust
   #[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
   fn build_otel_layer() -> Option<BoxedOtelLayer> {
       use opentelemetry::trace::TracerProvider as _;
       let tracer = TRACER_PROVIDER.get()?.tracer("greentic-telemetry");
       let tracer_layer = tracing_opentelemetry::layer().with_tracer(tracer);
       #[cfg(feature = "otlp")]
       if let Some(logger) = LOGGER_PROVIDER.get() {
           let bridge = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(logger);
           return Some(tracer_layer.and_then(bridge).boxed());
       }
       Some(tracer_layer.boxed())
   }
   ```
   This makes the bridge ride in the SAME reload slot as the tracer layer, so it
   sits under the global `EnvFilter` (RUST_LOG) — logs, spans, and fmt output all
   share one filter. No new subscriber, no second filter.

c. **Wire the log exporter in the two OTLP install paths**, setting
   `LOGGER_PROVIDER` BEFORE the layer is (re)built:

   - `install_otlp_inner` (init.rs:311-338, the `OTEL_EXPORTER_OTLP_ENDPOINT`
     gRPC path): after building tracer+meter, build a gRPC `LogExporter`
     (`LogExporter::builder().with_tonic().with_endpoint(endpoint).build()?`),
     build `SdkLoggerProvider` with the same `resource`, `LOGGER_PROVIDER.set(..)`.
     (Layer is composed later by `init_fmt_layers` via `build_otel_layer`.)

   - `install_otlp_from_export_inner` (init.rs:395-474, the TELEMETRY_EXPORT path,
     handles grpc + http): mirror the existing span/metric grpc-vs-http branch to
     build the `LogExporter` (`.with_http()` vs `.with_tonic()`, same endpoint,
     headers, compression), build `SdkLoggerProvider`, `LOGGER_PROVIDER.set(..)`,
     BEFORE the `init_otel_subscriber()` call at init.rs:469 (which now calls
     `build_otel_layer()` and picks up the logger).

d. **Optional redaction parity.** Spans are wrapped by
   `redaction::wrap_span_exporter`. Decide whether log bodies/attributes need the
   same PII redaction. Simplest first cut: rely on the existing
   `RedactingFormatFields` NOT applying to the appender (it doesn't), and add a
   `wrap_log_exporter` analogue in `redaction.rs` only if we want log-record
   redaction. Recommend: add a thin `LogExporter` wrapper in a follow-up; for v1,
   document that the appender forwards fields verbatim. (Flag for review.)

e. **`shutdown()`** (init.rs:341-348): also flush/shutdown the logger provider:
   `if let Some(p) = LOGGER_PROVIDER.get() { let _ = p.shutdown(); }`.

### 3. Imports / cfg
Add `use opentelemetry_sdk::logs::SdkLoggerProvider;` and
`use opentelemetry_otlp::LogExporter;` under `#[cfg(feature = "otlp")]`.

### 4. Tests — `tests/otlp_smoke.rs`
- Extend `otlp_pipeline_initializes` (or add `otlp_logs_pipeline_initializes`) to
  assert init still succeeds with logs wired, for BOTH OtlpGrpc and OtlpHttp.
- Add a stronger test if feasible: build the provider with an in-memory log
  exporter (`opentelemetry_sdk::testing`/`InMemoryLogExporter` if exposed), emit
  `tracing::info!`, force-flush, assert ≥1 log record captured with the right
  `service.name` resource attr and severity. If the in-memory exporter isn't
  reachable through the public init API, gate this behind a small test-only hook
  or assert via `LOGGER_PROVIDER` being set.
- Keep tests `#![cfg(feature = "otlp")]` like the existing file.

### 5. Docs
- Update `docs/otlp-backends.md`: state that logs are now exported (signal matrix:
  traces ✓ / metrics ✓ / logs ✓ for OTLP; logs ✗ for azure/aws/gcp pending).
- Note RUST_LOG governs log export level (same filter as fmt/spans).

## Downstream / rollout (separate PRs)
1. **greentic-runner**: VERIFIED — **no code change needed.** Chain confirmed:
   - `greentic-runner/crates/greentic-runner/src/main.rs:251`
     `#[greentic_types::telemetry::main(service_name = "greentic-runner")]`
   - → `greentic-types` proc-macro `expand_main` calls
     `greentic_types::telemetry::install_telemetry("greentic-runner")` inside the
     tokio runtime (greentic-types-macros/src/lib.rs:41).
   - → `install_telemetry` calls `greentic_telemetry::init_telemetry_auto`
     (greentic-types src/telemetry/mod.rs:24-27).
   - → `init_telemetry_auto` = `ExportConfig::from_env()` →
     `init_telemetry_from_config` → OtlpGrpc/OtlpHttp →
     `install_otlp_from_export_inner` — **the fn this PR wired with logs.**

   Two deployment shapes, both covered:
   - **In-process runner-host** (greentic-start embeds `greentic-runner-host`):
     telemetry is greentic-start's own `otlp_telemetry.rs`, which ALREADY exports
     logs. Converging it onto this crate (delete the bespoke stack) is the
     follow-up that removes the duplicate + the service.name divergence.
   - **Spawned standalone `greentic-runner` binary** (greentic-start
     `services/runner.rs:62` `Command::new(..).envs(..)` — no `env_clear`, so it
     **inherits greentic-start's environment**): WAS missing logs; now exports
     them on greentic-telemetry 0.5.x once the env carries the OTLP vars.

   Operative env = **`TELEMETRY_EXPORT=otlp-grpc|otlp-http` + `OTLP_ENDPOINT`**
   (the documented demo flow). GOTCHA: the macro path (`init_telemetry_auto`)
   does NOT honour `OTEL_EXPORTER_OTLP_ENDPOINT` alone — only the unused
   `init_telemetry`/`configure_otlp` path reads that var. Recommend a follow-up to
   let `ExportConfig::from_env()` fall back to `OTEL_EXPORTER_OTLP_ENDPOINT` for
   uniformity with greentic-start's resolver.

   Rollout = **version bump only**: publish greentic-telemetry 0.5.5; greentic-
   types (`greentic-telemetry = { version = "0.5" }`, default features → `otlp`
   on) and greentic-runner pick it up on lockfile refresh. No source change.
2. **greentic-start**: converge onto this crate and delete the bespoke
   `otlp_telemetry.rs` (removes the duplicate stack + service.name divergence).
   Larger; do after the runner is confirmed working.
3. greentic-e2e: the file-export-collector test (separate plan) asserts OTLP
   **logs** arrive for both service names — the regression net for this change.

## Verification
- `cargo build --features otlp` and `cargo test --features otlp`.
- Manual: run the greentic-demo `docker/` collector (file or debug exporter),
  `TELEMETRY_EXPORT=otlp-grpc OTLP_ENDPOINT=http://localhost:4317` a binary that
  uses this crate, drive activity, confirm `resourceLogs` appears in the
  collector output (not just `resourceSpans`/`resourceMetrics`).

## Risks / open questions
- **Double signal**: events become both span-events (tracer layer) and log
  records (appender). This is intended OTel semantics and matches greentic-start;
  confirm the collector routes logs→Loki and traces→traces without duplication
  concerns.
- **Runtime**: the log batch processor needs the leaked tokio runtime already
  created in `install_otlp*`; logger build must happen inside that `rt.enter()`
  scope (it will, since it's in the same `_inner` fn). Verify no
  "no reactor running" at first export.
- **Redaction** of log bodies (item 2d) — decide scope for v1.
- **appender-tracing 0.31 ↔ opentelemetry 0.31** version match (greentic-start
  already uses this pairing successfully).
