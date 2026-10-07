//! Integration tests for `POST /generate`: Task 1's tracer proves the full
//! HTTP -> engine driver -> writer -> mock-scheduler -> dispatcher ->
//! decoder -> body-stream path end to end. Task 2 adds the
//! lifecycle/edge-case tests on top (LIFE-01).

#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::Observed;
use common::http_client;
use common::test_server::{TestConfig, TestServer};

use rsg_server::fsm::RegistrySnapshot;
use rsg_server::http::MAX_REQUEST_BODY_BYTES;

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
        observed.iter().any(|o| matches!(
            o,
            Observed::Submit {
                uid: 0,
                input_len: 3
            }
        )),
        "{observed:?}"
    );
    assert!(
        !observed.iter().any(|o| matches!(o, Observed::Abort { .. })),
        "{observed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lifecycle_counts_one_finished_request() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"prompt":"abc","max_tokens":5}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(body)).await;
    assert_eq!(resp.status, 200);
    assert!(resp.complete);

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(
        snap,
        RegistrySnapshot {
            received: 1,
            active: 0,
            finished: 1,
            cancelled: 0,
            failed: 0,
            invalid_transitions: 0,
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_prompt_is_forwarded_and_finishes() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"prompt":"","max_tokens":2}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert!(resp.complete);
    assert_eq!(resp.body, b"data: \0\ndata: \0\ndata: [DONE]\n");

    let observed = server.mock.observed();
    assert!(
        observed.iter().any(|o| matches!(
            o,
            Observed::Submit {
                uid: 0,
                input_len: 0
            }
        )),
        "{observed:?}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.finished, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_token_request_has_one_data_line() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"prompt":"abc","max_tokens":1}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert!(resp.complete);
    assert_eq!(resp.body, b"data: a\ndata: [DONE]\n");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multibyte_char_split_across_tokens_streams_an_empty_chunk_then_the_char() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = "{\"prompt\":\"é\",\"max_tokens\":2}".as_bytes();
    let resp = http_client::send(server.addr, "POST", "/generate", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert!(resp.complete);
    assert_eq!(resp.body, "data: \ndata: é\ndata: [DONE]\n".as_bytes());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_body_gets_422_and_consumes_no_uid() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let bad_body = br#"{"prompt":"x"}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(bad_body)).await;
    assert_eq!(resp.status, 422);

    let good_body = br#"{"prompt":"abc","max_tokens":1}"#;
    let resp2 = http_client::send(server.addr, "POST", "/generate", Some(good_body)).await;
    assert_eq!(resp2.status, 200);

    let observed = server.mock.observed();
    assert!(
        observed
            .iter()
            .any(|o| matches!(o, Observed::Submit { uid: 0, .. })),
        "{observed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oversized_body_gets_413() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let prompt = "x".repeat(MAX_REQUEST_BODY_BYTES + 1);
    let body = format!("{{\"prompt\":\"{prompt}\",\"max_tokens\":1}}").into_bytes();
    let resp = http_client::send(server.addr, "POST", "/generate", Some(&body)).await;

    assert_eq!(resp.status, 413);

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.received, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn concurrent_requests_get_only_their_own_tokens() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let mut set = tokio::task::JoinSet::new();
    for i in 0..8i32 {
        let addr = server.addr;
        set.spawn(async move {
            let prompt = format!("req-{i}-xyz");
            let body = format!("{{\"prompt\":\"{prompt}\",\"max_tokens\":12}}");
            let resp = http_client::send(addr, "POST", "/generate", Some(body.as_bytes())).await;
            assert_eq!(resp.status, 200, "uid {i}");
            assert!(resp.complete, "uid {i}");

            let ids: Vec<i32> = prompt.bytes().map(i32::from).collect();
            let cycled = common::echo_tokens(&ids, 12);
            let mut expected = String::new();
            for t in &cycled {
                expected.push_str(&format!("data: {}\n", *t as u8 as char));
            }
            expected.push_str("data: [DONE]\n");
            assert_eq!(resp.body, expected.as_bytes(), "uid {i}: prompt {prompt:?}");
        });
    }

    while let Some(res) = set.join_next().await {
        res.expect("task panicked");
    }

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.finished, 8);
    assert_eq!(snap.invalid_transitions, 0);
}
