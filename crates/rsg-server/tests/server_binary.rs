//! Binary-level tests of the real `rsg-server` process: Task 1's tracer
//! proves the full HTTP -> real tokenizer -> engine -> writer ->
//! mock-scheduler -> dispatcher -> real detokenizer -> body-stream path,
//! gated on the readiness handshake, exactly as the launcher will drive it
//! on the GPU box. Task 2 adds CLI validation, bind-failure and
//! flag-wiring coverage on top.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use common::MockScheduler;
use common::http_client;
use common::rsg_process::RsgServer;

use rsg_server::codec::Prompt;
use rsg_server::codec::TextCodec;
use rsg_server::hf_codec::HfCodec;

const QWEN3: &str = "Qwen/Qwen3-0.6B";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_binary_serves_generate_through_real_tokenizer() {
    let mut mock = MockScheduler::spawn(&["--decode-delay-ms", "1"]);
    mock.wait_ready();
    let handshake_line = mock
        .handshake_line()
        .expect("mock-scheduler's handshake line is available after wait_ready");

    let mut server = RsgServer::spawn(&[
        "--backend-addr",
        &mock.backend_addr,
        "--backend-role",
        "connect",
        "--detok-addr",
        &mock.detok_addr,
        "--detok-role",
        "bind",
        "--model",
        QWEN3,
        "--run-id",
        ".rsg=bin",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
    ]);

    let addr = server.listening_addr();

    // Before the handshake: /health is up, /health/ready and the
    // generation endpoints are not.
    let health = http_client::send(addr, "GET", "/health", None).await;
    assert_eq!(health.status, 200);
    let ready = http_client::send(addr, "GET", "/health/ready", None).await;
    assert_eq!(ready.status, 503);
    let gen_before =
        http_client::send(addr, "POST", "/generate", Some(br#"{"prompt":"x","max_tokens":1}"#))
            .await;
    assert_eq!(gen_before.status, 503);

    server.send_line(&handshake_line);

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let ready = http_client::send(addr, "GET", "/health/ready", None).await;
        if ready.status == 200 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for /health/ready; stderr:\n{}",
            server.stderr()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let codec = HfCodec::load(QWEN3).expect("load Qwen3-0.6B (network/HF cache required)");
    let ids = codec
        .encode(&Prompt::Text("Hello world".to_string()))
        .expect("encode \"Hello world\"");
    let n = ids.len();

    let body = format!(r#"{{"prompt":"Hello world","max_tokens":{n}}}"#);
    let resp = http_client::send(addr, "POST", "/generate", Some(body.as_bytes())).await;
    assert_eq!(resp.status, 200, "stderr:\n{}", server.stderr());
    assert!(resp.complete, "response body did not end cleanly");

    let body_str = String::from_utf8(resp.body).expect("response body is valid UTF-8");
    assert!(
        body_str.ends_with("data: [DONE]\n"),
        "response must end with data: [DONE]\\n: {body_str:?}"
    );
    let text: String = body_str
        .strip_suffix("data: [DONE]\n")
        .expect("checked above")
        .lines()
        .map(|line| line.strip_prefix("data: ").unwrap_or(line))
        .collect();
    assert_eq!(text, "Hello world");

    server.signal("-TERM");
    assert_eq!(server.wait_exit(), 0, "stderr:\n{}", server.stderr());
}
