//! Binary-level tests of the real `rsg-server` process: Task 1's tracer
//! proves the full HTTP -> real tokenizer -> engine -> writer ->
//! mock-scheduler -> dispatcher -> real detokenizer -> body-stream path,
//! gated on the readiness handshake, exactly as the launcher will drive it
//! on the GPU box. Task 2 adds CLI validation, bind-failure and
//! flag-wiring coverage on top.

#[allow(dead_code)]
mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::MockScheduler;
use common::http_client::{self, OpenStream};
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
    let gen_before = http_client::send(
        addr,
        "POST",
        "/generate",
        Some(br#"{"prompt":"x","max_tokens":1}"#),
    )
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

/// `--abort-timing sometimes` and `--backend-timeout-ms 0` are both clap
/// errors, caught before any socket or tokenizer load is even attempted.
#[test]
fn invalid_cli_values_exit_2() {
    let base = [
        "--backend-addr",
        "ipc:///tmp/rsgb-invalid-cli-0",
        "--backend-role",
        "connect",
        "--detok-addr",
        "ipc:///tmp/rsgb-invalid-cli-1",
        "--detok-role",
        "bind",
        "--model",
        QWEN3,
        "--run-id",
        ".rsg=invalid-cli",
    ];

    let mut bad_abort_timing: Vec<&str> = base.to_vec();
    bad_abort_timing.extend(["--abort-timing", "sometimes"]);
    let status = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
        .args(&bad_abort_timing)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn rsg-server");
    assert_eq!(
        status.code(),
        Some(2),
        "--abort-timing sometimes: {status:?}"
    );

    let mut bad_backend_timeout: Vec<&str> = base.to_vec();
    bad_backend_timeout.extend(["--backend-timeout-ms", "0"]);
    let status = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
        .args(&bad_backend_timeout)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn rsg-server");
    assert_eq!(status.code(), Some(2), "--backend-timeout-ms 0: {status:?}");
}

/// Binding a port already held by another listener exits 1 (EXIT_STARTUP),
/// with the bind failure named in stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn port_in_use_exits_1() {
    let sentinel =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind sentinel port for the test");
    let port = sentinel.local_addr().expect("local_addr").port();

    let mut server = RsgServer::spawn(&[
        "--backend-addr",
        "ipc:///tmp/rsgb-port-in-use-0",
        "--backend-role",
        "connect",
        "--detok-addr",
        "ipc:///tmp/rsgb-port-in-use-1",
        "--detok-role",
        "bind",
        "--model",
        QWEN3,
        "--run-id",
        ".rsg=port-in-use",
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
    ]);

    let code = server.wait_exit();
    assert_eq!(code, 1, "stderr:\n{}", server.stderr());
    let stderr = server.stderr().to_lowercase();
    assert!(
        stderr.contains("bind") || stderr.contains("address in use"),
        "stderr should name the bind failure: {}",
        server.stderr()
    );
    drop(sentinel);
}

/// `--abort-timing` and `--backend-timeout-ms` reach the engine: the
/// "ready to serve" log line reflects the effective values, and a
/// `/generate` request against a silently-misbehaving backend ends
/// without `data: [DONE]` well inside the configured timeout.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flags_reach_the_engine() {
    let mut mock = MockScheduler::spawn(&["--misbehave-uids", "0", "--behavior", "drop-overlong"]);
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
        ".rsg=flags",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
        "--abort-timing",
        "deferred",
        "--backend-timeout-ms",
        "300",
    ]);

    let addr = server.listening_addr();
    server.send_line(&handshake_line);

    let ready_line = server.wait_for_log("ready to serve");
    assert!(
        ready_line.contains("abort_timing=Deferred"),
        "{ready_line:?}"
    );
    assert!(
        ready_line.contains("backend_timeout_ms=300"),
        "{ready_line:?}"
    );

    let t0 = Instant::now();
    let stream = OpenStream::open(
        addr,
        "POST",
        "/generate",
        Some(br#"{"prompt":"x","max_tokens":10}"#),
    )
    .await;
    assert_eq!(stream.status, 200);
    let resp = stream.read_to_end(Duration::from_millis(1500)).await;
    assert!(
        !resp.complete,
        "generate stream must not end cleanly within 1500ms (elapsed {:?}); stderr:\n{}",
        t0.elapsed(),
        server.stderr()
    );
    assert!(
        !String::from_utf8_lossy(&resp.body).contains("data: [DONE]"),
        "{:?}",
        String::from_utf8_lossy(&resp.body)
    );

    server.signal("-TERM");
    server.wait_exit();
}
