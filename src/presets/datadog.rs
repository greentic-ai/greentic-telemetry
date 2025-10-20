use anyhow::Result;

use super::{PresetConfig, parse_headers_from_env};
use crate::export::ExportMode;

pub fn config() -> Result<PresetConfig> {
    let mut preset = PresetConfig::default();
    preset.export_mode = Some(ExportMode::OtlpGrpc);

    let endpoint = std::env::var("OTLP_ENDPOINT").ok();
    preset.otlp_endpoint = match endpoint {
        Some(ep) if !ep.is_empty() => Some(ep),
        _ => Some(String::from("http://datadog-agent:4317")),
    };

    let mut headers = parse_headers_from_env(std::env::var("OTLP_HEADERS").ok())?;
    if let Ok(api_key) = std::env::var("DD_API_KEY") {
        if !api_key.is_empty() && !headers.contains_key("DD_API_KEY") {
            headers.insert("DD_API_KEY".into(), api_key);
        }
    }
    preset.otlp_headers = headers;

    Ok(preset)
}
