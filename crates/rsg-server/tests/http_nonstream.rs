//! The non-streaming disconnect bound D-03 accepts (named as this file's
//! companion measurement in plan 05-04's `AbortGuard` doc comment).

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use common::Observed;
use common::test_server::{TestConfig, TestServer};

fn has_abort(observed: &[Observed], uid: i64) -> bool {
    observed
        .iter()
        .any(|o| matches!(o, Observed::Abort { uid: u } if *u == uid))
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
