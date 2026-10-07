//! A streaming client disconnect becomes an abort on the backend, and late
//! tokens the backend sends after that abort are dropped and counted
//! (LIFE-02). `queued_stream_disconnect_abort_bound` measures the D-03
//! bound on a request still queued (no token emitted yet) when the client
//! disconnects.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use common::Observed;
use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

fn has_abort(observed: &[Observed], uid: i64) -> bool {
    observed
        .iter()
        .any(|o| matches!(o, Observed::Abort { uid: u } if *u == uid))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_stream_disconnect_sends_abort_and_counts_late_tokens() {
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

    let body = br#"{"prompt":"abcdef","max_tokens":1000}"#;
    let mut stream = OpenStream::open(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(stream.status, 200);

    for i in 0..3 {
        let chunk = stream.next_chunk(Duration::from_secs(2)).await;
        assert!(chunk.is_some(), "expected data chunk {i}");
    }

    let disconnect_at = Instant::now();
    drop(stream);

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if has_abort(&server.mock.observed(), 0) {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for abort");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let disconnect_to_abort_ms = disconnect_at.elapsed().as_millis();
    println!(
        "disconnect_to_abort_ms={disconnect_to_abort_ms} (mock-scheduler measurement on the Mac, not performance evidence)"
    );
    assert!(
        disconnect_to_abort_ms < 1000,
        "abort took too long: {disconnect_to_abort_ms}ms"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.cancelled, 1);
    assert_eq!(snap.finished, 0);
    assert_eq!(snap.failed, 0);
    assert_eq!(snap.invalid_transitions, 0);

    let stats_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stats = server.engine.dispatch_stats();
        if stats.unknown_uid + stats.closed_route == 3 {
            break;
        }
        assert!(
            Instant::now() < stats_deadline,
            "timed out waiting for late-token count; last stats: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let stats = server.engine.dispatch_stats();
    assert_eq!(
        stats.unknown_uid + stats.closed_route,
        3,
        "late-token count must not keep growing: {stats:?}"
    );

    let resp = http_client::send(
        server.addr,
        "POST",
        "/generate",
        Some(br#"{"prompt":"xy","max_tokens":2}"#),
    )
    .await;
    assert_eq!(resp.status, 200);
    assert!(
        resp.body.ends_with(b"data: [DONE]\n"),
        "server must keep serving: {:?}",
        String::from_utf8_lossy(&resp.body)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_stream_disconnect_abort_bound() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "1500", "--decode-delay-ms", "20"],
        TestConfig::default(),
    )
    .await;

    let body = br#"{"prompt":"abcdef","max_tokens":10}"#;
    let stream = OpenStream::open(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(stream.status, 200);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let t0 = Instant::now();
    drop(stream);

    let deadline = t0 + Duration::from_millis(2500);
    loop {
        if has_abort(&server.mock.observed(), 0) {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for abort");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    println!(
        "queued_disconnect_to_abort_ms={} (mock-scheduler measurement on the Mac, not performance evidence)",
        t0.elapsed().as_millis()
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.cancelled, 1);
}
