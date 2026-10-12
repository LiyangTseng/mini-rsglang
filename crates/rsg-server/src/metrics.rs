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
//! fixed (eight series plus the [`StageMetrics`] ones) regardless of traffic, and nothing
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

/// Bucket upper bounds, in seconds, for every `rsg_stage_*_seconds`
/// histogram. Buckets (not the exporter's default summaries, whose
/// quantiles only cover a recent rolling window) keep the whole run, so a
/// burst at the start of a benchmark still shows up in a scrape at its end.
pub const STAGE_BUCKETS_SECONDS: [f64; 17] = [
    0.0001, 0.00025, 0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5,
    5.0, 10.0, 30.0,
];

/// Per-lifecycle-stage latency histograms (Phase 8, #15), one series per
/// stage, each in seconds with [`STAGE_BUCKETS_SECONDS`]. `Clone` is cheap:
/// every field is an `Arc` handle.
#[derive(Clone)]
pub struct StageMetrics {
    /// `Engine::start` until the driver task first runs on a worker.
    pub spawn_delay: Histogram,
    /// `codec.encode` (chat template + tokenize), inline on the driver task.
    pub encode: Histogram,
    /// Time the decoder build waited in tokio's blocking pool before running.
    pub decoder_queue: Histogram,
    /// Run time of `codec.decoder()` on the blocking pool.
    pub decoder_build: Histogram,
    /// The `writer.submit` await (bounded-queue enqueue).
    pub submit_enqueue: Histogram,
    /// Time a message sat in the writer's queue before `tx-zmq` dequeued it.
    pub writer_queue: Histogram,
    /// One frame's `send_backend` call on `tx-zmq`.
    pub zmq_send: Histogram,
    /// Submit returned until the driver received the first token.
    pub backend_wait: Histogram,
}

/// `(series name, help text)` for every [`StageMetrics`] field, in field order.
const STAGE_SERIES: [(&str, &str); 8] = [
    (
        "rsg_stage_spawn_delay_seconds",
        "Engine::start until the request's driver task first runs.",
    ),
    (
        "rsg_stage_encode_seconds",
        "Prompt encode (chat template + tokenize), inline on the driver task.",
    ),
    (
        "rsg_stage_decoder_queue_seconds",
        "Time the per-request decoder build waited in tokio's blocking pool.",
    ),
    (
        "rsg_stage_decoder_build_seconds",
        "Run time of the per-request decoder build.",
    ),
    (
        "rsg_stage_submit_enqueue_seconds",
        "The writer.submit await (enqueue onto the tx-zmq queue).",
    ),
    (
        "rsg_stage_writer_queue_seconds",
        "Time a message waited in the tx-zmq queue before being dequeued.",
    ),
    (
        "rsg_stage_zmq_send_seconds",
        "One frame's send on the backend ZMQ socket.",
    ),
    (
        "rsg_stage_backend_wait_seconds",
        "Submit returned until the driver received the first token.",
    ),
];

impl StageMetrics {
    /// Handles that record nothing, for callers without a server recorder.
    pub fn noop() -> StageMetrics {
        StageMetrics {
            spawn_delay: Histogram::noop(),
            encode: Histogram::noop(),
            decoder_queue: Histogram::noop(),
            decoder_build: Histogram::noop(),
            submit_enqueue: Histogram::noop(),
            writer_queue: Histogram::noop(),
            zmq_send: Histogram::noop(),
            backend_wait: Histogram::noop(),
        }
    }

