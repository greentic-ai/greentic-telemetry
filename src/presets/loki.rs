use anyhow::Result;

use super::PresetConfig;

pub fn config() -> Result<PresetConfig> {
    let mut preset = PresetConfig::default();
    // Loki path prefers logs via stdout; keep exporter as json-stdout unless explicitly overridden.
    // Respect any OTLP env overrides for traces if provided.
    preset.otlp_endpoint = std::env::var("OTLP_ENDPOINT")
        .ok()
        .filter(|s| !s.is_empty());
    Ok(preset)
}
