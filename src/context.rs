use greentic_types::TenantCtx;
use std::fmt;

/// Tenant-aware telemetry context propagated to spans and exporters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelemetryCtx {
    pub tenant: Option<String>,
    pub session: Option<String>,
    pub flow: Option<String>,
    pub node: Option<String>,
    pub provider: Option<String>,
}

impl TelemetryCtx {
    pub fn with_tenant<T>(mut self, tenant: T) -> Self
    where
        T: Into<String>,
    {
        self.tenant = Some(tenant.into());
        self
    }

    pub fn with_session<T>(mut self, session: T) -> Self
    where
        T: Into<String>,
    {
        self.session = Some(session.into());
        self
    }

    pub fn with_flow<T>(mut self, flow: T) -> Self
    where
        T: Into<String>,
    {
        self.flow = Some(flow.into());
        self
    }

    pub fn with_node<T>(mut self, node: T) -> Self
    where
        T: Into<String>,
    {
        self.node = Some(node.into());
        self
    }

    pub fn with_provider<T>(mut self, provider: T) -> Self
    where
        T: Into<String>,
    {
        self.provider = Some(provider.into());
        self
    }

    /// Returns key/value pairs suitable for recording on tracing spans.
    pub fn to_span_kv(&self) -> Vec<(&'static str, String)> {
        let mut pairs = Vec::with_capacity(5);
        if let Some(value) = &self.tenant {
            pairs.push(("gt.tenant", value.clone()));
        }
        if let Some(value) = &self.session {
            pairs.push(("gt.session", value.clone()));
        }
        if let Some(value) = &self.flow {
            pairs.push(("gt.flow", value.clone()));
        }
        if let Some(value) = &self.node {
            pairs.push(("gt.node", value.clone()));
        }
        if let Some(value) = &self.provider {
            pairs.push(("gt.provider", value.clone()));
        }
        pairs
    }

    pub fn is_empty(&self) -> bool {
        self.tenant.is_none()
            && self.session.is_none()
            && self.flow.is_none()
            && self.node.is_none()
            && self.provider.is_none()
    }
}

impl From<&TenantCtx> for TelemetryCtx {
    fn from(ctx: &TenantCtx) -> Self {
        TelemetryCtx::default().with_tenant(ctx.tenant_id.to_string())
    }
}

impl fmt::Display for TelemetryCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TelemetryCtx(")?;
        let mut first = true;
        for (key, value) in self.to_span_kv() {
            if !first {
                write!(f, ", ")?;
            }
            first = false;
            write!(f, "{}={}", key, value)?;
        }
        write!(f, ")")
    }
}
