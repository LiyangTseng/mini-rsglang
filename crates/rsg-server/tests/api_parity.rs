//! API-01 parity (D-02): replays every case recorded from a live run of
//! upstream's frozen Python frontend (`fixtures/api/manifest.json`, plan
//! 05-05) against the real `rsg-server` binary on `mock-scheduler`, in the
//! same fixed order, and requires byte-identical status/content-type/body
//! (after the generator's own `created` normalization). The captured Python
//! behavior is the specification: any divergence is a Rust bug, fixed in
//! `src/http/`, never papered over by widening this test's comparison.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use common::MockScheduler;
use common::http_client;
use common::rsg_process::RsgServer;

/// `fixtures/api/`, resolved relative to this crate (two levels up from
/// `crates/rsg-server`).
const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/api");

/// How far "created"'s original value may be from the test's own clock
/// before it's treated as implausible rather than normalized away --
/// mirrors `scripts/gen_api_fixtures.py`'s `_MAX_CREATED_SKEW_S`.
const MAX_CREATED_SKEW_S: i64 = 86_400;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Finds the first byte offset of `needle` in `haystack` at or after `from`.
fn find_from(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Mirrors `scripts/gen_api_fixtures.py`'s `normalize_created`: replaces the
/// single `"created":<digits>` occurrence with `"created":0`, returning the
/// original integer value. `Err` names the problem instead of silently
/// widening the comparison (there must be exactly one occurrence).
fn normalize_created(body: &[u8]) -> Result<(Vec<u8>, i64), String> {
    const NEEDLE: &[u8] = b"\"created\":";
    let mut occurrences: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while let Some(pos) = find_from(body, NEEDLE, i) {
        let digit_start = pos + NEEDLE.len();
        let mut digit_end = digit_start;
        while digit_end < body.len() && body[digit_end].is_ascii_digit() {
            digit_end += 1;
        }
        occurrences.push((pos, digit_start, digit_end));
        i = digit_end.max(pos + 1);
    }
    if occurrences.len() != 1 {
        return Err(format!(
            "expected exactly one \"created\":<digits> occurrence, found {}",
            occurrences.len()
        ));
    }
    let (pos, digit_start, digit_end) = occurrences[0];
    let original: i64 = std::str::from_utf8(&body[digit_start..digit_end])
        .map_err(|e| format!("non-UTF8 created value: {e}"))?
        .parse()
        .map_err(|e| format!("non-numeric created value: {e}"))?;
    let mut normalized = Vec::with_capacity(body.len());
    normalized.extend_from_slice(&body[..pos]);
    normalized.extend_from_slice(b"\"created\":0");
    normalized.extend_from_slice(&body[digit_end..]);
    Ok((normalized, original))
}

/// Renders `bytes` for a mismatch message: lossy UTF-8 decode, then
/// `escape_debug` so control/non-printable bytes are visible rather than
/// corrupting the test output.
fn render(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).escape_debug().to_string()
}

