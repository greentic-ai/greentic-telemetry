//! Rollout lifecycle events (C5).
//!
//! A deployment's rollout moves through a small set of state transitions —
//! a revision is staged, warmed, drained, evicted; a traffic split is applied;
//! a health gate passes or fails. [`emit_rollout_event`] records each
//! transition as a structured telemetry item so operators can trace a rollout
//! end to end and correlate it with the revision/bundle/deployment it touched.
//!
//! Each transition rides on a short-lived `greentic.rollout` span that declares
//! the full `gt.*` field set at its callsite and records the present values, so
//! the attribution reaches **every** subscriber — fmt, json, and OTLP — and
//! survives even when no span exporter is configured or the span is sampled
//! out. The inner `info!` marker keeps the transition visible as a log line.

use crate::TelemetryCtx;

/// A rollout lifecycle transition worth recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolloutEvent {
    /// A revision was added to the deployment's routing table.
    RevisionStaged,
    /// A revision passed its warm/ready health gate and is admittable.
    RevisionWarmed,
    /// A revision was flagged draining (keeps existing sessions, no new ones).
    RevisionDraining,
    /// A revision was removed from the routing table after its drain window.
    RevisionEvicted,
    /// A new per-deployment traffic split was applied.
    TrafficSplitApplied,
    /// A revision health gate passed.
    HealthGatePassed,
    /// A revision health gate failed.
    HealthGateFailed,
}

impl RolloutEvent {
    /// Stable dotted discriminant, used as the `rollout.event` attribute and
    /// for filtering in trace/log backends.
    pub const fn as_str(self) -> &'static str {
        match self {
            RolloutEvent::RevisionStaged => "rollout.revision.staged",
            RolloutEvent::RevisionWarmed => "rollout.revision.warmed",
            RolloutEvent::RevisionDraining => "rollout.revision.draining",
            RolloutEvent::RevisionEvicted => "rollout.revision.evicted",
            RolloutEvent::TrafficSplitApplied => "rollout.traffic_split.applied",
            RolloutEvent::HealthGatePassed => "rollout.health_gate.passed",
            RolloutEvent::HealthGateFailed => "rollout.health_gate.failed",
        }
    }
}

