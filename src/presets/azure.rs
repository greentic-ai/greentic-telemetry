use anyhow::Result;

use super::{PresetConfig, parse_headers_from_env};
use crate::export::ExportMode;

pub fn config() -> Result<PresetConfig> {
    let mut preset = PresetConfig::default();
    preset.export_mode = Some(ExportMode::OtlpGrpc);

    let endpoint = std::env::var("OTLP_ENDPOINT").ok();
    preset.otlp_endpoint = match endpoint {
        Some(ep) if !ep.is_empty() => Some(ep),
        _ => Some(String::from("http://otel-collector-azure:4317")),
    };

    preset.otlp_headers = parse_headers_from_env(std::env::var("OTLP_HEADERS").ok())?;
    Ok(preset)
}
