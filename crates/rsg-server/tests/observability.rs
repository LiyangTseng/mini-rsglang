//! `/metrics`, `/health` and `/health/ready` (API-02): every number
//! `/metrics` exposes comes from the same registry that proves LIFE-01's
//! exactly-one-terminal-state invariant.

#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

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
