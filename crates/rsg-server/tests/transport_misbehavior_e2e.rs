//! Cross-plan end-to-end proof (03-06): the misbehaving mock (03-03) driving
//! the full transport (03-04's ticket-gated writer, 03-05's counting
//! dispatcher), plus the real `rsg-server` binary accepting the mock's
//! readiness handshake. This file composes the 03-01 through 03-05
//! contracts; it adds no new production code.

#[allow(dead_code)]
mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rsg_server::dispatch::{UidEvent, spawn_dispatcher};
use rsg_server::handshake::EXPECTED_UPSTREAM_SHA;
use rsg_server::writer::spawn_writer;
use rsg_wire::{SamplingParams, Tensor};

use common::{MockScheduler, Observed, echo_tokens};

// --- Task 1: tracer — late tokens after an abort travel the full transport
// and are dropped and counted, while another uid streams on. ---

#[test]
fn tracer_late_tokens_after_abort_are_dropped_and_counted() {
    let mut mock = MockScheduler::spawn(&[
        "--decode-delay-ms",
        "20",
        "--misbehave-uids",
        "5",
        "--behavior",
        "late-abort-token",
    ]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatch = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build runtime");

    let (uid5_tail, uid6_events) = rt.block_on(async {
        let mut stream5 = dispatch.register(5);
        let mut stream6 = dispatch.register(6);

        // uid 6's consumer drains continuously in its own task so its
        // 16-entry channel never lags while uid 5 is being aborted.
        let consumer6 = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = stream6.recv().await {
                events.push(event);
            }
            events
        });

        let t5 = writer
            .submit(
                5,
                Tensor::from_i32_slice(&[51, 52, 53]),
                SamplingParams {
                    max_tokens: 1000,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit 5");
        let t6 = writer
            .submit(
                6,
                Tensor::from_i32_slice(&[61, 62, 63]),
                SamplingParams {
                    max_tokens: 1000,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit 6");

        // Wait for uid 5's first token before aborting.
        let event = tokio::time::timeout(Duration::from_secs(5), stream5.recv())
            .await
            .expect("timed out waiting for uid 5's first token")
            .expect("uid 5's stream ended before its first token");
        match event {
            UidEvent::Token(reply) => assert_eq!(reply.uid, 5),
            UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens for uid 5"),
        }

        dispatch.deregister(5);
        writer.abort(&t5).await.expect("abort 5");

        // Poll stats until unknown_uid >= 3 (2s deadline), checking this
        // bound now, before uid 6 is aborted, because uid 6's own in-flight
        // tokens after its own deregister would also count as unknown.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if dispatch.stats().unknown_uid >= 3 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for unknown_uid >= 3; stats: {:?}",
                dispatch.stats()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let unknown_after_abort5 = dispatch.stats().unknown_uid;
        assert!(
            (3..=4).contains(&unknown_after_abort5),
            "expected unknown_uid in 3..=4 (3 late tokens plus at most one already in \
             flight), got {unknown_after_abort5}"
        );

        // Drain uid 5's stream until it ends; no event in it is finished.
        let mut uid5_tail = Vec::new();
        while let Some(event) = tokio::time::timeout(Duration::from_secs(1), stream5.recv())
            .await
            .expect("timed out draining uid 5's stream")
        {
            if let UidEvent::Token(reply) = event {
                assert!(!reply.finished, "uid 5's stream must never finish");
            }
            uid5_tail.push(event);
        }

        writer.abort(&t6).await.expect("abort 6");
        dispatch.deregister(6);
        let uid6_events = consumer6.await.expect("uid 6 consumer task panicked");

        writer.exit().await.expect("exit");

        (uid5_tail, uid6_events)
    });

    assert_eq!(mock.wait_exit(), 0);
    assert_eq!(
        mock.observed(),
        vec![
            Observed::Submit {
                uid: 5,
                input_len: 3
            },
            Observed::Submit {
                uid: 6,
                input_len: 3
            },
            Observed::Abort { uid: 5 },
            Observed::Abort { uid: 6 },
            Observed::Exit,
        ]
    );
    assert!(
        mock.stderr().contains("late-abort-token"),
        "stderr missing late-abort-token: {}",
        mock.stderr()
    );

    // uid 5's drained tail: no finished token (already asserted above while
    // draining), and no panic occurred.
    let _ = uid5_tail;

    // uid 6's events: no Dropped, no finished token, at least 5 Tokens, and
    // equal to the echo_tokens prefix for however many tokens it received.
    assert!(
        !uid6_events
            .iter()
            .any(|e| matches!(e, UidEvent::Dropped(_))),
        "uid 6 must never see a Dropped event: {uid6_events:?}"
    );
    assert!(
        !uid6_events
            .iter()
            .any(|e| matches!(e, UidEvent::Token(r) if r.finished)),
        "uid 6 must never see a finished token: {uid6_events:?}"
    );
    let uid6_tokens: Vec<i64> = uid6_events
        .iter()
        .map(|e| match e {
            UidEvent::Token(r) => r.next_token,
            UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens for uid 6"),
        })
        .collect();
    assert!(
        uid6_tokens.len() >= 5,
        "uid 6 should have kept streaming throughout uid 5's abort, got {} tokens",
        uid6_tokens.len()
    );
    assert_eq!(
        uid6_tokens,
        echo_tokens(&[61, 62, 63], uid6_tokens.len()),
        "uid 6's tokens must equal the echo_tokens prefix"
    );

    assert_eq!(
        dispatch.stats().malformed_frames,
        0,
        "no malformed frames expected"
    );
}

// --- Task 2: batched replies routed per uid, silent uids that stall
// nobody, and rsg-server accepting the mock's handshake. ---

/// `--batch-size 4`: four requests each receive exactly their own 10 echo
/// tokens in order through the dispatcher's `BatchTokenizerMsg` unwrapping.
#[test]
fn batched_replies_are_routed_to_each_uid() {
    let mut mock = MockScheduler::spawn(&["--batch-size", "4", "--decode-delay-ms", "2"]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatch = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build runtime");

    rt.block_on(async {
        let mut streams = Vec::new();
        for uid in 1..=4i64 {
            streams.push((uid, dispatch.register(uid)));
            writer
                .submit(
                    uid,
                    Tensor::from_i32_slice(&[(uid * 10) as i32, (uid * 10 + 1) as i32]),
                    SamplingParams {
                        max_tokens: 10,
                        ..SamplingParams::default()
                    },
                )
                .await
                .expect("submit");
        }

        let mut tasks = Vec::new();
        for (uid, mut stream) in streams {
            tasks.push(tokio::spawn(async move {
                let mut tokens = Vec::new();
                loop {
                    let event = tokio::time::timeout(Duration::from_secs(10), stream.recv())
                        .await
                        .expect("timed out waiting for token")
                        .expect("stream ended before finished");
                    match event {
                        UidEvent::Token(reply) => {
                            let finished = reply.finished;
                            tokens.push(reply.next_token);
                            if finished {
                                break;
                            }
                        }
                        UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens for {uid}"),
                    }
                }
                (uid, tokens)
            }));
        }

        for task in tasks {
            let (uid, tokens) = task.await.expect("consumer task panicked");
            assert_eq!(
                tokens.len(),
                10,
                "uid {uid} should get exactly 10 echo tokens"
            );
            assert_eq!(
                tokens,
                echo_tokens(&[(uid * 10) as i32, (uid * 10 + 1) as i32], 10),
                "uid {uid}'s tokens must match the echo rule"
            );
        }

        writer.exit().await.expect("exit");
    });

    let final_stats = dispatch.stats();
    assert_eq!(final_stats.routed, 40, "stats: {final_stats:?}");
    assert_eq!(final_stats.unknown_uid, 0, "stats: {final_stats:?}");
    assert_eq!(final_stats.malformed_frames, 0, "stats: {final_stats:?}");

    assert_eq!(mock.wait_exit(), 0);
}

/// A uid flagged `drop-overlong` and a 64-token prompt at `--max-seq-len 64`
/// both get no reply within 500ms, while a normal request submitted
/// alongside them completes. The transport never stalls on a silent uid.
#[test]
fn silent_uids_do_not_stall_other_requests() {
    let mut mock = MockScheduler::spawn(&[
        "--max-seq-len",
        "64",
        "--misbehave-uids",
        "9",
        "--behavior",
        "drop-overlong",
        "--decode-delay-ms",
        "2",
    ]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatch = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build runtime");

    rt.block_on(async {
        let mut stream9 = dispatch.register(9);
        let mut stream10 = dispatch.register(10);
        let mut stream11 = dispatch.register(11);

        writer
            .submit(
                9,
                Tensor::from_i32_slice(&[1, 2, 3]),
                SamplingParams {
                    max_tokens: 5,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit 9");
        writer
            .submit(
                10,
                Tensor::from_i32_slice(&[4, 5, 6]),
                SamplingParams {
                    max_tokens: 5,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit 10");
        let ids11: Vec<i32> = (0..64).collect();
        writer
            .submit(
                11,
                Tensor::from_i32_slice(&ids11),
                SamplingParams {
                    max_tokens: 5,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit 11");

        // uid 10 receives its 5 echo tokens, finished last, within 5s.
        let mut tokens10 = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), stream10.recv())
                .await
                .expect("timed out waiting for uid 10's token")
                .expect("uid 10's stream ended before finished");
            match event {
                UidEvent::Token(reply) => {
                    let finished = reply.finished;
                    tokens10.push(reply.next_token);
                    if finished {
                        break;
                    }
                }
                UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens for uid 10"),
            }
        }
        assert_eq!(tokens10.len(), 5, "uid 10 should get exactly 5 tokens");
        assert_eq!(tokens10, echo_tokens(&[4, 5, 6], 5));

        // uid 9 and uid 11 both elapse a 500ms timeout with no event.
        let elapsed9 = tokio::time::timeout(Duration::from_millis(500), stream9.recv()).await;
        assert!(
            elapsed9.is_err(),
            "uid 9 (drop-overlong) should get no reply within 500ms"
        );
        let elapsed11 = tokio::time::timeout(Duration::from_millis(500), stream11.recv()).await;
        assert!(
            elapsed11.is_err(),
            "uid 11 (>= max-seq-len) should get no reply within 500ms"
        );

        assert_eq!(dispatch.stats().unknown_uid, 0);

        writer.exit().await.expect("exit");
    });

    assert_eq!(mock.wait_exit(), 0);
    let observed = mock.observed();
    assert!(
        observed.contains(&Observed::Submit {
            uid: 9,
            input_len: 3
        }),
        "observed: {observed:?}"
    );
    assert!(
        observed.contains(&Observed::Submit {
            uid: 10,
            input_len: 3
        }),
        "observed: {observed:?}"
    );
    assert!(
        observed.contains(&Observed::Submit {
            uid: 11,
            input_len: 64
        }),
        "observed: {observed:?}"
    );
    assert_eq!(
        observed.last(),
        Some(&Observed::Exit),
        "observed: {observed:?}"
    );
}

/// A small test-local helper for spawning the real `rsg-server` binary,
/// modeled on `tests/cli.rs`'s own `Server` harness, but kept local to this
/// test file (the plan forbids editing `tests/cli.rs` or
/// `tests/common/mod.rs`).
struct RsgServer {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl RsgServer {
    fn spawn(backend_addr: &str, detok_addr: &str) -> RsgServer {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rsg-server"))
            .args([
                "--backend-addr",
                backend_addr,
                "--backend-role",
                "connect",
                "--detok-addr",
                detok_addr,
                "--detok-role",
                "bind",
                "--model",
                "Qwen/Qwen3-0.6B",
                "--run-id",
                ".rsg=mock",
                "--port",
                "0",
            ])
            .env("RUST_LOG", "info")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rsg-server");
        let stderr = child.stderr.take().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&lines);
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });
        let stdin = child.stdin.take();
        RsgServer {
            child,
            stdin,
            lines,
        }
    }

    fn write(&mut self, s: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        stdin.write_all(s.as_bytes()).expect("write stdin");
        stdin.flush().expect("flush stdin");
    }

    fn stderr(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    fn wait_for_log(&mut self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(l) = self
                .lines
                .lock()
                .unwrap()
                .iter()
                .find(|l| l.contains(needle))
            {
                return l.clone();
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "timed out waiting for {needle:?}; stderr:\n{}",
                    self.stderr()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn signal(&self, sig: &str) {
        let status = Command::new("kill")
            .args([sig, &self.child.id().to_string()])
            .status()
            .expect("run kill");
        assert!(status.success(), "kill {sig} failed");
    }

    fn wait_exit(&mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("rsg-server did not exit; stderr:\n{}", self.stderr());
            }
            thread::sleep(Duration::from_millis(20));
        };
        thread::sleep(Duration::from_millis(100));
        status
            .code()
            .unwrap_or_else(|| panic!("killed by signal: {status:?}; stderr:\n{}", self.stderr()))
    }
}

impl Drop for RsgServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The real `rsg-server` binary accepts mock-scheduler's own handshake
/// line, relayed verbatim to its stdin as the launcher would.
#[test]
fn rsg_server_binary_accepts_mock_handshake() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let handshake_line = mock
        .handshake_line()
        .expect("mock's handshake line available after wait_ready");

    // rsg-server takes the opposite role from the mock on each endpoint
    // (backend: mock binds, rsg-server connects; detok: mock connects,
    // rsg-server binds) — exactly mirroring the real launcher's topology.
    // This test only exercises the handshake/signal contract, not actual
    // message exchange over these sockets.
    let mut server = RsgServer::spawn(&mock.backend_addr, &mock.detok_addr);
    server.write(&format!("{handshake_line}\n"));

    let line = server.wait_for_log("handshake received");
    for field in [
        "max_seq_len=4096".to_string(),
        "eos_token_id=151645".to_string(),
        format!("upstream_sha={EXPECTED_UPSTREAM_SHA}"),
    ] {
        assert!(line.contains(&field), "missing {field} in {line:?}");
    }

    server.signal("-TERM");
    assert_eq!(server.wait_exit(), 0, "stderr:\n{}", server.stderr());
}
