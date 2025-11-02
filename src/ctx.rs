use serde::{Deserialize, Serialize};

/// Tenant-aware telemetry context propagated to spans and exporters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelemetryCtx {
    pub tenant_id: Option<String>,
    pub team_id: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub flow_id: Option<String>,
    pub node_id: Option<String>,
    pub provider: Option<String>,
}

impl TelemetryCtx {
    pub fn with_tenant<T>(mut self, tenant: T) -> Self
    where
        T: Into<String>,
    {
        self.tenant_id = Some(tenant.into());
        self
    }

    pub fn with_team<T>(mut self, team: T) -> Self
    where
        T: Into<String>,
    {
        self.team_id = Some(team.into());
        self
    }

    pub fn with_team_opt<T>(mut self, team: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.team_id = team.map(Into::into);
        self
    }

    pub fn with_user<T>(mut self, user: T) -> Self
    where
        T: Into<String>,
    {
        self.user_id = Some(user.into());
        self
    }

    pub fn with_user_opt<T>(mut self, user: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.user_id = user.map(Into::into);
        self
    }

    pub fn with_session<T>(mut self, session: T) -> Self
    where
        T: Into<String>,
    {
        self.session_id = Some(session.into());
        self
    }

    pub fn with_session_opt<T>(mut self, session: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.session_id = session.map(Into::into);
        self
    }

    pub fn with_flow<T>(mut self, flow: T) -> Self
    where
        T: Into<String>,
    {
        self.flow_id = Some(flow.into());
        self
    }

    pub fn with_flow_opt<T>(mut self, flow: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.flow_id = flow.map(Into::into);
        self
    }

    pub fn with_node<T>(mut self, node: T) -> Self
    where
        T: Into<String>,
    {
        self.node_id = Some(node.into());
        self
    }

    pub fn with_node_opt<T>(mut self, node: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.node_id = node.map(Into::into);
        self
    }

    pub fn with_provider<T>(mut self, provider: T) -> Self
    where
        T: Into<String>,
    {
        self.provider = Some(provider.into());
        self
    }

    pub fn with_provider_opt<T>(mut self, provider: Option<T>) -> Self
    where
        T: Into<String>,
    {
        self.provider = provider.map(Into::into);
        self
    }

    /// Iterator over attribute pairs aligned with Greentic semantic conventions.
    pub fn iter_pairs(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("greentic.tenant", self.tenant_id.as_deref()),
            ("greentic.team", self.team_id.as_deref()),
            ("greentic.user", self.user_id.as_deref()),
            ("greentic.session", self.session_id.as_deref()),
            ("greentic.flow", self.flow_id.as_deref()),
            ("greentic.node", self.node_id.as_deref()),
            ("greentic.provider", self.provider.as_deref()),
        ]
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
    }

    #[cfg(feature = "otlp")]
    pub fn to_otel_attributes(&self) -> Vec<opentelemetry::KeyValue> {
        self.iter_pairs()
            .map(|(key, value)| opentelemetry::KeyValue::new(key, value.to_string()))
            .collect()
    }
}

impl From<&greentic_types::TenantCtx> for TelemetryCtx {
    fn from(ctx: &greentic_types::TenantCtx) -> Self {
        TelemetryCtx::default()
            .with_tenant(ctx.tenant_id.to_string())
            .with_team_opt(ctx.team_id.clone().map(|id| id.to_string()))
            .with_user_opt(ctx.user_id.clone().map(|id| id.to_string()))
    }
}

impl From<&greentic_types::InvocationEnvelope> for TelemetryCtx {
    fn from(envelope: &greentic_types::InvocationEnvelope) -> Self {
        TelemetryCtx::from(&envelope.ctx)
            .with_flow(envelope.flow_id.clone())
            .with_node_opt(envelope.node_id.clone())
    }
}

impl From<&greentic_types::telemetry::SpanContext> for TelemetryCtx {
    fn from(ctx: &greentic_types::telemetry::SpanContext) -> Self {
        let mut telemetry = TelemetryCtx::default()
            .with_tenant(ctx.tenant.to_string())
            .with_flow(ctx.flow_id.clone())
            .with_provider(ctx.provider.clone());

        telemetry = telemetry.with_session_opt(ctx.session_id.clone().map(|id| id.to_string()));
        telemetry.with_node_opt(ctx.node_id.clone())
    }
}
