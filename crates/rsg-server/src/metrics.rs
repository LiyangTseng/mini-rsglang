//! Per-server Prometheus metrics (API-02): `ServerMetrics` builds its own
//! `PrometheusRecorder` and keeps every handle this server ever reports to
//! in one `Clone`able struct. Each server instance gets its own private
//! recorder instance registered through the `metrics::Recorder` trait, not
//! shared with any other instance in the same process — the only way two
//! in-process test servers running in parallel stay isolated from each
//! other, and the only way `/metrics` output for one server never includes
//! another's numbers.
//!
//! No series here carries any label (T-05-13/T-05-14): cardinality is
//! fixed at seven series regardless of traffic, and nothing
//! request-specific (uid, model text, prompt) can appear in the
//! exposition.

use std::time::Duration;

use metrics::{Counter, Gauge, Histogram, Key, KeyName, Level, Metadata, Recorder};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

use crate::dispatch::DispatchStatsSnapshot;
use crate::fsm::state::LifecycleState;

/// TTFT histogram bucket upper bounds, in seconds (Claude's discretion per
/// CONTEXT: label cardinality and TTFT bucket boundaries).
pub const TTFT_BUCKETS_SECONDS: [f64; 12] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

/// The exact Prometheus text-exposition content-type `/metrics` always
/// returns.
pub const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

const META: Metadata<'static> = Metadata::new("rsg_server::metrics", Level::INFO, None);

/// Every Prometheus series this server exposes, with the live handle
/// `ServerMetrics`'s own methods write through. `Clone` is cheap: every
/// field is an `Arc` clone under the hood (the `metrics` crate's own
/// handle types), so every clone of a `ServerMetrics` observes and reports
/// into the exact same series.
#[derive(Clone)]
pub struct ServerMetrics {
    requests_total: Counter,
    requests_finished_total: Counter,
    requests_cancelled_total: Counter,
    requests_failed_total: Counter,
    late_tokens_dropped_total: Counter,
    requests_active: Gauge,
    ttft_seconds: Histogram,
    handle: PrometheusHandle,
}

impl Default for ServerMetrics {
    fn default() -> ServerMetrics {
        ServerMetrics::new()
    }
}

impl ServerMetrics {
    /// Builds a fresh per-server recorder (registered only through the
    /// `metrics::Recorder` trait on the value this function builds and
    /// keeps — never shared process-wide), configures the TTFT histogram's
    /// buckets, describes and registers every series, and touches each one
    /// once so every series renders with value 0 from the very first
    /// scrape.
    pub fn new() -> ServerMetrics {
        let recorder = PrometheusBuilder::new()
            .set_buckets_for_metric(
                Matcher::Full("rsg_ttft_seconds".to_string()),
                &TTFT_BUCKETS_SECONDS,
            )
            .expect("TTFT_BUCKETS_SECONDS is a fixed, non-empty bucket list")
            .build_recorder();

        recorder.describe_counter(
            KeyName::from_const_str("rsg_requests_total"),
            None,
            "Total requests received.".into(),
        );
        recorder.describe_counter(
            KeyName::from_const_str("rsg_requests_finished_total"),
            None,
            "Requests that reached the Finished terminal state.".into(),
        );
        recorder.describe_counter(
            KeyName::from_const_str("rsg_requests_cancelled_total"),
            None,
            "Requests that reached the Cancelled terminal state.".into(),
        );
        recorder.describe_counter(
            KeyName::from_const_str("rsg_requests_failed_total"),
            None,
            "Requests that reached the Failed terminal state.".into(),
        );
        recorder.describe_counter(
            KeyName::from_const_str("rsg_late_tokens_dropped_total"),
            None,
            "Backend replies dropped for a uid with no registered route (late tokens after an abort, or already-terminal).".into(),
        );
        recorder.describe_gauge(
            KeyName::from_const_str("rsg_requests_active"),
            None,
            "Requests currently tracked by the registry (not yet terminal).".into(),
        );
        recorder.describe_histogram(
            KeyName::from_const_str("rsg_ttft_seconds"),
            None,
            "Time to first token, in seconds, measured from Received to the first Decoding transition.".into(),
        );

        let requests_total =
            recorder.register_counter(&Key::from_name("rsg_requests_total"), &META);
        let requests_finished_total =
            recorder.register_counter(&Key::from_name("rsg_requests_finished_total"), &META);
        let requests_cancelled_total =
            recorder.register_counter(&Key::from_name("rsg_requests_cancelled_total"), &META);
        let requests_failed_total =
            recorder.register_counter(&Key::from_name("rsg_requests_failed_total"), &META);
        let late_tokens_dropped_total =
            recorder.register_counter(&Key::from_name("rsg_late_tokens_dropped_total"), &META);
        let requests_active =
            recorder.register_gauge(&Key::from_name("rsg_requests_active"), &META);
        let ttft_seconds = recorder.register_histogram(&Key::from_name("rsg_ttft_seconds"), &META);

        requests_total.increment(0);
        requests_finished_total.increment(0);
        requests_cancelled_total.increment(0);
        requests_failed_total.increment(0);
        late_tokens_dropped_total.increment(0);
        requests_active.set(0.0);

        let handle = recorder.handle();

        ServerMetrics {
            requests_total,
            requests_finished_total,
            requests_cancelled_total,
            requests_failed_total,
            late_tokens_dropped_total,
            requests_active,
            ttft_seconds,
            handle,
        }
    }