    fn register(recorder: &impl Recorder) -> StageMetrics {
        let reg = |(name, help): (&'static str, &'static str)| {
            recorder.describe_histogram(KeyName::from_const_str(name), None, help.into());
            recorder.register_histogram(&Key::from_static_name(name), &META)
        };
        let [
            spawn_delay,
            encode,
            decoder_queue,
            decoder_build,
            submit_enqueue,
            writer_queue,
            zmq_send,
            backend_wait,
        ] = STAGE_SERIES.map(reg);
        StageMetrics {
            spawn_delay,
            encode,
            decoder_queue,
            decoder_build,
            submit_enqueue,
            writer_queue,
            zmq_send,
            backend_wait,
        }
    }
}

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
    writer_queue_depth: Gauge,
    ttft_seconds: Histogram,
    stages: StageMetrics,
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
            .set_buckets_for_metric(
                Matcher::Prefix("rsg_stage_".to_string()),
                &STAGE_BUCKETS_SECONDS,
            )
            .expect("STAGE_BUCKETS_SECONDS is a fixed, non-empty bucket list")
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
        recorder.describe_gauge(
            KeyName::from_const_str("rsg_writer_queue_depth"),
            None,
            "Messages enqueued on the single tx-zmq writer's inbox but not yet sent to the scheduler.".into(),
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
        let writer_queue_depth =
            recorder.register_gauge(&Key::from_name("rsg_writer_queue_depth"), &META);
        let ttft_seconds = recorder.register_histogram(&Key::from_name("rsg_ttft_seconds"), &META);
        let stages = StageMetrics::register(&recorder);

        requests_total.increment(0);
        requests_finished_total.increment(0);
        requests_cancelled_total.increment(0);
        requests_failed_total.increment(0);
        late_tokens_dropped_total.increment(0);
        requests_active.set(0.0);
        writer_queue_depth.set(0.0);

        let handle = recorder.handle();

        ServerMetrics {
            requests_total,
            requests_finished_total,
            requests_cancelled_total,
            requests_failed_total,
            late_tokens_dropped_total,
            requests_active,
            writer_queue_depth,
            ttft_seconds,
            stages,
            handle,
        }
    }

    /// The per-stage latency handles (Phase 8, #15), for the engine and writer.
    pub fn stages(&self) -> StageMetrics {
        self.stages.clone()
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
    /// of truth for that count). When `writer_queue_depth` is `Some`, sets
    /// `rsg_writer_queue_depth` to it. Then renders.
    pub fn render(
        &self,
        dispatch: Option<DispatchStatsSnapshot>,
        writer_queue_depth: Option<usize>,
    ) -> String {
        if let Some(d) = dispatch {
            self.late_tokens_dropped_total
                .absolute(d.unknown_uid + d.closed_route);
        }
        if let Some(depth) = writer_queue_depth {
            self.writer_queue_depth.set(depth as f64);
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
        let text = metrics.render(None, None);
        for name in [
            "rsg_requests_total",
            "rsg_requests_finished_total",
            "rsg_requests_cancelled_total",
            "rsg_requests_failed_total",
            "rsg_late_tokens_dropped_total",
            "rsg_requests_active",
            "rsg_writer_queue_depth",
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
        let text = metrics.render(None, None);
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
        let text = metrics.render(Some(dispatch), None);
        assert!(text.contains("rsg_late_tokens_dropped_total 3"), "{text}");
    }

    #[test]
    fn stage_histograms_keep_every_observation_in_buckets() {
        let metrics = ServerMetrics::new();
        let stages = metrics.stages();
        stages.decoder_build.record(Duration::from_millis(20));
        stages.decoder_build.record(Duration::from_secs(3));
        let text = metrics.render(None, None);
        for want in [
            "rsg_stage_decoder_build_seconds_bucket{le=\"0.025\"} 1",
            "rsg_stage_decoder_build_seconds_bucket{le=\"5\"} 2",
            "rsg_stage_decoder_build_seconds_count 2",
        ] {
            assert!(text.contains(want), "missing {want:?} in:\n{text}");
        }
    }

    #[test]
    fn render_sets_writer_queue_depth_when_given() {
        let metrics = ServerMetrics::new();
        let text = metrics.render(None, Some(7));
        assert!(text.contains("rsg_writer_queue_depth 7"), "{text}");
    }
}
