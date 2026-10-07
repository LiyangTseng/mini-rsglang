//! Overlong-prompt 400 and backend-unresponsive timeout handling (LIFE-04):
//! a prompt the scheduler would silently drop gets a 400 instead, and a
//! backend that stops responding fails the request with an abort and a
//! truncated stream instead of hanging it forever.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use common::Observed;
use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

/// Polls `server.mock.observed()` every 5ms until it contains
/// `Abort { uid }`. The registry reaching a terminal state (or the HTTP
/// response ending) only means the abort was enqueued onto the writer's
/// channel, not that the mock-scheduler subprocess has received and
/// recorded it in its observe file across the real ipc boundary yet, so
/// callers poll for this instead of asserting immediately. Panics after
/// 2s.
async fn wait_for_abort(server: &TestServer, uid: i64) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observed = server.mock.observed();
        if observed
            .iter()
            .any(|o| matches!(o, Observed::Abort { uid: u } if *u == uid))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for Abort {{ uid: {uid} }}; last observed: {observed:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlong_prompt_gets_immediate_400_and_boundary_is_accepted() {
    let server = TestServer::start(&["--max-seq-len", "16"], TestConfig::default()).await;

    let overlong_prompt = "a".repeat(16);
    let overlong_body = format!(r#"{{"prompt":"{overlong_prompt}","max_tokens":10}}"#);
    let t0 = Instant::now();
    let resp = tokio::time::timeout(
        Duration::from_millis(500),
        http_client::send(
            server.addr,
            "POST",
            "/generate",
            Some(overlong_body.as_bytes()),
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("400 took too long: {:?}", t0.elapsed()));
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "400 took too long: {:?}",
        t0.elapsed()
    );
    assert_eq!(resp.status, 400);
    let body: serde_json::Value = serde_json::from_slice(&resp.body).expect("json body");
    assert_eq!(
        body["error"]["type"].as_str(),
        Some("invalid_request_error")
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("16"),
        "message should mention 16: {body}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);
    assert!(
        !server
            .mock
            .observed()
            .iter()
            .any(|o| matches!(o, Observed::Submit { .. })),
        "overlong prompt must never reach the backend"
    );

    let boundary_prompt = "a".repeat(15);
    let boundary_body = format!(r#"{{"prompt":"{boundary_prompt}","max_tokens":1}}"#);
    let resp2 = http_client::send(
        server.addr,
        "POST",
        "/generate",
        Some(boundary_body.as_bytes()),
    )
    .await;
    assert_eq!(resp2.status, 200);
    assert!(resp2.body.ends_with(b"data: [DONE]\n"));

    let snap2 = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap2.finished, 1);
    assert!(
        server
            .mock
            .observed()
            .iter()
            .any(|o| matches!(o, Observed::Submit { input_len: 15, .. }))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backend_timeout_fails_streaming_request_without_done() {
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
    assert!(
        !resp
            .body
            .windows(b"data: [DONE".len())
            .any(|w| w == b"data: [DONE"),
        "must not contain data: [DONE]: {:?}",
        String::from_utf8_lossy(&resp.body)
    );

    assert!(
        server
            .mock
            .observed()
            .iter()
            .any(|o| matches!(o, Observed::Submit { uid: 0, .. }))
    );
    wait_for_abort(&server, 0).await;

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);
    assert_eq!(snap.invalid_transitions, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backend_stall_mid_stream_times_out() {
    let server = TestServer::start(
        &["--decode-delay-ms", "20"],
        TestConfig {
            backend_timeout_ms: 300,
            ..TestConfig::default()
        },
    )
    .await;

    let body = br#"{"prompt":"abcdef","max_tokens":500}"#;
    let mut stream = OpenStream::open(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(stream.status, 200);

    for i in 0..3 {
        let chunk = stream.next_chunk(Duration::from_secs(2)).await;
        assert!(chunk.is_some(), "expected data chunk {i}");
    }
    server.mock.signal("-STOP");

    let resp = stream.read_to_end(Duration::from_millis(1500)).await;
    assert!(!resp.complete, "stream must not end cleanly");
    assert!(
        !resp
            .body
            .windows(b"data: [DONE".len())
            .any(|w| w == b"data: [DONE"),
        "must not contain data: [DONE]"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);

    server.mock.signal("-CONT");
    wait_for_abort(&server, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timeout_is_isolated_per_request() {
    let server = TestServer::start(
        &[
            "--decode-delay-ms",
            "5",
            "--misbehave-uids",
            "0",
            "--behavior",
            "drop-overlong",
        ],
        TestConfig {
            backend_timeout_ms: 300,
            ..TestConfig::default()
        },
    )
    .await;

    let body_a = br#"{"prompt":"abc","max_tokens":10}"#;
    let stream_a = OpenStream::open(server.addr, "POST", "/generate", Some(body_a)).await;
    assert_eq!(stream_a.status, 200);

    let body_b = br#"{"prompt":"xyz","max_tokens":3}"#;
    let resp_b = http_client::send(server.addr, "POST", "/generate", Some(body_b)).await;
    assert_eq!(resp_b.status, 200);
    assert!(resp_b.body.ends_with(b"data: [DONE]\n"));

    let resp_a = stream_a.read_to_end(Duration::from_millis(1500)).await;
    assert!(!resp_a.complete);

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);
    assert_eq!(snap.finished, 1);
    assert_eq!(snap.invalid_transitions, 0);
}
