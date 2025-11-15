#[cfg(feature = "otlp")]
pub mod client;
pub mod context;
#[cfg(feature = "otlp")]
pub mod host_bridge;
pub mod init;
pub mod layer;
pub mod tasklocal;
pub mod testutil;

#[cfg(feature = "otlp")]
pub use client::{init, metric, set_trace_id, span};
pub use context::TelemetryCtx;
#[cfg(feature = "otlp")]
pub use host_bridge::{HostContext, emit_span as emit_host_span};
#[cfg(feature = "otlp")]
pub use init::{OtlpConfig, TelemetryError, init_otlp};
pub use init::{TelemetryConfig, init_telemetry, shutdown};
pub use layer::{layer_from_task_local, layer_with_provider};
pub use tasklocal::{set_current_telemetry_ctx, with_current_telemetry_ctx, with_task_local};