    /// A request reached `Received` (the registry's new-request path).
    pub fn record_received(&self) {
        self.requests_total.increment(1);
    }

    /// A request reached one of the three terminal states. Any non-terminal
    /// state is a no-op — callers only report terminal transitions here.
    pub fn record_terminal(&self, state: LifecycleState) {
        match state {
            LifecycleState::Finished => self.requests_finished_total.increment(1),
            LifecycleState::Cancelled => self.requests_cancelled_total.increment(1),
            LifecycleState::Failed => self.requests_failed_total.increment(1),
            _ => {}
        }
    }

    /// Sets the active-request gauge to `n` (the registry's current table
    /// length), called after every applied report.
    pub fn set_active(&self, n: u64) {
        self.requests_active.set(n as f64);
    }

    /// Records a TTFT observation, computed by the caller from a monotonic
    /// `Instant` difference (never the wall clock).
    pub fn record_ttft(&self, ttft: Duration) {
        self.ttft_seconds.record(ttft.as_secs_f64());
    }

    /// Renders the current Prometheus text exposition. When `dispatch` is
    /// `Some`, first sets `rsg_late_tokens_dropped_total` to the
    /// dispatcher's own `unknown_uid + closed_route` sum (the single source
    /// of truth for that count), then renders.
    pub fn render(&self, dispatch: Option<DispatchStatsSnapshot>) -> String {
        if let Some(d) = dispatch {
            self.late_tokens_dropped_total
                .absolute(d.unknown_uid + d.closed_route);
        }
        self.handle.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_metrics_render_every_series_at_zero() {
        let metrics = ServerMetrics::new();
        let text = metrics.render(None);
        for name in [
            "rsg_requests_total",
            "rsg_requests_finished_total",
            "rsg_requests_cancelled_total",
            "rsg_requests_failed_total",
            "rsg_late_tokens_dropped_total",
            "rsg_requests_active",
        ] {
            assert!(
                text.contains(&format!("{name} 0")),
                "missing zero-valued series {name} in:\n{text}"
            );
        }
        assert!(text.contains("rsg_ttft_seconds_bucket"), "{text}");
        assert!(text.contains("rsg_ttft_seconds_sum 0"), "{text}");
        assert!(text.contains("rsg_ttft_seconds_count 0"), "{text}");
    }

    #[test]
    fn record_terminal_increments_the_right_counter_only() {
        let metrics = ServerMetrics::new();
        metrics.record_received();
        metrics.record_terminal(LifecycleState::Finished);
        let text = metrics.render(None);
        assert!(text.contains("rsg_requests_total 1"), "{text}");
        assert!(text.contains("rsg_requests_finished_total 1"), "{text}");
        assert!(text.contains("rsg_requests_cancelled_total 0"), "{text}");
        assert!(text.contains("rsg_requests_failed_total 0"), "{text}");
    }

    #[test]
    fn render_sets_late_tokens_from_dispatch_stats() {
        let metrics = ServerMetrics::new();
        let dispatch = DispatchStatsSnapshot {
            routed: 10,
            unknown_uid: 2,
            closed_route: 1,
            malformed_frames: 0,
        };
        let text = metrics.render(Some(dispatch));
        assert!(text.contains("rsg_late_tokens_dropped_total 3"), "{text}");
    }
}
