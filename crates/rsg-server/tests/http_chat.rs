//! Integration tests for `POST /v1/chat/completions`: Task 1's tracer
//! proves the full streaming path end to end with upstream's exact chunk
//! bytes. Task 2 adds the non-streaming and edge-case tests on top
//! (API-01).

#[allow(dead_code)]
mod common;

use common::http_client;
use common::test_server::{TestConfig, TestServer};

/// Finds the `"created":<digits>` field in a compact JSON body, returns the
/// parsed value and the body with that run of digits replaced by `T` (so
/// the rest of the byte-exact comparison is unaffected by the live
/// timestamp).
fn extract_created(body: &[u8]) -> (u64, String) {
    let s = std::str::from_utf8(body).expect("utf8 body");
    let key = "\"created\":";
    let key_start = s.find(key).expect("created field");
    let digits_start = key_start + key.len();
    let digits_len = s[digits_start..].find(',').expect("created delimiter");
    let created: u64 = s[digits_start..digits_start + digits_len]
        .parse()
        .expect("created is a number");
    let mut normalized = String::with_capacity(s.len());
    normalized.push_str(&s[..digits_start]);
    normalized.push('T');
    normalized.push_str(&s[digits_start + digits_len..]);
    (created, normalized)
}

fn assert_created_recent(created: u64) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_secs();
    assert!(
        created.abs_diff(now) <= 5,
        "created {created} not within 5s of {now}"
    );
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_stream_escapes_non_ascii_and_omits_empty_content() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body =
        "{\"model\":\"m\",\"messages\":[{\"role\":\"user\",\"content\":\"\u{e9}\"}],\"max_tokens\":9,\"stream\":true}"
            .as_bytes();
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert!(resp.complete);
    let s = String::from_utf8(resp.body).expect("utf8 body");
    let chunks: Vec<&str> = s.split("\n\n").filter(|c| !c.is_empty()).collect();
    assert_eq!(chunks.len(), 11, "{chunks:?}"); // 9 tokens + stop chunk + [DONE]
    assert!(chunks[6].contains("\"delta\": {}"), "{}", chunks[6]);
    assert!(
        chunks[7].contains("\"delta\": {\"content\": \"\\u00e9\"}"),
        "{}",
        chunks[7]
    );
    assert!(
        chunks[8].contains(r#""delta": {"content": "\n"}"#),
        "{}",
        chunks[8]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_nonstream_response_shape() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":4,"stream":false}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    assert_eq!(resp.header("content-type"), Some("application/json"));
    assert!(resp.complete);

    let (created, normalized) = extract_created(&resp.body);
    assert_created_recent(created);
    let expected = r#"{"id":"chatcmpl-0","object":"chat.completion","created":T,"model":"m","choices":[{"index":0,"message":{"role":"assistant","content":"user"},"finish_reason":"stop"}],"usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}}"#;
    assert_eq!(normalized, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_prompt_text_used_when_no_messages() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"model":"m","prompt":"xyz","max_tokens":3}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    let (_created, normalized) = extract_created(&resp.body);
    assert!(normalized.contains(r#""content":"xyz""#), "{normalized}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_empty_messages_falls_back_to_prompt() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"model":"m","messages":[],"prompt":"ab","max_tokens":2}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    let (_created, normalized) = extract_created(&resp.body);
    assert!(normalized.contains(r#""content":"ab""#), "{normalized}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_missing_prompt_is_500_and_consumes_no_uid() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body = br#"{"model":"m"}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;
    assert_eq!(resp.status, 500);
    let s = String::from_utf8(resp.body).expect("utf8 body");
    assert!(s.contains("internal_error"), "{s}");
    assert!(
        s.contains("Either 'messages' or 'prompt' must be provided"),
        "{s}"
    );

    let body2 =
        br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":1,"stream":true}"#;
    let resp2 = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body2)).await;
    assert_eq!(resp2.status, 200);
    let s2 = String::from_utf8(resp2.body).expect("utf8 body");
    assert!(s2.contains("\"id\": \"cmpl-0\""), "{s2}");

    let snap = server.engine.registry().snapshot().await;
    assert_eq!(snap.received, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_default_max_tokens_is_16() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let body =
        br#"{"model":"m","messages":[{"role":"user","content":"abcdefghijklmnopqrstuvwxyz"}],"stream":true}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;

    assert_eq!(resp.status, 200);
    let s = String::from_utf8(resp.body).expect("utf8 body");
    let count = s.matches("data: ").count();
    assert_eq!(count, 18, "{s}");
    assert!(s.ends_with("data: [DONE]\n\n"), "{s}");
}
