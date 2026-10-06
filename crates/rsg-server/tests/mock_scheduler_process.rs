//! Process-level tests of `mock-scheduler` (D-08, D-10): the tracer proves
//! one submitted `UserMsg` comes back as its echo tokens over a real
//! `ipc://` round trip through a spawned subprocess.

#[allow(dead_code)] // shared test helpers; this file uses only some of them
mod common;

use common::{MockScheduler, Observed, echo_tokens, recv_frame, user_msg};
use rsg_server::handshake::EXPECTED_UPSTREAM_SHA;
use rsg_server::transport::BackendSink;
use rsg_wire::{BackendMsg, TokenizerMsg, encode_backend};
use std::time::{Duration, Instant};

#[test]
fn tracer_one_request_echo_round_trip() {
    let mut mock = MockScheduler::spawn(&["--prefill-delay-ms", "5", "--decode-delay-ms", "1"]);
    let handshake = mock.wait_ready();
    assert_eq!(handshake.handshake_version, 1);
    assert_eq!(handshake.max_seq_len, 4096);
    assert_eq!(handshake.eos_token_id, Some(151645));
    assert_eq!(handshake.upstream_sha, EXPECTED_UPSTREAM_SHA);

    let frontend = mock.frontend();
    let msg = user_msg(7, &[11, 22, 33], 5);
    frontend
        .send_backend(&encode_backend(&msg).expect("encode UserMsg"))
        .expect("send UserMsg");

    let mut frames = Vec::new();
    loop {
        let frame = recv_frame(&frontend, 2000).unwrap_or_else(|| {
            panic!("timed out waiting for a reply frame; got {frames:?} so far")
        });
        let finished = match &frame {
            TokenizerMsg::DetokenizeMsg { finished, .. } => *finished,
            TokenizerMsg::BatchTokenizerMsg { .. } => {
                panic!("expected bare DetokenizeMsg frames, got a batch: {frame:?}")
            }
        };
        frames.push(frame);
        if finished {
            break;
        }
    }

    assert_eq!(frames.len(), 5, "{frames:?}");
    let expected_tokens = echo_tokens(&[11, 22, 33], 5);
    for (i, frame) in frames.iter().enumerate() {
        match frame {
            TokenizerMsg::DetokenizeMsg {
                uid,
                next_token,
                finished,
            } => {
                assert_eq!(*uid, 7, "frame {i}");
                assert_eq!(*next_token, expected_tokens[i], "frame {i}");
                assert_eq!(*finished, i == 4, "frame {i}");
            }
            TokenizerMsg::BatchTokenizerMsg { .. } => unreachable!(),
        }
    }
}

#[test]
fn exit_msg_exits_0_and_records_order() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(1, &[1, 2, 3], 2),
            BackendMsg::AbortBackendMsg { uid: 1 },
            user_msg(2, &[1, 2, 3, 4], 1),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");
    frontend
        .send_backend(&encode_backend(&BackendMsg::ExitMsg {}).expect("encode ExitMsg"))
        .expect("send ExitMsg");

    assert_eq!(mock.wait_exit(), 0, "stderr:\n{}", mock.stderr());
    assert_eq!(
        mock.observed(),
        vec![
            Observed::Submit {
                uid: 1,
                input_len: 3
            },
            Observed::Abort { uid: 1 },
            Observed::Submit {
                uid: 2,
                input_len: 4
            },
            Observed::Exit,
        ]
    );
}

#[test]
fn items_after_exit_in_a_batch_are_not_processed() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(1, &[1, 2, 3], 2),
            BackendMsg::ExitMsg {},
            user_msg(2, &[1, 2, 3, 4], 1),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    assert_eq!(mock.wait_exit(), 0, "stderr:\n{}", mock.stderr());
    assert_eq!(
        mock.observed(),
        vec![
            Observed::Submit {
                uid: 1,
                input_len: 3
            },
            Observed::Exit,
        ]
    );
}