/// Describes a body mismatch: the first differing byte offset, with up to 80
/// bytes of context (post-normalization) on each side.
fn describe_body_diff(expected: &[u8], actual: &[u8]) -> String {
    let common_len = expected.len().min(actual.len());
    let mut offset = 0;
    while offset < common_len && expected[offset] == actual[offset] {
        offset += 1;
    }
    const CTX: usize = 80;
    let start = offset.saturating_sub(CTX);
    let exp_end = (offset + CTX).min(expected.len());
    let act_end = (offset + CTX).min(actual.len());
    format!(
        "body mismatch at byte offset {offset} (expected len {elen}, actual len {alen})\n    expected: {exp}\n    actual:   {act}",
        elen = expected.len(),
        alen = actual.len(),
        exp = render(&expected[start..exp_end]),
        act = render(&actual[start..act_end]),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api_parity_matches_python_frontend_fixtures() {
    let manifest_text = std::fs::read_to_string(format!("{FIXTURES_DIR}/manifest.json"))
        .expect("read fixtures/api/manifest.json");
    let manifest: Value =
        serde_json::from_str(&manifest_text).expect("manifest.json is valid JSON");

    let model = manifest["model"]
        .as_str()
        .expect("manifest.model is a string");
    let mock_args: Vec<String> = manifest["mock_args"]
        .as_array()
        .expect("manifest.mock_args is an array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("mock_args entries are strings")
                .to_string()
        })
        .collect();
    let mock_args_ref: Vec<&str> = mock_args.iter().map(String::as_str).collect();

    let cases = manifest["cases"]
        .as_array()
        .expect("manifest.cases is an array");
    assert_eq!(
        cases.len(),
        18,
        "manifest case count changed; this test (and the fixtures it replays) must be updated"
    );

    let mut mock = MockScheduler::spawn(&mock_args_ref);
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
        model,
        "--run-id",
        ".rsg=api-parity",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
    ]);
    let addr = server.listening_addr();
    server.send_line(&handshake_line);

    let ready_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let ready = http_client::send(addr, "GET", "/health/ready", None).await;
        if ready.status == 200 {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "timed out waiting for /health/ready; stderr:\n{}",
            server.stderr()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let mut mismatches: Vec<String> = Vec::new();

    for case in cases {
        let name = case["name"].as_str().expect("case.name is a string");
        let method = case["method"].as_str().expect("case.method is a string");
        let path = case["path"].as_str().expect("case.path is a string");
        let expected_status = case["status"].as_u64().expect("case.status is a number") as u16;
        let compare = case["compare"].as_str().expect("case.compare is a string");
        let expected_content_type = case["content_type"].as_str();
        let normalize: Vec<&str> = case["normalize"]
            .as_array()
            .expect("case.normalize is an array")
            .iter()
            .map(|v| v.as_str().expect("normalize entries are strings"))
            .collect();
        let request_body = case["request_body"].as_str().map(|s| s.as_bytes().to_vec());

        let resp = http_client::send(addr, method, path, request_body.as_deref()).await;

        if resp.status != expected_status {
            mismatches.push(format!(
                "{name}: status expected {expected_status}, got {}; body={:?}; stderr:\n{}",
                resp.status,
                render(&resp.body),
                server.stderr()
            ));
            continue;
        }

        if compare != "bytes" {
            continue;
        }

        let actual_content_type = resp.header("content-type");
        if actual_content_type != expected_content_type {
            mismatches.push(format!(
                "{name}: content-type expected {expected_content_type:?}, got {actual_content_type:?}"
            ));
            continue;
        }

        let expected_body = std::fs::read(format!("{FIXTURES_DIR}/{name}.body"))
            .unwrap_or_else(|e| panic!("read fixtures/api/{name}.body: {e}"));

        let actual_body = if normalize.contains(&"created") {
            match normalize_created(&resp.body) {
                Ok((normalized, original)) => {
                    let skew = (original - now_unix()).abs();
                    if skew > MAX_CREATED_SKEW_S {
                        mismatches.push(format!(
                            "{name}: \"created\":{original} is {skew}s from the test's clock, refusing to normalize"
                        ));
                        continue;
                    }
                    normalized
                }
                Err(e) => {
                    mismatches.push(format!("{name}: {e}"));
                    continue;
                }
            }
        } else {
            resp.body.clone()
        };

        if actual_body != expected_body {
            mismatches.push(format!(
                "{name}: {}",
                describe_body_diff(&expected_body, &actual_body)
            ));
        }
    }

    for mismatch in &mismatches {
        println!("MISMATCH {mismatch}");
    }

    server.signal("-TERM");
    let _ = server.wait_exit();

    assert!(
        mismatches.is_empty(),
        "{} of {} case(s) mismatched; see output above",
        mismatches.len(),
        cases.len()
    );
}
