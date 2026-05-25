/// Tenant-aware telemetry context propagated to spans and exporters.
///
/// # Cardinality policy (B11)
///
/// The fields split into two tiers by cardinality:
///
/// - **Span/log attributes** (all fields, via [`kv`](Self::kv)): unbounded
///   identifiers such as `deployment_id`, `revision_id`, `customer_id`, and
///   `session` are safe here — traces and logs are not aggregated, so high
///   cardinality costs nothing and full attribution aids debugging/billing.
/// - **Metric labels** (bounded subset, via [`metric_attrs`](Self::metric_attrs)):
///   only `tenant`, `env`, and `bundle_id` — values whose cardinality is
///   bounded by the deployment topology. The unbounded IDs MUST NOT become
///   metric labels (they explode time-series cost); correlate them to metrics
///   through trace exemplars instead. See `docs/correlation-model.md`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetryCtx {
    pub tenant: String,
    pub session: Option<String>,
    pub flow: Option<String>,
    pub node: Option<String>,
    pub provider: Option<String>,
    /// Environment scope (bounded — metric-safe).
    pub env: Option<String>,
    /// Billing principal (P6). Span/log only — unbounded.
    pub customer_id: Option<String>,
    /// `BundleDeployment` id (P6). Span/log only — unbounded.
    pub deployment_id: Option<String>,
    /// Application bundle id (bounded by the env's bundle set — metric-safe).
    pub bundle_id: Option<String>,
    /// Immutable revision id. Span/log only — unbounded.
    pub revision_id: Option<String>,
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

    pub fn with_env(mut self, v: impl Into<String>) -> Self {
        self.env = Some(v.into());
        self
    }

    pub fn with_customer_id(mut self, v: impl Into<String>) -> Self {
        self.customer_id = Some(v.into());
        self
    }

    pub fn with_deployment_id(mut self, v: impl Into<String>) -> Self {
        self.deployment_id = Some(v.into());
        self
    }

    pub fn with_bundle_id(mut self, v: impl Into<String>) -> Self {
        self.bundle_id = Some(v.into());
        self
    }

    pub fn with_revision_id(mut self, v: impl Into<String>) -> Self {
        self.revision_id = Some(v.into());
        self
    }

    /// Full attribute set for **spans and logs**. High-cardinality IDs are
    /// included here on purpose (traces/logs aren't aggregated).
    pub fn kv(&self) -> [(&'static str, Option<&str>); 10] {
        [
            ("gt.tenant", Some(self.tenant.as_str())),
            ("gt.session", self.session.as_deref()),
            ("gt.flow", self.flow.as_deref()),
            ("gt.node", self.node.as_deref()),
            ("gt.provider", self.provider.as_deref()),
            ("gt.env", self.env.as_deref()),
            ("gt.customer_id", self.customer_id.as_deref()),
            ("gt.deployment_id", self.deployment_id.as_deref()),
            ("gt.bundle_id", self.bundle_id.as_deref()),
            ("gt.revision_id", self.revision_id.as_deref()),
        ]
    }

    /// Bounded-cardinality label subset for **metrics** (`{tenant, env,
    /// bundle_id}`). Returns only the present labels. The unbounded IDs
    /// (`customer_id`/`deployment_id`/`revision_id`/`session`/`node`) are
    /// deliberately excluded — feed them to traces/logs via [`kv`](Self::kv),
    /// not to metric label sets.
    pub fn metric_attrs(&self) -> Vec<(&'static str, &str)> {
        let mut attrs: Vec<(&'static str, &str)> = vec![("gt.tenant", self.tenant.as_str())];
        if let Some(env) = self.env.as_deref() {
            attrs.push(("gt.env", env));
        }
        if let Some(bundle) = self.bundle_id.as_deref() {
            attrs.push(("gt.bundle_id", bundle));
        }
        attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_includes_revision_scoped_ids_when_set() {
        let ctx = TelemetryCtx::new("acme")
            .with_env("prod-eu")
            .with_customer_id("cust-acme")
            .with_deployment_id("01JTKS")
            .with_bundle_id("customer.support")
            .with_revision_id("01JTKR");
        let kv = ctx.kv();
        let get = |k: &str| kv.iter().find(|(key, _)| *key == k).and_then(|(_, v)| *v);
        assert_eq!(get("gt.tenant"), Some("acme"));
        assert_eq!(get("gt.env"), Some("prod-eu"));
        assert_eq!(get("gt.customer_id"), Some("cust-acme"));
        assert_eq!(get("gt.deployment_id"), Some("01JTKS"));
        assert_eq!(get("gt.bundle_id"), Some("customer.support"));
        assert_eq!(get("gt.revision_id"), Some("01JTKR"));
    }

    #[test]
    fn kv_omits_unset_ids() {
        let ctx = TelemetryCtx::new("acme");
        let kv = ctx.kv();
        for key in [
            "gt.env",
            "gt.customer_id",
            "gt.deployment_id",
            "gt.bundle_id",
            "gt.revision_id",
        ] {
            let entry = kv.iter().find(|(k, _)| *k == key).expect("key present");
            assert_eq!(entry.1, None, "{key} must be None when unset");
        }
        // tenant is always present.
        assert_eq!(
            kv.iter().find(|(k, _)| *k == "gt.tenant").unwrap().1,
            Some("acme")
        );
    }

    #[test]
    fn metric_attrs_is_the_bounded_subset_only() {
        let ctx = TelemetryCtx::new("acme")
            .with_env("prod-eu")
            .with_customer_id("cust-acme")
            .with_deployment_id("01JTKS")
            .with_bundle_id("customer.support")
            .with_revision_id("01JTKR")
            .with_session("sess-1")
            .with_node("node-7");
        let attrs = ctx.metric_attrs();
        let keys: Vec<&str> = attrs.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["gt.tenant", "gt.env", "gt.bundle_id"]);
        // The unbounded IDs must never reach metric labels.
        for forbidden in [
            "gt.customer_id",
            "gt.deployment_id",
            "gt.revision_id",
            "gt.session",
            "gt.node",
        ] {
            assert!(
                !keys.contains(&forbidden),
                "{forbidden} must not be a metric label"
            );
        }
    }

    #[test]
    fn metric_attrs_drops_absent_optional_labels() {
        // Only tenant present → env/bundle_id absent from the subset.
        let ctx = TelemetryCtx::new("acme");
        assert_eq!(ctx.metric_attrs(), vec![("gt.tenant", "acme")]);
    }
}
