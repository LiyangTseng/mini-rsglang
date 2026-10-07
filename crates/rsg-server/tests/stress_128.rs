//! D-04: a minimal, Phase-5-only 128-agent concurrent-cancellation stress
//! test proving LIFE-03 — no leaked requests, no stuck connections, and
//! every request reaches exactly one terminal state, even under races
//! between finish and cancellation. This is a correctness check against
//! the Mac mock, never performance evidence (see the module-level
//! `<prohibitions>` in the plan this file implements): Phase 7 builds its
//! own instrumented load generator, unconstrained by this file.

#[allow(dead_code)]
mod common;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::task::JoinSet;

use common::Observed;
use common::http_client::{self, OpenStream};
use common::test_server::{TestConfig, TestServer};

/// Fixed seed for the test's own splitmix64 PRNG (reproducible by default);
/// override with `RSG_STRESS_SEED=<u64>` to replay a specific failure.
const STRESS_SEED: u64 = 0x5EED_0005;

const AGENTS: usize = 128;
const REQUESTS_PER_AGENT: usize = 2;

/// A small, fast, test-local PRNG (SplitMix64) — no new crate, no
/// dependency on any system/thread RNG, fully reproducible from one u64
/// seed.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform value in `0..bound`. Returns 0 when `bound == 0`.
    fn next_range(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }

    /// A uniform `f64` in `[0, 1)`.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endpoint {
    Generate,
    ChatStream,
    ChatNonStream,
}

/// What a request does after it is sent. `DisconnectAfterK`'s `k` is read
/// as a data-chunk count for the two streaming endpoints, and `non_stream_ms`
/// is used instead for `ChatNonStream` (a non-streaming response has no
/// chunks to count before it is fully ready).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Complete,
    DisconnectAfterK { k: usize, non_stream_ms: u64 },
    DisconnectImmediate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestOutcome {
    Completed,
    DisconnectedAfterK,
    DisconnectedImmediate,
}

#[derive(Clone, Debug)]
struct RequestPlan {
    agent: usize,
    req_idx: usize,
    endpoint: Endpoint,
    max_tokens: i64,
    think_time_ms: u64,
    mode: Mode,
}

/// The expected echo for a request: its rendered prompt bytes (the prompt
/// itself for `/generate`, `"user: {prompt}\n"` for chat — matching
/// `ByteCodec`, `tests/common/test_server.rs`), cycled to `max_tokens`.
fn expected_echo(endpoint: Endpoint, prompt: &str, max_tokens: i64) -> Vec<u8> {
    let rendered: Vec<u8> = match endpoint {
        Endpoint::Generate => prompt.as_bytes().to_vec(),
        Endpoint::ChatStream | Endpoint::ChatNonStream => format!("user: {prompt}\n").into_bytes(),
    };
    let n = max_tokens.max(0) as usize;
    (0..n).map(|k| rendered[k % rendered.len()]).collect()
}

/// Parses a complete `/generate` response body (`data: <char>\n` lines,
/// then `data: [DONE]\n`) into the concatenated echoed bytes and whether
/// the terminator was seen.
fn parse_generate_complete(body: &[u8]) -> (Vec<u8>, bool) {
    let s = std::str::from_utf8(body).expect("utf8 body");
    let mut echoed = Vec::new();
    let mut saw_done = false;
    for line in s.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix("data: ") else {
            continue;
        };
        if rest == "[DONE]" {
            saw_done = true;
        } else {
            echoed.extend_from_slice(rest.as_bytes());
        }
    }
    (echoed, saw_done)
}

