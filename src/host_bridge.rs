use crate::client;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

/// Context provided by the host runtime when invoking the telemetry bridge.
#[derive(Debug, Default, Clone)]
pub struct HostContext {
    pub tenant: String,
    pub team: Option<String>,
    pub user: Option<String>,
    pub flow_id: String,
    pub node_id: Option<String>,
    pub connector: Option<String>,
    pub tool: Option<String>,
    pub action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HostSpan<'a> {
    #[serde(default)]
    name: &'a str,
    #[serde(default)]
    attributes: HashMap<&'a str, Value>,
}

/// Handle `telemetry.emit` calls emitted by the host environment.
pub fn emit_span(span_json: &str, ctx: &HostContext) -> Result<()> {
    let parsed: HostSpan<'_> = serde_json::from_str(span_json)
        .with_context(|| format!("invalid span JSON: {span_json}"))?;

    let mut owned: Vec<(String, String)> = Vec::new();
    for (key, value) in parsed.attributes.iter() {
        let val = match value {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        owned.push(((*key).to_string(), val));
    }

    // Standard labels
    owned.push(("tenant".into(), ctx.tenant.clone()));
    owned.push(("flow_id".into(), ctx.flow_id.clone()));
    if let Some(team) = &ctx.team {
        owned.push(("team".into(), team.clone()));
    }
    if let Some(user) = &ctx.user {
        owned.push(("user".into(), user.clone()));
    }
    if let Some(node) = &ctx.node_id {
        owned.push(("node_id".into(), node.clone()));
    }
    if let Some(connector) = &ctx.connector {
        owned.push(("connector".into(), connector.clone()));
    }
    if let Some(tool) = &ctx.tool {
        owned.push(("tool".into(), tool.clone()));
    }
    if let Some(action) = &ctx.action {
        owned.push(("action".into(), action.clone()));
    }

    let owned_refs: Vec<(&str, &str)> = owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let name = if parsed.name.is_empty() {
        "host-span"
    } else {
        parsed.name
    };

    client::span(name, &owned_refs);
    Ok(())
}
