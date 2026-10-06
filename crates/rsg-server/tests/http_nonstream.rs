//! The non-streaming disconnect bound D-03 accepts (named as this file's
//! companion measurement in plan 05-04's `AbortGuard` doc comment), and
//! LIFE-04 on the chat route: a silent backend gets 504, an overlong prompt
//! gets 400 in both stream modes, and a streaming chat request that times
//! out ends without the `finish_reason: "stop"` chunk or `data: [DONE]`.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use common::Observed;
use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

fn has_abort(observed: &[Observed], uid: i64) -> bool {
    observed
        .iter()
        .any(|o| matches!(o, Observed::Abort { uid: u } if *u == uid))
}

/// Polls `server.mock.observed()` every 5ms until it contains
/// `Abort { uid }`, returning the observed list at that point. The registry
/// reaching a terminal state (or the HTTP response ending) only means the
/// abort was enqueued onto the writer's channel, not that the
/// mock-scheduler subprocess has received and recorded it in its observe
/// file across the real ipc boundary yet, so callers poll instead of
/// asserting immediately (same pattern as plan 05-04's `http_errors.rs`).
/// Panics after 2s.
async fn wait_for_abort(server: &TestServer, uid: i64) -> Vec<Observed> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let observed = server.mock.observed();
        if has_abort(&observed, uid) {
            return observed;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for Abort {{ uid: {uid} }}; last observed: {observed:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_nonstream_disconnect_reaches_one_terminal_state() {
    let server = TestServer::start(&["--decode-delay-ms", "20"], TestConfig::default()).await;

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":100}"#;
    let mut stream = TcpStream::connect(server.addr).await.expect("connect");
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.expect("write head");
    stream.write_all(body).await.expect("write body");
    stream.flush().await.expect("flush");

    // About 2s of decode (100 tokens x 20ms); sleep well before any token
    // could have been produced, then disconnect.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let t0 = Instant::now();
    let _ = stream.shutdown().await;
    drop(stream);

    let snap = server.snapshot_when_idle(Duration::from_secs(6)).await;
    assert_eq!(snap.active, 0);
    assert_eq!(snap.invalid_transitions, 0);
    assert_eq!(snap.failed, 0);
    assert_eq!(snap.finished + snap.cancelled, 1);

    let observed = server.mock.observed();
    let outcome = if snap.cancelled == 1 {
        assert!(
            has_abort(&observed, 0),
            "cancelled outcome must have sent an abort: {observed:?}"
        );
        "cancelled"
    } else {
        assert_eq!(snap.finished, 1);
        assert!(
            !has_abort(&observed, 0),
            "finished outcome must not have sent an abort: {observed:?}"
        );
        "finished"
    };

    let terminal_after_ms = t0.elapsed().as_millis();
    println!(
        "nonstream_disconnect_outcome={outcome} terminal_after_ms={terminal_after_ms} (mock-scheduler measurement on the Mac, not performance evidence)"
    );
    assert!(
        terminal_after_ms <= 4000,
        "terminal state took too long: {terminal_after_ms}ms"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nonstream_backend_timeout_gets_504() {
    let server = TestServer::start(
        &["--misbehave-uids", "0", "--behavior", "drop-overlong"],
        TestConfig {
            backend_timeout_ms: 300,
            ..TestConfig::default()
        },
    )
    .await;

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":10}"#;
    let t0 = Instant::now();
    let resp = tokio::time::timeout(
        Duration::from_millis(1500),
        http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)),
    )
    .await
    .unwrap_or_else(|_| panic!("504 took too long: {:?}", t0.elapsed()));
    assert!(
        t0.elapsed() < Duration::from_millis(1500),
        "504 took too long: {:?}",
        t0.elapsed()
    );
    assert_eq!(resp.status, 504);
    assert_eq!(resp.header("content-type"), Some("application/json"));
    let body_json: serde_json::Value = serde_json::from_slice(&resp.body).expect("json body");
    assert_eq!(
        body_json["error"]["type"].as_str(),
        Some("backend_timeout")
    );

    let observed = wait_for_abort(&server, 0).await;
    let submit_idx = observed
        .iter()
        .position(|o| matches!(o, Observed::Submit { uid: 0, .. }));
    let abort_idx = observed
        .iter()
        .position(|o| matches!(o, Observed::Abort { uid: 0 }));
    assert!(
        submit_idx.is_some() && abort_idx.is_some() && submit_idx < abort_idx,
        "expected Submit {{ uid: 0 }} before Abort {{ uid: 0 }}: {observed:?}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);
    assert_eq!(snap.invalid_transitions, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_overlong_gets_400_in_both_modes() {
    let server = TestServer::start(&["--max-seq-len", "16"], TestConfig::default()).await;

    // ByteCodec renders a chat message as "{role}: {content}\n"; "user: "
    // is 6 bytes, so a 9-byte content plus the trailing "\n" makes exactly
    // 16 bytes == max_seq_len, which is >= the limit (overlong).
    let content = "123456789";
    assert_eq!(format!("user: {content}\n").len(), 16);

    let body_nonstream = format!(
        r#"{{"model":"m","messages":[{{"role":"user","content":"{content}"}}],"max_tokens":1,"stream":false}}"#
    );
    let resp1 = http_client::send(
        server.addr,
        "POST",
        "/v1/chat/completions",
        Some(body_nonstream.as_bytes()),
    )
    .await;
    assert_eq!(resp1.status, 400);
    let json1: serde_json::Value = serde_json::from_slice(&resp1.body).expect("json body");
    assert_eq!(
        json1["error"]["type"].as_str(),
        Some("invalid_request_error")
    );

    let body_stream = format!(
        r#"{{"model":"m","messages":[{{"role":"user","content":"{content}"}}],"max_tokens":1,"stream":true}}"#
    );
    let resp2 = http_client::send(
        server.addr,
        "POST",
        "/v1/chat/completions",
        Some(body_stream.as_bytes()),
    )
    .await;
    assert_eq!(resp2.status, 400);
    let json2: serde_json::Value = serde_json::from_slice(&resp2.body).expect("json body");
    assert_eq!(
        json2["error"]["type"].as_str(),
        Some("invalid_request_error")
    );
    let body2_str = String::from_utf8_lossy(&resp2.body);
    assert!(
        !body2_str.contains("data:"),
        "the 400 must arrive before any data line: {body2_str}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 2);
    assert!(
        !server
            .mock
            .observed()
            .iter()
            .any(|o| matches!(o, Observed::Submit { .. })),
        "overlong prompt must never reach the backend"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_stream_timeout_ends_without_stop_chunk_or_done() {
    let server = TestServer::start(
        &["--misbehave-uids", "0", "--behavior", "drop-overlong"],
        TestConfig {
            backend_timeout_ms: 300,
            ..TestConfig::default()
        },
    )
    .await;

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":10,"stream":true}"#;
    let stream = OpenStream::open(server.addr, "POST", "/v1/chat/completions", Some(body)).await;
    assert_eq!(stream.status, 200);

    let resp = stream.read_to_end(Duration::from_millis(1500)).await;
    assert!(!resp.complete, "stream must not end cleanly");
    let body_str = String::from_utf8_lossy(&resp.body);
    assert!(
        !body_str.contains("\"finish_reason\": \"stop\""),
        "must not contain a stop chunk: {body_str}"
    );
    assert!(
        !body_str.contains("data: [DONE]"),
        "must not contain data: [DONE]: {body_str}"
    );

    wait_for_abort(&server, 0).await;
    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.failed, 1);
    assert_eq!(snap.invalid_transitions, 0);
}
