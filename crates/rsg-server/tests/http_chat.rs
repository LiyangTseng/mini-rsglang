//! Integration tests for `POST /v1/chat/completions`: Task 1's tracer
//! proves the full streaming path end to end with upstream's exact chunk
//! bytes. Task 2 adds the non-streaming and edge-case tests on top
//! (API-01).

#[allow(dead_code)]
mod common;

use common::http_client;
use common::test_server::{TestConfig, TestServer};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_chat_stream_matches_upstream_framing() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":4,"stream":true}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.header("content-type"),
        Some("text/event-stream; charset=utf-8")
    );
    assert!(resp.complete, "response body did not end cleanly");

    let expected = concat!(
        "data: {\"id\": \"cmpl-0\", \"object\": \"text_completion.chunk\", \"choices\": [{\"delta\": {\"role\": \"assistant\", \"content\": \"u\"}, \"index\": 0, \"finish_reason\": null}]}\n\n",
        "data: {\"id\": \"cmpl-0\", \"object\": \"text_completion.chunk\", \"choices\": [{\"delta\": {\"content\": \"s\"}, \"index\": 0, \"finish_reason\": null}]}\n\n",
        "data: {\"id\": \"cmpl-0\", \"object\": \"text_completion.chunk\", \"choices\": [{\"delta\": {\"content\": \"e\"}, \"index\": 0, \"finish_reason\": null}]}\n\n",
        "data: {\"id\": \"cmpl-0\", \"object\": \"text_completion.chunk\", \"choices\": [{\"delta\": {\"content\": \"r\"}, \"index\": 0, \"finish_reason\": null}]}\n\n",
        "data: {\"id\": \"cmpl-0\", \"object\": \"text_completion.chunk\", \"choices\": [{\"delta\": {}, \"index\": 0, \"finish_reason\": \"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    assert_eq!(resp.body, expected.as_bytes());
}
