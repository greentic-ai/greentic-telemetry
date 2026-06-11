// The cloud exporters + dev-console subscriber need a multi-threaded Tokio runtime — not on wasm32.
#[cfg(all(
    target_arch = "wasm32",
    any(
        feature = "otlp",
        feature = "azure",
        feature = "aws",
        feature = "gcp",
        feature = "dev-console",
    )
))]
compile_error!(
    "greentic-telemetry: features `otlp`, `azure`, `aws`, `gcp`, and `dev-console` require a \
     multi-threaded Tokio runtime and are not supported on wasm32. Build with \
     `--no-default-features` (the `default` feature enables `otlp`) and export telemetry via \
     the wasm guest/host bridge instead."
);

#[cfg(feature = "aws")]
mod aws_sigv4_client;
#[cfg(feature = "otlp")]
pub mod client;
pub mod context;
pub mod export;
#[cfg(feature = "otlp")]
pub mod host_bridge;
pub mod init;
pub mod layer;
pub mod operation_subs;
pub mod presets;
pub mod provider;
pub mod redaction;
pub mod rollout;
pub mod secrets;
pub mod state_subs;
pub mod tasklocal;
pub mod testutil;
pub mod wasm_guest;
pub mod wasm_host;

#[cfg(feature = "otlp")]
pub use client::{init, metric, set_trace_id, span};
pub use context::TelemetryCtx;
#[cfg(feature = "otlp")]
pub use host_bridge::{HostContext, emit_span as emit_host_span};
pub use init::{
    TelemetryConfig, init_telemetry, init_telemetry_auto, init_telemetry_from_config, shutdown,
};
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
pub use layer::{annotate_current_span, annotate_span};
pub use layer::{layer_from_task_local, layer_with_provider};
pub use operation_subs::OperationSubsConfig;
pub use provider::TelemetryProviderConfig;
pub use rollout::{RolloutEvent, emit_rollout_event};
pub use secrets::*;
pub use tasklocal::{set_current_telemetry_ctx, with_current_telemetry_ctx, with_task_local};