#[test]
fn abort_stops_in_flight_tokens() {
    let mut mock = MockScheduler::spawn(&["--decode-delay-ms", "20"]);
    mock.wait_ready();
    let frontend = mock.frontend();

    frontend
        .send_backend(&encode_backend(&user_msg(3, &[1, 2, 3], 1000)).expect("encode UserMsg"))
        .expect("send UserMsg");

    match recv_frame(&frontend, 2000).expect("first token") {
        TokenizerMsg::DetokenizeMsg { uid, finished, .. } => {
            assert_eq!(uid, 3);
            assert!(!finished);
        }
        other => panic!("expected DetokenizeMsg, got {other:?}"),
    }

    frontend
        .send_backend(&encode_backend(&BackendMsg::AbortBackendMsg { uid: 3 }).expect("encode"))
        .expect("send abort");

    let deadline = Instant::now() + Duration::from_millis(300);
    let mut extra = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match recv_frame(&frontend, remaining.as_millis() as i64) {
            Some(TokenizerMsg::DetokenizeMsg { uid, finished, .. }) => {
                assert_eq!(uid, 3);
                assert!(!finished, "no finished frame should follow an abort");
                extra += 1;
                assert!(extra <= 1, "at most one token should follow an abort");
            }
            Some(other) => panic!("expected DetokenizeMsg, got {other:?}"),
            None => break,
        }
    }
}

#[test]
fn abort_for_unknown_uid_is_noop() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let frontend = mock.frontend();

    frontend
        .send_backend(&encode_backend(&BackendMsg::AbortBackendMsg { uid: 99 }).expect("encode"))
        .expect("send abort");
    frontend
        .send_backend(&encode_backend(&user_msg(4, &[5, 6, 7], 3)).expect("encode UserMsg"))
        .expect("send UserMsg");

    let mut frames = Vec::new();
    loop {
        let frame = recv_frame(&frontend, 2000).expect("reply frame");
        let finished = match &frame {
            TokenizerMsg::DetokenizeMsg { finished, .. } => *finished,
            other => panic!("expected DetokenizeMsg, got {other:?}"),
        };
        frames.push(frame);
        if finished {
            break;
        }
    }
    assert_eq!(frames.len(), 3, "{frames:?}");
    let expected = echo_tokens(&[5, 6, 7], 3);
    for (i, frame) in frames.iter().enumerate() {
        match frame {
            TokenizerMsg::DetokenizeMsg {
                uid,
                next_token,
                finished,
            } => {
                assert_eq!(*uid, 4, "frame {i}");
                assert_eq!(*next_token, expected[i], "frame {i}");
                assert_eq!(*finished, i == 2, "frame {i}");
            }
            TokenizerMsg::BatchTokenizerMsg { .. } => unreachable!(),
        }
    }
}

#[test]
fn stdin_eof_exits_3() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    mock.close_stdin();
    assert_eq!(mock.wait_exit(), 3, "stderr:\n{}", mock.stderr());
    assert!(mock.stderr().contains("stdin EOF"), "{}", mock.stderr());
}

#[test]
fn sigterm_exits_0() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    mock.signal("-TERM");
    assert_eq!(mock.wait_exit(), 0, "stderr:\n{}", mock.stderr());
}

#[test]
fn sigint_exits_0() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    mock.signal("-INT");
    assert_eq!(mock.wait_exit(), 0, "stderr:\n{}", mock.stderr());
}

#[test]
fn undecodable_frame_exits_4() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let frontend = mock.frontend();
    frontend
        .send_backend(b"not msgpack")
        .expect("send raw bytes");
    assert_eq!(mock.wait_exit(), 4, "stderr:\n{}", mock.stderr());
    assert!(mock.stderr().contains("decode"), "{}", mock.stderr());
}

#[test]
fn prefill_and_decode_delays_are_honored() {
    let mut mock =
        MockScheduler::spawn(&["--prefill-delay-ms", "100", "--decode-delay-ms", "50"]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let start = Instant::now();
    frontend
        .send_backend(&encode_backend(&user_msg(5, &[1, 2, 3], 3)).expect("encode UserMsg"))
        .expect("send UserMsg");

    let mut timestamps = Vec::new();
    loop {
        let frame = recv_frame(&frontend, 2000).expect("reply frame");
        timestamps.push(Instant::now());
        let finished = match frame {
            TokenizerMsg::DetokenizeMsg { finished, .. } => finished,
            other => panic!("expected DetokenizeMsg, got {other:?}"),
        };
        if finished {
            break;
        }
    }

    assert_eq!(timestamps.len(), 3, "{timestamps:?}");
    assert!(
        timestamps[0].duration_since(start) >= Duration::from_millis(100),
        "first token arrived too early: {:?}",
        timestamps[0].duration_since(start)
    );
    for i in 1..timestamps.len() {
        let gap = timestamps[i].duration_since(timestamps[i - 1]);
        assert!(
            gap >= Duration::from_millis(45),
            "gap {i} too short: {gap:?}; stderr:\n{}",
            mock.stderr()
        );
    }
}
