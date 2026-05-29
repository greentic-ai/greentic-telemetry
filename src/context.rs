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
///
/// `#[non_exhaustive]`: construct via [`TelemetryCtx::new`] + the `with_*`
/// builders, never a struct literal. The field set grows as the rollout model
/// gains attributes; the attribute keeps each addition non-breaking for
/// builder-based construction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TelemetryCtx {
    pub tenant: String,
    /// Team scope within the tenant. Span/log only — unbounded.
    pub team: Option<String>,
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
    /// Pack id the invocation resolved to (C5). Span/log only — unbounded.
    pub pack_id: Option<String>,
    /// Env-pack kind backing the environment (C5, e.g. `greentic.deployer.k8s`).
    /// Span/log only — unbounded.
    pub env_pack_kind: Option<String>,
    /// Rollout generation of the deployment's routing table (C5), as the
    /// decimal string of a `u64` (set via [`with_generation`](Self::with_generation)).
    /// Span/log only — unbounded.
    pub generation: Option<String>,
    /// Messaging endpoint id the inbound activity arrived on (M1.4, e.g.
    /// `teams-legal` vs `teams-accounting`). Span/log only — unbounded.
    pub messaging_endpoint_id: Option<String>,
}

impl TelemetryCtx {
    pub fn new<T: Into<String>>(tenant: T) -> Self {
        Self {
            tenant: tenant.into(),
            ..Self::default()
        }
    }

    pub fn with_team(mut self, v: impl Into<String>) -> Self {
        self.team = Some(v.into());
        self
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

    pub fn with_pack_id(mut self, v: impl Into<String>) -> Self {
        self.pack_id = Some(v.into());
        self
    }

    pub fn with_env_pack_kind(mut self, v: impl Into<String>) -> Self {
        self.env_pack_kind = Some(v.into());
        self
    }

    /// Set the rollout generation. Takes a `u64` (not a string) so every
    /// producer formats it identically — downstream numeric range queries on
    /// `gt.generation` stay sound.
    pub fn with_generation(mut self, v: u64) -> Self {
        self.generation = Some(v.to_string());
        self
    }

    pub fn with_messaging_endpoint_id(mut self, v: impl Into<String>) -> Self {
        self.messaging_endpoint_id = Some(v.into());
        self
    }

    /// Full attribute set for **spans and logs**. High-cardinality IDs are
    /// included here on purpose (traces/logs aren't aggregated).
    ///
    /// The returned shape is a fixed-length array whose length grows in
    /// lockstep with the field set. Consumers MUST iterate it or
    /// slice-coerce it (`for (k, v) in ctx.kv()` / `let kv = ctx.kv();
    /// helper(&kv)`); binding the result to a typed `[(&'static str,
    /// Option<&str>); N]` opts into a per-field-bump break.
    pub fn kv(&self) -> [(&'static str, Option<&str>); 15] {
        [
            ("gt.tenant", Some(self.tenant.as_str())),
            ("gt.team", self.team.as_deref()),
            ("gt.session", self.session.as_deref()),
            ("gt.flow", self.flow.as_deref()),
            ("gt.node", self.node.as_deref()),
            ("gt.provider", self.provider.as_deref()),
            ("gt.env", self.env.as_deref()),
            ("gt.customer_id", self.customer_id.as_deref()),
            ("gt.deployment_id", self.deployment_id.as_deref()),
            ("gt.bundle_id", self.bundle_id.as_deref()),
            ("gt.revision_id", self.revision_id.as_deref()),
            ("gt.pack_id", self.pack_id.as_deref()),
            ("gt.env_pack_kind", self.env_pack_kind.as_deref()),
            ("gt.generation", self.generation.as_deref()),
            (
                "gt.messaging_endpoint_id",
                self.messaging_endpoint_id.as_deref(),
            ),
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
            .with_team("support")
            .with_env("prod-eu")
            .with_customer_id("cust-acme")
            .with_deployment_id("01JTKS")
            .with_bundle_id("customer.support")
            .with_revision_id("01JTKR")
            .with_pack_id("customer.support@1.2.0")
            .with_env_pack_kind("greentic.deployer.k8s")
            .with_generation(3)
            .with_messaging_endpoint_id("teams-legal");
        let kv = ctx.kv();
        let get = |k: &str| kv.iter().find(|(key, _)| *key == k).and_then(|(_, v)| *v);
        assert_eq!(get("gt.tenant"), Some("acme"));
        assert_eq!(get("gt.team"), Some("support"));
        assert_eq!(get("gt.env"), Some("prod-eu"));
        assert_eq!(get("gt.customer_id"), Some("cust-acme"));
        assert_eq!(get("gt.deployment_id"), Some("01JTKS"));
        assert_eq!(get("gt.bundle_id"), Some("customer.support"));
        assert_eq!(get("gt.revision_id"), Some("01JTKR"));
        assert_eq!(get("gt.pack_id"), Some("customer.support@1.2.0"));
        assert_eq!(get("gt.env_pack_kind"), Some("greentic.deployer.k8s"));
        assert_eq!(get("gt.generation"), Some("3"));
        assert_eq!(get("gt.messaging_endpoint_id"), Some("teams-legal"));
    }

    #[test]
    fn kv_omits_unset_ids() {
        let ctx = TelemetryCtx::new("acme");
        let kv = ctx.kv();
        for key in [
            "gt.team",
            "gt.env",
            "gt.customer_id",
            "gt.deployment_id",
            "gt.bundle_id",
            "gt.revision_id",
            "gt.pack_id",
            "gt.env_pack_kind",
            "gt.generation",
            "gt.messaging_endpoint_id",
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
        let ctx = ctx
            .with_team("support")
            .with_pack_id("p@1")
            .with_env_pack_kind("greentic.deployer.k8s")
            .with_generation(3)
            .with_messaging_endpoint_id("teams-legal");
        let attrs = ctx.metric_attrs();
        let keys: Vec<&str> = attrs.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["gt.tenant", "gt.env", "gt.bundle_id"]);
        // The unbounded IDs must never reach metric labels.
        for forbidden in [
            "gt.team",
            "gt.customer_id",
            "gt.deployment_id",
            "gt.revision_id",
            "gt.session",
            "gt.node",
            "gt.pack_id",
            "gt.env_pack_kind",
            "gt.generation",
            "gt.messaging_endpoint_id",
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