/// Parses a complete streaming `/v1/chat/completions` response body
/// (`data: <json>\n\n` chunks, then `data: [DONE]\n\n`) into the
/// concatenated delta content, whether the stop chunk was seen, whether
/// `[DONE]` was seen, and every chunk's `id` (asserted identical).
fn parse_chat_stream_complete(body: &[u8]) -> (String, bool, bool, String) {
    let s = std::str::from_utf8(body).expect("utf8 body");
    let mut echoed = String::new();
    let mut saw_stop = false;
    let mut saw_done = false;
    let mut id: Option<String> = None;
    for chunk in s.split("\n\n") {
        if chunk.is_empty() {
            continue;
        }
        let Some(rest) = chunk.strip_prefix("data: ") else {
            continue;
        };
        if rest == "[DONE]" {
            saw_done = true;
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(rest).expect("chunk json");
        let this_id = v["id"].as_str().unwrap_or_default().to_string();
        match &id {
            None => id = Some(this_id),
            Some(prev) => assert_eq!(*prev, this_id, "chunk id changed mid-stream: {s}"),
        }
        if let Some(c) = v["choices"][0]["delta"]["content"].as_str() {
            echoed.push_str(c);
        }
        if v["choices"][0]["finish_reason"].as_str() == Some("stop") {
            saw_stop = true;
        }
    }
    (echoed, saw_stop, saw_done, id.unwrap_or_default())
}

/// Writes a raw HTTP/1.1 request onto `stream`: just enough framing
/// (`Content-Length`, `Connection: close`) for `mock-scheduler`/axum to
/// accept it. Errors are ignored — a `DisconnectImmediate`/`DisconnectAfterK`
/// agent may have the peer already gone by the time it writes, which is the
/// point of the test, not a bug in it.
async fn write_raw_request(stream: &mut TcpStream, method: &str, path: &str, body: &[u8]) {
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

/// Runs one planned request to completion (in the `Complete` case,
/// including full response verification) or to its planned disconnect
/// point, returning what happened. Any assertion failure here panics the
/// agent's task, which fails the whole test (JoinSet surfaces it as a
/// `JoinError`).
async fn execute_request(addr: SocketAddr, plan: &RequestPlan) -> RequestOutcome {
    let path = match plan.endpoint {
        Endpoint::Generate => "/generate",
        Endpoint::ChatStream | Endpoint::ChatNonStream => "/v1/chat/completions",
    };
    let prompt = format!("a{}-r{}-x", plan.agent, plan.req_idx);
    let body = match plan.endpoint {
        Endpoint::Generate => format!(
            r#"{{"prompt":"{prompt}","max_tokens":{}}}"#,
            plan.max_tokens
        ),
        Endpoint::ChatStream => format!(
            r#"{{"model":"m","messages":[{{"role":"user","content":"{prompt}"}}],"max_tokens":{},"stream":true}}"#,
            plan.max_tokens
        ),
        Endpoint::ChatNonStream => format!(
            r#"{{"model":"m","messages":[{{"role":"user","content":"{prompt}"}}],"max_tokens":{},"stream":false}}"#,
            plan.max_tokens
        ),
    };

    tokio::time::sleep(Duration::from_millis(plan.think_time_ms)).await;

    match plan.mode {
        Mode::DisconnectImmediate => {
            let mut stream = TcpStream::connect(addr).await.expect("connect");
            write_raw_request(&mut stream, "POST", path, body.as_bytes()).await;
            drop(stream);
            RequestOutcome::DisconnectedImmediate
        }
        Mode::DisconnectAfterK { k, non_stream_ms } => match plan.endpoint {
            Endpoint::ChatNonStream => {
                let mut stream = TcpStream::connect(addr).await.expect("connect");
                write_raw_request(&mut stream, "POST", path, body.as_bytes()).await;
                tokio::time::sleep(Duration::from_millis(non_stream_ms)).await;
                drop(stream);
                RequestOutcome::DisconnectedAfterK
            }
            _ => {
                let mut os = OpenStream::open(addr, "POST", path, Some(body.as_bytes())).await;
                assert_eq!(
                    os.status, 200,
                    "expected 200 before disconnecting mid-stream ({:?} a{}-r{})",
                    plan.endpoint, plan.agent, plan.req_idx
                );
                for _ in 0..k {
                    let _ = os.next_chunk(Duration::from_secs(2)).await;
                }
                drop(os);
                RequestOutcome::DisconnectedAfterK
            }
        },
        Mode::Complete => {
            let resp = http_client::send(addr, "POST", path, Some(body.as_bytes())).await;
            assert_eq!(
                resp.status,
                200,
                "expected 200 for a complete-mode request ({:?} a{}-r{}): {:?}",
                plan.endpoint,
                plan.agent,
                plan.req_idx,
                String::from_utf8_lossy(&resp.body)
            );
            assert!(resp.complete, "response body did not end cleanly");
            let expected = expected_echo(plan.endpoint, &prompt, plan.max_tokens);
            match plan.endpoint {
                Endpoint::Generate => {
                    let (echoed, saw_done) = parse_generate_complete(&resp.body);
                    assert!(
                        saw_done,
                        "missing data: [DONE] for a{}-r{}: {:?}",
                        plan.agent,
                        plan.req_idx,
                        String::from_utf8_lossy(&resp.body)
                    );
                    assert_eq!(
                        echoed, expected,
                        "echo mismatch for /generate a{}-r{}",
                        plan.agent, plan.req_idx
                    );
                }
                Endpoint::ChatStream => {
                    let (echoed, saw_stop, saw_done, _id) = parse_chat_stream_complete(&resp.body);
                    assert!(
                        saw_stop && saw_done,
                        "missing stop chunk or [DONE] for a{}-r{}: {:?}",
                        plan.agent,
                        plan.req_idx,
                        String::from_utf8_lossy(&resp.body)
                    );
                    assert_eq!(
                        echoed.as_bytes(),
                        expected.as_slice(),
                        "echo mismatch for chat stream a{}-r{}",
                        plan.agent,
                        plan.req_idx
                    );
                }
                Endpoint::ChatNonStream => {
                    let v: serde_json::Value =
                        serde_json::from_slice(&resp.body).expect("json body");
                    let content = v["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or_default();
                    assert_eq!(
                        content.as_bytes(),
                        expected.as_slice(),
                        "echo mismatch for chat non-stream a{}-r{}",
                        plan.agent,
                        plan.req_idx
                    );
                }
            }
            RequestOutcome::Completed
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn stress_128_concurrent_requests_with_random_cancellations() {
    // 128 client sockets, 128 server sockets and the mock's own fds exceed
    // macOS's default soft limit of 256; scripts/check_all.sh raises it
    // before this test runs. Fail fast with a clear message instead of a
    // confusing "too many open files" error mid-test.
    let ulimit_output = std::process::Command::new("sh")
        .args(["-c", "ulimit -n"])
        .output()
        .expect("run ulimit -n");
    let soft_limit: i64 = String::from_utf8_lossy(&ulimit_output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    assert!(
        soft_limit >= 1024,
        "open-file soft limit {soft_limit} is below 1024; run 'ulimit -n 4096' first (scripts/check_all.sh does this)"
    );

    let seed = std::env::var("RSG_STRESS_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(STRESS_SEED);
    println!("stress_seed={seed:#x}");

    let server = TestServer::start(
        &[
            "--prefill-delay-ms",
            "5",
            "--decode-delay-ms",
            "2",
            "--batch-size",
            "4",
            "--misbehave-uids",
            "0-100000",
            "--behavior",
            "late-abort-token",
        ],
        TestConfig::default(),
    )
    .await;

    let mut rng = SplitMix64::new(seed);
    let mut plans: Vec<RequestPlan> = Vec::with_capacity(AGENTS * REQUESTS_PER_AGENT);
    for agent in 0..AGENTS {
        for req_idx in 0..REQUESTS_PER_AGENT {
            let endpoint = match rng.next_range(3) {
                0 => Endpoint::Generate,
                1 => Endpoint::ChatStream,
                _ => Endpoint::ChatNonStream,
            };
            let max_tokens = 1 + rng.next_range(32) as i64; // 1..=32
            let think_time_ms = rng.next_range(21); // 0..=20
            let mode_roll = rng.next_f64();
            // Always drawn, regardless of which mode is picked, so the
            // PRNG's draw sequence per request is fixed and reproducible.
            let k = rng.next_range(max_tokens as u64) as usize; // 0..max_tokens
            let non_stream_ms = rng.next_range(41); // 0..=40
            let mode = if mode_roll < 0.50 {
                Mode::Complete
            } else if mode_roll < 0.85 {
                Mode::DisconnectAfterK { k, non_stream_ms }
            } else {
                Mode::DisconnectImmediate
            };
            plans.push(RequestPlan {
                agent,
                req_idx,
                endpoint,
                max_tokens,
                think_time_ms,
                mode,
            });
        }
    }
    let total_requests = plans.len();

    let addr = server.addr;
    let mut join_set: JoinSet<Vec<RequestOutcome>> = JoinSet::new();
    for agent in 0..AGENTS {
        let agent_plans: Vec<RequestPlan> =
            plans.iter().filter(|p| p.agent == agent).cloned().collect();
        join_set.spawn(async move {
            let mut outcomes = Vec::with_capacity(agent_plans.len());
            for plan in &agent_plans {
                outcomes.push(execute_request(addr, plan).await);
            }
            outcomes
        });
    }

    let drain = async {
        let mut all_outcomes = Vec::with_capacity(total_requests);
        while let Some(res) = join_set.join_next().await {
            let outcomes = res.expect("agent task panicked (see cause above)");
            all_outcomes.extend(outcomes);
        }
        all_outcomes
    };
    let all_outcomes = tokio::time::timeout(Duration::from_secs(60), drain)
        .await
        .expect("stress test timed out after 60s: a stuck connection or hung agent");
    assert_eq!(all_outcomes.len(), total_requests);

    let completed_count = all_outcomes
        .iter()
        .filter(|o| **o == RequestOutcome::Completed)
        .count();
    let snap = server.snapshot_when_idle(Duration::from_secs(10)).await;
    assert_eq!(snap.active, 0, "leaked request(s): {snap:?}");
    assert_eq!(snap.failed, 0, "unexpected failure(s): {snap:?}");
    assert_eq!(
        snap.invalid_transitions, 0,
        "invalid transition(s): {snap:?}"
    );
    assert_eq!(
        snap.received,
        snap.finished + snap.cancelled,
        "every received request must reach exactly one terminal state: {snap:?}"
    );
    assert!(
        snap.finished as usize >= completed_count,
        "finished ({}) must be at least the completed-mode count ({completed_count}): {snap:?}",
        snap.finished
    );
    // Deliberately NOT `>= completed_count + (a DisconnectedAfterK count)`:
    // a DisconnectAfterK client only *attempts* to reach the server before
    // tearing the connection down -- for Endpoint::ChatNonStream
    // specifically, that's a raw write followed by a sleep of
    // `non_stream_ms` (randomly 0..=40ms, see Mode::DisconnectAfterK's
    // construction above) and then an unconditional drop, with no read-back
    // confirming the server ever accepted the connection at all. Under CI's
    // slower task/connection scheduling (fewer vCPUs than a dev machine;
    // `rsg-server`'s `#[tokio::main]` sizes its worker pool to the host's
    // core count), a low roll of `non_stream_ms` can race the client's own
    // disconnect ahead of the server's accept+parse, so the request never
    // reaches `drive_request`'s first `registry.report(Received)` at all --
    // a legitimate "never arrived" outcome, indistinguishable from the
    // client never connecting, not a leak or a registry bug. Reproduced on
    // CI with stress_seed=0x5eed0005 (received=220, completed=132,
    // disconnect-after-k=92; 220 < 132+92 but >= 132).
    assert!(
        snap.received as usize >= completed_count,
        "received ({}) must be at least completed ({completed_count}): {snap:?}",
        snap.received
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let observed = loop {
        let observed = server.mock.observed();
        let submits = observed
            .iter()
            .filter(|o| matches!(o, Observed::Submit { .. }))
            .count() as u64;
        let aborts = observed
            .iter()
            .filter(|o| matches!(o, Observed::Abort { .. }))
            .count() as u64;
        if submits == snap.finished + aborts {
            break observed;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for #Submit == finished + #Abort; submits={submits} aborts={aborts} finished={}",
            snap.finished
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    let mut submitted: HashSet<i64> = HashSet::new();
    let mut abort_count: u64 = 0;
    for o in &observed {
        match o {
            Observed::Submit { uid, .. } => {
                assert!(
                    submitted.insert(*uid),
                    "uid {uid} submitted twice: {observed:?}"
                );
            }
            Observed::Abort { uid } => {
                assert!(
                    submitted.contains(uid),
                    "abort for uid {uid} with no earlier submit: {observed:?}"
                );
                abort_count += 1;
            }
            Observed::Exit => {}
        }
    }

    // One more request after the storm still completes: the server is not
    // wedged.
    let final_resp = http_client::send(
        server.addr,
        "POST",
        "/generate",
        Some(br#"{"prompt":"after","max_tokens":3}"#),
    )
    .await;
    assert_eq!(final_resp.status, 200);
    assert!(
        final_resp.body.ends_with(b"data: [DONE]\n"),
        "server must still be serving after the storm: {:?}",
        String::from_utf8_lossy(&final_resp.body)
    );

    println!(
        "stress_seed={seed:#x} requests={total_requests} finished={} cancelled={} aborts={abort_count} (correctness check against the Mac mock; not performance evidence)",
        snap.finished, snap.cancelled
    );
}
