//! Guest-side telemetry shim.
//!
//! With the `wit-guest` feature on a `wasm32` target, `log` / `span_start` /
//! `span_end` forward to the host through the `greentic:telemetry/logging` WIT
//! import (see `wit/greentic-telemetry.wit`). Otherwise (native builds, or
//! `wasm32` without `wit-guest`) they fall back to stdout, so this module is
//! always usable for local development.
//!
//! ## Stdout-fallback line format
//!
//! Every emitted line carries the information needed to trace an event back to
//! its source even when many components share a single stdout:
//!
//! * RFC3339 UTC timestamp (millisecond precision)
//! * Severity level
//! * Component identity (set once via [`set_component_name`]; otherwise blank)
//! * Caller `file:line` (captured automatically via `#[track_caller]`, can be
//!   suppressed via [`set_caller_location_enabled`] when logs get noisy)
//! * The free-form message
//! * The structured `key=value` field list (optional)
//!
//! ```text
//! 2026-05-22T10:30:42.123Z [DEBUG][messaging.webchat-gui] components/messaging-provider-webchat-gui/src/lib.rs:160 span-start: send_payload [id=1, event_kind=send_payload]
//! 2026-05-22T10:30:42.165Z [INFO][messaging.webchat-gui] components/messaging-provider-webex/src/ops/send_payload.rs:142 message delivered [message_id=...]
//! 2026-05-22T10:30:42.220Z [DEBUG][messaging.webchat-gui] components/messaging-provider-webchat-gui/src/lib.rs:160 span-end: send_payload [id=1, duration_ms=55]
//! ```
//!
//! Spans get monotonically increasing ids so concurrent / nested spans can be
//! correlated. Duration is computed from the start instant.
//!
//! ## Recommended init
//!
//! Each guest component should call [`set_component_name`] once during its
//! first invocation (or in a lazy init), passing the canonical provider id:
//!
//! ```ignore
//! greentic_telemetry::wasm_guest::set_component_name("messaging.webchat-gui");
//! ```
//!
//! Operators that find the `file:line` segment too noisy can disable it at
//! runtime:
//!
//! ```ignore
//! greentic_telemetry::wasm_guest::set_caller_location_enabled(false);
//! ```
//!
//! The identity and the caller-location toggle are stored in atomic globals,
//! so subsequent calls to the identity setter after the first are ignored
//! (no panic).

use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
use std::panic::Location;
#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
use std::sync::Mutex;
#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
use std::sync::OnceLock;
#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
use std::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn rank(self) -> u8 {
        match self {
            Level::Trace => 0,
            Level::Debug => 1,
            Level::Info => 2,
            Level::Warn => 3,
            Level::Error => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Field<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

/// Register the canonical component name to be prepended to every fallback
/// log line for the lifetime of this process. The first call wins; subsequent
/// calls are silently ignored. On the `wit-guest` path this is a no-op since
/// identity is supplied via the host context.
pub fn set_component_name(name: impl Into<String>) {
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        let _ = component_name_cell().set(name.into());
    }
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        let _ = name;
    }
}

/// Turn the `file:line` segment in fallback lines on or off at runtime.
///
/// Defaults to enabled. Disable when logs get too verbose (e.g. tight loops
/// emitting `Level::Trace`) and you only need the timestamp, level,
/// component, and message.
pub fn set_caller_location_enabled(enabled: bool) {
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        CALLER_LOCATION_ENABLED.store(enabled, Ordering::Relaxed);
    }
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        let _ = enabled;
    }
}

/// Set the minimum level for emission. Calls to [`log`] / [`span_start`] /
/// [`span_end`] at a level below this threshold short-circuit before
/// formatting and before any stdout write, so tight `Trace` / `Debug` loops
/// pay nothing.
///
/// Spans (start + end) emit at `Level::Debug` in the fallback path, so
/// setting the threshold to `Info` or higher silences all spans as well.
/// `span_start` still returns a sentinel id so the matching `span_end` is a
/// no-op rather than an "unknown id" diagnostic.
///
/// Defaults to `Level::Trace` (no filtering). On the `wit-guest` path the
/// check still applies on the guest side so we never cross the WIT boundary
/// for filtered events. The host pipeline's own filter (e.g. `RUST_LOG`)
/// remains the authoritative level filter for what gets exported.
pub fn set_min_level(level: Level) {
    MIN_LEVEL.store(level.rank(), Ordering::Relaxed);
}

fn level_is_enabled(level: Level) -> bool {
    level.rank() >= MIN_LEVEL.load(Ordering::Relaxed)
}

