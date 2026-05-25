//! B11 export-facing test: a `TelemetryCtx` set in the task-local slot must
//! reach exported OTLP spans as real attributes — even for spans whose callsite
//! never declared the `gt.*` fields. This is the regression guard for "the IDs
//! are silently dropped before export."
#![cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]

use std::sync::{Arc, Mutex};

use greentic_telemetry::{
    TelemetryCtx, annotate_current_span, annotate_span, set_current_telemetry_ctx, with_task_local,
};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tracing::subscriber;
use tracing_subscriber::prelude::*;

#[derive(Clone, Debug)]
struct TestExporter {
    spans: Arc<Mutex<Vec<SpanData>>>,
}

impl SpanExporter for TestExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let spans = self.spans.clone();
        async move {
            spans.lock().unwrap().extend(batch);
            Ok(())
        }
    }
}

fn attr_keys(span: &SpanData) -> Vec<String> {
    span.attributes
        .iter()
        .map(|kv| kv.key.to_string())
        .collect()
}

fn attr_value(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

#[test]
fn task_local_ctx_exports_as_span_attributes_without_declared_fields() {
    let exported = Arc::new(Mutex::new(Vec::new()));
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(TestExporter {
            spans: exported.clone(),
        })
        .build();
    let tracer = provider.tracer("b11-context-export");

    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = subscriber::set_default(subscriber);

    let ctx = TelemetryCtx::new("acme")
        .with_env("prod-eu")
        .with_customer_id("cust-acme")
        .with_deployment_id("01JTKS")
        .with_bundle_id("customer.support")
        .with_revision_id("01JTKR");

    // A plain span that declares NONE of the gt.* fields; `annotate_span`
    // attaches the context as real OTel attributes regardless.
    let span = tracing::info_span!("plain.op");
    annotate_span(&span, &ctx);
    {
        let _enter = span.enter();
    }
    drop(span);

    let _ = provider.force_flush();
    let _ = provider.shutdown();

    let spans = exported.lock().unwrap();
    let span = spans.last().expect("span exported");
    let keys = attr_keys(span);

    for expected in [
        "gt.tenant",
        "gt.env",
        "gt.customer_id",
        "gt.deployment_id",
        "gt.bundle_id",
        "gt.revision_id",
    ] {
        assert!(
            keys.contains(&expected.to_string()),
            "exported span missing `{expected}`; got {keys:?}"
        );
    }
    assert_eq!(
        attr_value(span, "gt.customer_id").as_deref(),
        Some("cust-acme")
    );
    assert_eq!(
        attr_value(span, "gt.revision_id").as_deref(),
        Some("01JTKR")
    );
}

#[test]
fn unset_rollout_ids_are_absent_from_exported_attributes() {
    let exported = Arc::new(Mutex::new(Vec::new()));
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(TestExporter {
            spans: exported.clone(),
        })
        .build();
    let tracer = provider.tracer("b11-context-export-unset");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = subscriber::set_default(subscriber);

    let span = tracing::info_span!("plain.op");
    annotate_span(&span, &TelemetryCtx::new("acme").with_env("local"));
    {
        let _enter = span.enter();
    }
    drop(span);
    let _ = provider.force_flush();
    let _ = provider.shutdown();

    let spans = exported.lock().unwrap();
    let span = spans.last().expect("span exported");
    let keys = attr_keys(span);
    assert!(keys.contains(&"gt.tenant".to_string()));
    assert!(keys.contains(&"gt.env".to_string()));
    for absent in ["gt.customer_id", "gt.deployment_id", "gt.revision_id"] {
        assert!(
            !keys.contains(&absent.to_string()),
            "`{absent}` must be absent when unset; got {keys:?}"
        );
    }
}

#[tokio::test]
async fn annotate_current_span_uses_task_local_ctx() {
    let exported = Arc::new(Mutex::new(Vec::new()));
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(TestExporter {
            spans: exported.clone(),
        })
        .build();
    let tracer = provider.tracer("b11-context-export-tasklocal");
    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = subscriber::set_default(subscriber);

    // The task-local ctx slot only exists inside a `with_task_local` scope —
    // how the runner wraps invocation handling.
    with_task_local(async {
        set_current_telemetry_ctx(
            TelemetryCtx::new("acme")
                .with_env("prod-eu")
                .with_deployment_id("01JTKS"),
        );
        let span = tracing::info_span!("plain.op");
        annotate_current_span(&span);
        {
            let _enter = span.enter();
        }
        drop(span);
    })
    .await;

    let _ = provider.force_flush();
    let _ = provider.shutdown();

    let spans = exported.lock().unwrap();
    let span = spans.last().expect("span exported");
    assert_eq!(
        attr_value(span, "gt.deployment_id").as_deref(),
        Some("01JTKS")
    );
    assert_eq!(attr_value(span, "gt.env").as_deref(), Some("prod-eu"));
}