/// Record a rollout lifecycle transition, attributed to `tctx`.
///
/// Emits a `greentic.rollout` span carrying the full present `gt.*` attribution
/// plus a `rollout.event` discriminant, and an `info!` marker inside it. Safe to
/// call with no subscriber installed (then it is a no-op beyond the cheap span
/// construction).
///
/// The `gt.*` fields are declared at the callsite and populated via
/// [`Span::record`](tracing::Span::record), rather than `annotate_span`'s
/// OTLP-only `set_attribute`, so the attribution reaches fmt/json subscribers
/// too — the identifiers operators need during a rollout incident survive even
/// when no OTLP exporter is configured.
///
/// The span and the inner event are both intentional and serve different
/// backends: the span is the structured trace item carrying the full
/// attribution, while the `info!` is the log-visible line (a spans-only `fmt`
/// subscriber emits nothing for a span unless `FmtSpan` events are enabled).
/// Both carry `rollout.event`; in a backend that materializes span-events as
/// separate countable rows, filter on the `greentic.rollout` span to avoid
/// double-counting a single transition.
pub fn emit_rollout_event(event: RolloutEvent, tctx: &TelemetryCtx) {
    let span = tracing::info_span!(
        "greentic.rollout",
        rollout.event = event.as_str(),
        gt.tenant = tracing::field::Empty,
        gt.team = tracing::field::Empty,
        gt.session = tracing::field::Empty,
        gt.flow = tracing::field::Empty,
        gt.node = tracing::field::Empty,
        gt.provider = tracing::field::Empty,
        gt.env = tracing::field::Empty,
        gt.customer_id = tracing::field::Empty,
        gt.deployment_id = tracing::field::Empty,
        gt.bundle_id = tracing::field::Empty,
        gt.revision_id = tracing::field::Empty,
        gt.pack_id = tracing::field::Empty,
        gt.env_pack_kind = tracing::field::Empty,
        gt.generation = tracing::field::Empty,
    );
    for (key, value) in tctx.kv() {
        if let Some(v) = value {
            span.record(key, v);
        }
    }
    let _enter = span.enter();
    tracing::info!(
        rollout.event = event.as_str(),
        "rollout lifecycle transition"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable_and_distinct() {
        let all = [
            RolloutEvent::RevisionStaged,
            RolloutEvent::RevisionWarmed,
            RolloutEvent::RevisionDraining,
            RolloutEvent::RevisionEvicted,
            RolloutEvent::TrafficSplitApplied,
            RolloutEvent::HealthGatePassed,
            RolloutEvent::HealthGateFailed,
        ];
        let strs: Vec<&str> = all.iter().map(|e| e.as_str()).collect();
        // Every discriminant is unique.
        let mut sorted = strs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), all.len(), "discriminants must be distinct");
        // Every discriminant is namespaced under `rollout.`.
        assert!(strs.iter().all(|s| s.starts_with("rollout.")));
    }

    #[test]
    fn emit_does_not_panic_without_subscriber() {
        let ctx = TelemetryCtx::new("acme")
            .with_env("prod-eu")
            .with_deployment_id("01JTKS")
            .with_bundle_id("customer.support")
            .with_revision_id("01JTKR")
            .with_generation(3);
        emit_rollout_event(RolloutEvent::TrafficSplitApplied, &ctx);
        emit_rollout_event(RolloutEvent::HealthGateFailed, &ctx);
    }

    /// Regression for the Codex finding: the rollout attribution must reach
    /// subscribers WITHOUT relying on OTLP span export. A plain registry layer
    /// (no `otlp`/`azure`/`gcp` feature, no task-local bridge) must still see
    /// the full `gt.*` attribution.
    ///
    /// This also guards the kv()/callsite coupling: `emit_rollout_event`
    /// declares each `gt.*` field at the span callsite, and `Span::record`
    /// silently drops any field not declared there. The context below sets
    /// **every** `TelemetryCtx` field, and the assertion requires the captured
    /// `gt.*` set to equal exactly what `kv()` reports — so a field added to
    /// `kv()` without a matching span declaration fails here rather than
    /// vanishing silently from rollout events. When you add a `TelemetryCtx`
    /// field, set it here too.
    #[test]
    fn rollout_fields_survive_on_non_otlp_subscriber() {
        use std::collections::{BTreeMap, BTreeSet};
        use std::sync::{Arc, Mutex};
        use tracing::Subscriber;
        use tracing::field::{Field, Visit};
        use tracing::span::{Attributes, Id, Record};
        use tracing_subscriber::layer::{Context, Layer};
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::registry::LookupSpan;

        #[derive(Default)]
        struct Grab(BTreeMap<String, String>);
        impl Visit for Grab {
            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.insert(field.name().to_string(), value.to_string());
            }
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0
                    .entry(field.name().to_string())
                    .or_insert_with(|| format!("{value:?}"));
            }
        }

        struct Capture(Arc<Mutex<BTreeMap<String, String>>>);
        impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
            fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
                let mut g = Grab::default();
                attrs.record(&mut g);
                self.0.lock().unwrap().extend(g.0);
            }
            fn on_record(&self, _id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
                let mut g = Grab::default();
                values.record(&mut g);
                self.0.lock().unwrap().extend(g.0);
            }
        }

        // Every field populated, so kv() yields all gt.* keys as Some.
        let ctx = TelemetryCtx::new("acme")
            .with_team("support")
            .with_session("sess-1")
            .with_flow("flow-1")
            .with_node("node-7")
            .with_provider("openai")
            .with_env("prod-eu")
            .with_customer_id("cust-acme")
            .with_deployment_id("01JTKS")
            .with_bundle_id("customer.support")
            .with_revision_id("01JTKR")
            .with_pack_id("customer.support@1.2.0")
            .with_env_pack_kind("greentic.deployer.k8s")
            .with_generation(3);

        let store = Arc::new(Mutex::new(BTreeMap::new()));
        let subscriber = tracing_subscriber::registry().with(Capture(Arc::clone(&store)));
        tracing::subscriber::with_default(subscriber, || {
            emit_rollout_event(RolloutEvent::TrafficSplitApplied, &ctx);
        });

        let got = store.lock().unwrap();
        // The discriminant and a few representative IDs survive with values.
        assert_eq!(
            got.get("rollout.event").map(String::as_str),
            Some("rollout.traffic_split.applied")
        );
        assert_eq!(got.get("gt.tenant").map(String::as_str), Some("acme"));
        assert_eq!(
            got.get("gt.deployment_id").map(String::as_str),
            Some("01JTKS")
        );
        assert_eq!(got.get("gt.generation").map(String::as_str), Some("3"));

        // Drift guard: the captured gt.* set must equal kv()'s key set exactly.
        let captured: BTreeSet<&str> = got
            .keys()
            .map(String::as_str)
            .filter(|k| k.starts_with("gt."))
            .collect();
        let expected: BTreeSet<&str> = ctx
            .kv()
            .into_iter()
            .filter_map(|(k, v)| v.map(|_| k))
            .collect();
        assert_eq!(
            captured, expected,
            "every gt.* key from kv() must be declared at the rollout span callsite"
        );
    }
}
