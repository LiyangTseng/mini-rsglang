//! `/metrics`, `/health` and `/health/ready` (API-02): every number
//! `/metrics` exposes comes from the same registry that proves LIFE-01's
//! exactly-one-terminal-state invariant.

#[allow(dead_code)]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

use rsg_server::http::{self, AppState};
use rsg_server::metrics::ServerMetrics;

/// One parsed Prometheus text-exposition sample line: `name{labels} value`
/// (labels omitted for an unlabeled series).
struct Sample {
    name: String,
    labels: String,
    value: f64,
}

/// Parses every non-comment line of a Prometheus text exposition body into
/// `Sample`s. Lines starting with `#` (HELP/TYPE) are skipped.
fn parse_samples(text: &str) -> Vec<Sample> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|line| {
            let (head, value) = line.rsplit_once(' ').expect("name/labels value");
            let value: f64 = value.parse().expect("numeric sample value");
            if let Some(brace) = head.find('{') {
                let name = head[..brace].to_string();
                let labels = head[brace..].to_string();
                Sample {
                    name,
                    labels,
                    value,
                }
            } else {
                Sample {
                    name: head.to_string(),
                    labels: String::new(),
                    value,
                }
            }
        })
        .collect()
}

/// The value of the first sample named exactly `name` with no labels.
fn value_of(samples: &[Sample], name: &str) -> Option<f64> {
    samples
        .iter()
        .find(|s| s.name == name && s.labels.is_empty())
        .map(|s| s.value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_metrics_count_a_finished_request() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let resp = http_client::send(server.addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);
    for name in [
        "rsg_requests_total",
        "rsg_requests_cancelled_total",
        "rsg_requests_finished_total",
        "rsg_requests_failed_total",
        "rsg_requests_active",
        "rsg_late_tokens_dropped_total",
    ] {
        assert_eq!(value_of(&samples, name), Some(0.0), "{name} at startup: {text}");
    }

    let body = br#"{"prompt":"abc","max_tokens":3}"#;
    let stream = OpenStream::open(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(stream.status, 200);
    let resp = stream.read_to_end(Duration::from_secs(5)).await;
    assert!(resp.complete, "generate request must complete cleanly");
    assert!(resp.body.ends_with(b"data: [DONE]\n"));

    server.snapshot_when_idle(Duration::from_secs(5)).await;

    let resp = http_client::send(server.addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.header("content-type"),
        Some("text/plain; version=0.0.4; charset=utf-8")
    );
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);

    assert_eq!(value_of(&samples, "rsg_requests_total"), Some(1.0), "{text}");
    assert_eq!(
        value_of(&samples, "rsg_requests_finished_total"),
        Some(1.0),
        "{text}"
    );
    assert_eq!(value_of(&samples, "rsg_requests_active"), Some(0.0), "{text}");
    assert_eq!(
        value_of(&samples, "rsg_ttft_seconds_count"),
        Some(1.0),
        "{text}"
    );

    let has_any_bucket = samples
        .iter()
        .any(|s| s.name == "rsg_ttft_seconds_bucket" && s.labels.contains("le=\""));
    assert!(has_any_bucket, "expected at least one TTFT bucket line: {text}");

    let inf_bucket = samples
        .iter()
        .find(|s| s.name == "rsg_ttft_seconds_bucket" && s.labels.contains("le=\"+Inf\""));
    assert!(
        inf_bucket.is_some_and(|s| s.value == 1.0),
        "expected rsg_ttft_seconds_bucket{{le=\"+Inf\"}} 1: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn health_and_readiness_before_and_after_engine() {
    let state = AppState::new("test-model", ServerMetrics::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(http::serve(listener, state.clone()));

    let resp = http_client::send(addr, "GET", "/health", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, br#"{"status":"ok"}"#);

    let resp = http_client::send(addr, "GET", "/health/ready", None).await;
    assert_eq!(resp.status, 503);
    assert_eq!(resp.body, br#"{"status":"starting"}"#);

    let resp = http_client::send(addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);
    assert_eq!(value_of(&samples, "rsg_requests_total"), Some(0.0), "{text}");

    // Borrow a real engine from a fully-wired TestServer rather than
    // standing up a second mock-scheduler by hand: `/health/ready`'s only
    // contract is "engine set or not", not which engine.
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;
    state.set_engine(Arc::clone(&server.engine));

    let resp = http_client::send(addr, "GET", "/health/ready", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, br#"{"status":"ready"}"#);

    let resp = http_client::send(addr, "GET", "/health", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body, br#"{"status":"ok"}"#);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_request_and_late_tokens_are_counted() {
    let server = TestServer::start(
        &[
            "--decode-delay-ms",
            "20",
            "--misbehave-uids",
            "0",
            "--behavior",
            "late-abort-token",
        ],
        TestConfig::default(),
    )
    .await;

    let body0 = br#"{"prompt":"abcdef","max_tokens":1000}"#;
    let mut stream0 = OpenStream::open(server.addr, "POST", "/generate", Some(body0)).await;
    assert_eq!(stream0.status, 200);
    for i in 0..2 {
        let chunk = stream0.next_chunk(Duration::from_secs(2)).await;
        assert!(chunk.is_some(), "expected data chunk {i} for uid 0");
    }
    drop(stream0);

    let body1 = br#"{"prompt":"xy","max_tokens":3}"#;
    let resp1 = http_client::send(server.addr, "POST", "/generate", Some(body1)).await;
    assert_eq!(resp1.status, 200);
    assert!(resp1.body.ends_with(b"data: [DONE]\n"));

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stats = server.engine.dispatch_stats();
        if stats.unknown_uid + stats.closed_route == 3 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for late-token count; last stats: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    server.snapshot_when_idle(Duration::from_secs(5)).await;

    let resp = http_client::send(server.addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);
    assert_eq!(value_of(&samples, "rsg_requests_total"), Some(2.0), "{text}");
    assert_eq!(
        value_of(&samples, "rsg_requests_cancelled_total"),
        Some(1.0),
        "{text}"
    );
    assert_eq!(
        value_of(&samples, "rsg_requests_finished_total"),
        Some(1.0),
        "{text}"
    );
    assert_eq!(
        value_of(&samples, "rsg_ttft_seconds_count"),
        Some(2.0),
        "{text}"
    );
    assert_eq!(value_of(&samples, "rsg_requests_active"), Some(0.0), "{text}");
    assert_eq!(
        value_of(&samples, "rsg_late_tokens_dropped_total"),
        Some(3.0),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_request_is_counted() {
    let server = TestServer::start(
        &["--misbehave-uids", "0", "--behavior", "drop-overlong"],
        TestConfig {
            backend_timeout_ms: 300,
            ..TestConfig::default()
        },
    )
    .await;

    let body = br#"{"prompt":"abc","max_tokens":10}"#;
    let stream = OpenStream::open(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(stream.status, 200);
    let resp = stream.read_to_end(Duration::from_millis(1500)).await;
    assert!(!resp.complete, "stream must not end cleanly");

    server.snapshot_when_idle(Duration::from_secs(2)).await;

    let resp = http_client::send(server.addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);
    assert_eq!(
        value_of(&samples, "rsg_requests_failed_total"),
        Some(1.0),
        "{text}"
    );
    assert_eq!(
        value_of(&samples, "rsg_ttft_seconds_count"),
        Some(0.0),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_scrapes_are_monotonic() {
    let server = TestServer::start(&["--decode-delay-ms", "5"], TestConfig::default()).await;
    let addr = server.addr;

    let stop = Arc::new(AtomicBool::new(false));
    let scrape_stop = Arc::clone(&stop);
    let scrape_task = tokio::spawn(async move {
        let mut last_total = 0.0_f64;
        let mut last_finished = 0.0_f64;
        while !scrape_stop.load(Ordering::Relaxed) {
            let resp = http_client::send(addr, "GET", "/metrics", None).await;
            assert_eq!(resp.status, 200);
            let text = String::from_utf8(resp.body).expect("utf8 metrics body");
            let samples = parse_samples(&text);
            let total = value_of(&samples, "rsg_requests_total").expect("total sample");
            let finished =
                value_of(&samples, "rsg_requests_finished_total").expect("finished sample");
            assert!(
                total >= last_total,
                "rsg_requests_total went backwards: {total} < {last_total}: {text}"
            );
            assert!(
                finished >= last_finished,
                "rsg_requests_finished_total went backwards: {finished} < {last_finished}: {text}"
            );
            assert!(
                finished <= total,
                "rsg_requests_finished_total {finished} exceeds rsg_requests_total {total}: {text}"
            );
            last_total = total;
            last_finished = finished;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let mut handles = Vec::new();
    for _ in 0..16 {
        let addr = server.addr;
        handles.push(tokio::spawn(async move {
            let body = br#"{"prompt":"abcdef","max_tokens":20}"#;
            let resp = http_client::send(addr, "POST", "/generate", Some(body)).await;
            assert_eq!(resp.status, 200);
            assert!(resp.body.ends_with(b"data: [DONE]\n"));
        }));
    }
    for h in handles {
        h.await.expect("request task panicked");
    }

    server.snapshot_when_idle(Duration::from_secs(5)).await;
    stop.store(true, Ordering::Relaxed);
    scrape_task.await.expect("scrape task panicked");

    let resp = http_client::send(server.addr, "GET", "/metrics", None).await;
    assert_eq!(resp.status, 200);
    let text = String::from_utf8(resp.body).expect("utf8 metrics body");
    let samples = parse_samples(&text);
    assert_eq!(
        value_of(&samples, "rsg_requests_total"),
        Some(16.0),
        "{text}"
    );
    assert_eq!(
        value_of(&samples, "rsg_requests_finished_total"),
        Some(16.0),
        "{text}"
    );
    assert_eq!(value_of(&samples, "rsg_requests_active"), Some(0.0), "{text}");
}
