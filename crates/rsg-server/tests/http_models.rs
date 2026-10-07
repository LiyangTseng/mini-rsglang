//! Integration tests for `/v1/models`, `/v1`, 422 validation and the
//! not-ready response (API-01).

#[allow(dead_code)]
mod common;

use common::http_client;
use common::test_server::{TestConfig, TestServer};

use rsg_server::http::{self, AppState};
use rsg_server::metrics::ServerMetrics;

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
async fn models_list_shape() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let resp = http_client::send(server.addr, "GET", "/v1/models", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.header("content-type"), Some("application/json"));

    let (created, normalized) = extract_created(&resp.body);
    assert_created_recent(created);
    let expected = r#"{"object":"list","data":[{"id":"test-model","object":"model","created":T,"owned_by":"mini-sglang","root":"test-model"}]}"#;
    assert_eq!(normalized, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn v1_root_all_methods() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    for method in ["GET", "POST", "OPTIONS"] {
        let resp = http_client::send(server.addr, method, "/v1", None).await;
        assert_eq!(resp.status, 200, "{method}");
        assert_eq!(
            resp.header("content-type"),
            Some("application/json"),
            "{method}"
        );
        assert_eq!(resp.body, br#"{"status":"ok"}"#, "{method}");
    }

    let resp = http_client::send(server.addr, "HEAD", "/v1", None).await;
    assert_eq!(resp.status, 200);
    assert_eq!(resp.header("content-type"), Some("application/json"));
    assert!(resp.body.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validation_422_cases_consume_no_uid() {
    let server = TestServer::start(&["--decode-delay-ms", "1"], TestConfig::default()).await;

    let bad_role = br#"{"model":"m","messages":[{"role":"tool","content":"x"}]}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(bad_role)).await;
    assert_eq!(resp.status, 422, "role tool");

    let no_model = br#"{"messages":[{"role":"user","content":"x"}]}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(no_model)).await;
    assert_eq!(resp.status, 422, "no model");

    let null_stop = br#"{"model":"m","messages":[{"role":"user","content":"x"}],"stop":null}"#;
    let resp =
        http_client::send(server.addr, "POST", "/v1/chat/completions", Some(null_stop)).await;
    assert_eq!(resp.status, 422, "stop null");

    let no_max_tokens = br#"{"prompt":"x"}"#;
    let resp = http_client::send(server.addr, "POST", "/generate", Some(no_max_tokens)).await;
    assert_eq!(resp.status, 422, "generate no max_tokens");

    let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}],"max_tokens":1,"stream":true}"#;
    let resp = http_client::send(server.addr, "POST", "/v1/chat/completions", Some(body)).await;
    assert_eq!(resp.status, 200);
    let s = String::from_utf8(resp.body).expect("utf8 body");
    assert!(s.contains("\"id\": \"cmpl-0\""), "{s}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn not_ready_routes_return_503() {
    let state = AppState::new("test-model", ServerMetrics::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(http::serve(listener, state));

    let resp = http_client::send(addr, "GET", "/v1/models", None).await;
    assert_eq!(resp.status, 503);
    assert!(
        String::from_utf8_lossy(&resp.body).contains("\"type\":\"not_ready\""),
        "{resp:?}"
    );

    let resp = http_client::send(addr, "GET", "/v1", None).await;
    assert_eq!(resp.status, 503);
    assert!(
        String::from_utf8_lossy(&resp.body).contains("\"type\":\"not_ready\""),
        "{resp:?}"
    );

    let resp = http_client::send(
        addr,
        "POST",
        "/generate",
        Some(br#"{"prompt":"x","max_tokens":1}"#),
    )
    .await;
    assert_eq!(resp.status, 503);
    assert!(
        String::from_utf8_lossy(&resp.body).contains("\"type\":\"not_ready\""),
        "{resp:?}"
    );

    let resp = http_client::send(
        addr,
        "POST",
        "/v1/chat/completions",
        Some(br#"{"model":"m","prompt":"x","max_tokens":1}"#),
    )
    .await;
    assert_eq!(resp.status, 503);
    assert!(
        String::from_utf8_lossy(&resp.body).contains("\"type\":\"not_ready\""),
        "{resp:?}"
    );
}
