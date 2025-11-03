/// Tenant-aware telemetry context propagated to spans and exporters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetryCtx {
    pub tenant: String,
    pub session: Option<String>,
    pub flow: Option<String>,
    pub node: Option<String>,
    pub provider: Option<String>,
}

impl TelemetryCtx {
    pub fn new<T: Into<String>>(tenant: T) -> Self {
        Self {
            tenant: tenant.into(),
            ..Self::default()
        }
    }

    pub fn with_session(mut self, v: impl Into<String>) -> Self {
        self.session = Some(v.into());
        self
    }

    pub fn with_flow(mut self, v: impl Into<String>) -> Self {
        self.flow = Some(v.into());
        self
    }

    pub fn with_node(mut self, v: impl Into<String>) -> Self {
        self.node = Some(v.into());
        self
    }

    pub fn with_provider(mut self, v: impl Into<String>) -> Self {
        self.provider = Some(v.into());
        self
    }

    pub fn kv(&self) -> [(&'static str, Option<&str>); 5] {
        [
            ("gt.tenant", Some(self.tenant.as_str())),
            ("gt.session", self.session.as_deref()),
            ("gt.flow", self.flow.as_deref()),
            ("gt.node", self.node.as_deref()),
            ("gt.provider", self.provider.as_deref()),
        ]
    }
}
