use once_cell::sync::OnceCell;
use opentelemetry::Context as OtelContext;
use opentelemetry::KeyValue;
use opentelemetry::metrics::UpDownCounter;
use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry::metrics::{
    Counter as OtelCounter, Histogram as OtelHistogram, Meter, MeterProvider, ObservableGauge,
};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{ManualReader, MeterProviderBuilder, SdkMeterProvider};
use std::sync::Arc;

use crate::context::CloudCtx;
use crate::init::TELEMETRY_STATE;

static METER_PROVIDER: OnceCell<Option<Arc<SdkMeterProvider>>> = OnceCell::new();

#[derive(Clone)]
pub struct CounterWrapper {
    counter: Option<OtelCounter<f64>>,
}

impl CounterWrapper {
    pub fn add(&self, value: f64) {
        if let Some(counter) = &self.counter {
            counter.add(&OtelContext::current(), value, &attributes());
        }
    }
}

#[derive(Clone)]
pub struct GaugeWrapper;

impl GaugeWrapper {
    pub fn record(&self, _value: f64) {
        // No-op gauge; placeholder for future use.
    }
}

#[derive(Clone)]
pub struct HistogramWrapper {
    histogram: Option<OtelHistogram<f64>>,
}

impl HistogramWrapper {
    pub fn record(&self, value: f64) {
        if let Some(histogram) = &self.histogram {
            histogram.record(&OtelContext::current(), value, &attributes());
        }
    }
}

pub fn setup_meter_provider(provider: Option<SdkMeterProvider>) {
    let _ = METER_PROVIDER.set(provider.map(Arc::new));
}

pub fn counter(name: &'static str) -> CounterWrapper {
    let instrument = meter()
        .map(|meter| meter.f64_counter(name).build())
        .transpose();
    CounterWrapper {
        counter: instrument.ok().flatten(),
    }
}

pub fn gauge(_name: &'static str) -> GaugeWrapper {
    GaugeWrapper
}

pub fn histogram(name: &'static str) -> HistogramWrapper {
    let instrument = meter()
        .map(|meter| meter.f64_histogram(name).build())
        .transpose();
    HistogramWrapper {
        histogram: instrument.ok().flatten(),
    }
}

fn meter() -> Option<Meter> {
    METER_PROVIDER.get().and_then(|provider| {
        provider
            .as_ref()
            .map(|provider| provider.meter("greentic-telemetry"))
    })
}

fn attributes() -> Vec<KeyValue> {
    let mut attrs = Vec::new();

    if let Some(state) = TELEMETRY_STATE.get() {
        attrs.push(KeyValue::new(
            "service.name",
            state.service_name.to_string(),
        ));

        for (key, value) in state.context_snapshot() {
            if let Some(value) = value {
                attrs.push(KeyValue::new(key, value));
            }
        }
    }

    attrs
}
