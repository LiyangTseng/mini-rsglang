//! Integration tests for `POST /generate`: Task 1's tracer proves the full
//! HTTP -> engine driver -> writer -> mock-scheduler -> dispatcher ->
//! decoder -> body-stream path end to end. Task 2 adds the
//! lifecycle/edge-case tests on top (LIFE-01).

#[allow(dead_code)]
mod common;

use common::Observed;
use common::http_client;
use common::test_server::{TestConfig, TestServer};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_generate_streams_echo_tokens_end_to_end() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "5", "--decode-delay-ms", "1"],
        TestConfig::default(),
    )
    .await;

    let body = br#"{"prompt":"abc","max_tokens":5}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.header("content-type"),
        Some("text/event-stream; charset=utf-8")
    );
    assert!(resp.complete, "response body did not end cleanly");
    assert_eq!(
        resp.body,
        b"data: a\ndata: b\ndata: c\ndata: a\ndata: b\ndata: [DONE]\n"
    );

    let observed = server.mock.observed();
    assert!(
        observed
            .iter()
            .any(|o| matches!(o, Observed::Submit { uid: 0, input_len: 3 })),
        "{observed:?}"
    );
    assert!(
        !observed.iter().any(|o| matches!(o, Observed::Abort { .. })),
        "{observed:?}"
    );
}