static MIN_LEVEL: AtomicU8 = AtomicU8::new(0); // 0 = Level::Trace; no filtering.

/// Sentinel returned by [`span_start`] when the configured minimum level
/// filters the span out. [`span_end`] treats this id as a no-op so callers
/// don't need to special-case it.
const FILTERED_SPAN_ID: u64 = 0;

#[track_caller]
pub fn log(level: Level, message: &str, fields: &[Field<'_>]) {
    if !level_is_enabled(level) {
        return;
    }
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        host::log(level, message, fields);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        let caller = Location::caller();
        fallback_log(level, message, fields, caller);
    }
}

#[track_caller]
pub fn span_start(name: &str, fields: &[Field<'_>]) -> u64 {
    // Spans emit start + end lines at Debug; if Debug is filtered out, skip
    // the whole span lifecycle. Returning a sentinel id keeps span_end a
    // clean no-op.
    if !level_is_enabled(Level::Debug) {
        return FILTERED_SPAN_ID;
    }
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        return host::span_start(name, fields);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        let caller = Location::caller();
        fallback_span_start(name, fields, caller)
    }
}

#[track_caller]
pub fn span_end(id: u64) {
    if id == FILTERED_SPAN_ID {
        return;
    }
    if !level_is_enabled(Level::Debug) {
        // Level was lowered between span_start and span_end; drop the close
        // line silently rather than emitting an orphaned end.
        return;
    }
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        host::span_end(id);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        let caller = Location::caller();
        fallback_span_end(id, caller);
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
mod host {
    use super::{Field, Level};

    wit_bindgen::generate!({
        path: "wit",
        world: "guest-telemetry",
    });

    use greentic::telemetry::logging::{self as wit, Fields, Level as WitLevel};

    fn to_wit_level(level: Level) -> WitLevel {
        match level {
            Level::Trace => WitLevel::Trace,
            Level::Debug => WitLevel::Debug,
            Level::Info => WitLevel::Info,
            Level::Warn => WitLevel::Warn,
            Level::Error => WitLevel::Error,
        }
    }

    fn to_wit_fields(fields: &[Field<'_>]) -> Fields {
        Fields {
            entries: fields
                .iter()
                .map(|f| (f.key.to_string(), f.value.to_string()))
                .collect(),
        }
    }

    pub fn log(level: Level, message: &str, fields: &[Field<'_>]) {
        wit::log(to_wit_level(level), message, &to_wit_fields(fields));
    }

    pub fn span_start(name: &str, fields: &[Field<'_>]) -> u64 {
        wit::span_start(name, &to_wit_fields(fields))
    }

    pub fn span_end(id: u64) {
        wit::span_end(id);
    }
}

// === Fallback formatting ===================================================

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
static CALLER_LOCATION_ENABLED: AtomicBool = AtomicBool::new(true);

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn component_name_cell() -> &'static OnceLock<String> {
    static CELL: OnceLock<String> = OnceLock::new();
    &CELL
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn level_str(level: Level) -> &'static str {
    match level {
        Level::Trace => "TRACE",
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn fallback_log(
    level: Level,
    message: &str,
    fields: &[Field<'_>],
    caller: &'static Location<'static>,
) {
    let lvl = level_str(level);
    let ts = rfc3339_now();
    let component = component_name_cell()
        .get()
        .map(|s| format!("[{s}]"))
        .unwrap_or_default();

    // Caller location is opt-out via set_caller_location_enabled(false).
    let location = if CALLER_LOCATION_ENABLED.load(Ordering::Relaxed) {
        format!(" {}:{}", caller.file(), caller.line())
    } else {
        String::new()
    };

    if fields.is_empty() {
        println!("{ts} [{lvl}]{component}{location} {message}");
    } else {
        let serialized = fields
            .iter()
            .map(|f| format!("{}={}", f.key, f.value))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{ts} [{lvl}]{component}{location} {message} [{serialized}]");
    }
}

/// Minimal RFC3339 (UTC, millisecond precision) formatter without an extra
/// dependency. Howard Hinnant's date algorithm.
#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn rfc3339_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let millis = dur.subsec_millis();

    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400) as u32;
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;

    // civil_from_days: convert days-from-1970-01-01 to (year, month, day).
    let z = days + 719_468; // shift epoch to 0000-03-01
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

// === Fallback span tracking ================================================

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
struct SpanState {
    name: String,
    started_at: Instant,
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn span_registry() -> &'static Mutex<std::collections::HashMap<u64, SpanState>> {
    use std::collections::HashMap;
    static REG: OnceLock<Mutex<HashMap<u64, SpanState>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn fallback_span_start(
    name: &str,
    fields: &[Field<'_>],
    caller: &'static Location<'static>,
) -> u64 {
    let id = NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed);
    let id_str = id.to_string();

    let mut all_fields: Vec<Field<'_>> = Vec::with_capacity(fields.len() + 1);
    all_fields.push(Field {
        key: "id",
        value: &id_str,
    });
    all_fields.extend_from_slice(fields);
    fallback_log(
        Level::Debug,
        &format!("span-start: {name}"),
        &all_fields,
        caller,
    );

    if let Ok(mut reg) = span_registry().lock() {
        reg.insert(
            id,
            SpanState {
                name: name.to_string(),
                started_at: Instant::now(),
            },
        );
    }
    id
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn fallback_span_end(id: u64, caller: &'static Location<'static>) {
    let state = span_registry()
        .lock()
        .ok()
        .and_then(|mut reg| reg.remove(&id));
    let id_str = id.to_string();
    match state {
        Some(SpanState { name, started_at }) => {
            let duration_ms = started_at.elapsed().as_millis().to_string();
            fallback_log(
                Level::Debug,
                &format!("span-end: {name}"),
                &[
                    Field {
                        key: "id",
                        value: &id_str,
                    },
                    Field {
                        key: "duration_ms",
                        value: &duration_ms,
                    },
                ],
                caller,
            );
        }
        None => {
            // Mismatched start/end (or a span_end with an id this module never
            // issued). Emit a diagnostic line rather than panicking; fallback
            // logging should never crash the guest.
            fallback_log(
                Level::Debug,
                "span-end: unknown",
                &[Field {
                    key: "id",
                    value: &id_str,
                }],
                caller,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_start_returns_unique_ascending_ids() {
        let a = span_start("a", &[]);
        let b = span_start("a", &[]);
        assert!(a >= 1, "ids should start at 1 or later, got {a}");
        assert_eq!(b, a + 1, "ids should be monotonically increasing");
        span_end(a);
        span_end(b);
    }

    #[test]
    fn span_end_with_unknown_id_does_not_panic() {
        span_end(u64::MAX);
    }

    #[test]
    fn span_lifecycle_clears_the_registry_entry() {
        let id = span_start("leak-check", &[]);
        assert!(
            span_registry().lock().unwrap().contains_key(&id),
            "registry should hold the span until span_end is called"
        );
        span_end(id);
        assert!(
            !span_registry().lock().unwrap().contains_key(&id),
            "span_end must remove the entry to avoid an unbounded map"
        );
    }

    #[test]
    fn rfc3339_now_has_the_expected_shape() {
        let s = rfc3339_now();
        // 2026-05-22T10:30:42.123Z, 24 chars total.
        assert_eq!(s.len(), 24, "unexpected length: {s:?}");
        assert!(s.ends_with('Z'), "must be UTC, got {s:?}");
        assert_eq!(s.chars().nth(10), Some('T'), "missing T separator: {s:?}");
    }

    #[test]
    fn set_component_name_is_idempotent() {
        // Two calls must not panic. We can't assert against the cell directly
        // without leaking state across tests; this just exercises the path.
        set_component_name("test-component");
        set_component_name("ignored-second-call");
    }

    #[test]
    fn caller_location_toggle_is_observable() {
        // Default is enabled.
        set_caller_location_enabled(false);
        assert!(!CALLER_LOCATION_ENABLED.load(Ordering::Relaxed));
        set_caller_location_enabled(true);
        assert!(CALLER_LOCATION_ENABLED.load(Ordering::Relaxed));
    }

    #[test]
    fn min_level_filters_span_start_to_sentinel() {
        // With min=Info, Debug-level spans are filtered out and return the
        // sentinel id; the matching span_end is a no-op.
        set_min_level(Level::Info);
        let id = span_start("filtered", &[]);
        assert_eq!(
            id, FILTERED_SPAN_ID,
            "span_start should return sentinel when filtered"
        );
        span_end(id); // must not panic, must not print "unknown id"
        // Restore default so the rest of the test module is unaffected.
        set_min_level(Level::Trace);
    }

    #[test]
    fn min_level_lets_in_at_or_above_threshold() {
        set_min_level(Level::Warn);
        assert!(!level_is_enabled(Level::Trace));
        assert!(!level_is_enabled(Level::Debug));
        assert!(!level_is_enabled(Level::Info));
        assert!(level_is_enabled(Level::Warn));
        assert!(level_is_enabled(Level::Error));
        set_min_level(Level::Trace);
    }
}
