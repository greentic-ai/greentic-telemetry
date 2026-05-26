//! Rollout lifecycle events (C5).
//!
//! A deployment's rollout moves through a small set of state transitions —
//! a revision is staged, warmed, drained, evicted; a traffic split is applied;
//! a health gate passes or fails. [`emit_rollout_event`] records each
//! transition as a structured telemetry item so operators can trace a rollout
//! end to end and correlate it with the revision/bundle/deployment it touched.
//!
//! Each transition rides on a short-lived `greentic.rollout` span carrying the
//! full [`TelemetryCtx`] attribute set via the [`annotate_span`] export
//! primitive, so the `gt.*` attribution reaches OTLP regardless of the span
//! callsite. The inner `info!` marker keeps the transition visible to plain
//! log subscribers when no OTLP exporter is configured.
//!
//! [`annotate_span`]: crate::layer::annotate_span

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
/// Emits a `greentic.rollout` span annotated with the full `gt.*` set plus a
/// `rollout.event` discriminant, and an `info!` marker inside it. Safe to call
/// with no subscriber installed (then it is a no-op beyond the cheap span
/// construction).
pub fn emit_rollout_event(event: RolloutEvent, tctx: &TelemetryCtx) {
    let span = tracing::info_span!("greentic.rollout", rollout.event = event.as_str());
    #[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
    crate::layer::annotate_span(&span, tctx);
    let _enter = span.enter();
    tracing::info!(
        rollout.event = event.as_str(),
        gt.tenant = %tctx.tenant,
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
            .with_generation("3");
        emit_rollout_event(RolloutEvent::TrafficSplitApplied, &ctx);
        emit_rollout_event(RolloutEvent::HealthGateFailed, &ctx);
    }
}
