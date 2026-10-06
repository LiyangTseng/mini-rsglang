//! Process-level tests of `mock-scheduler`'s MOCK-01 backend misbehaviors
//! (D-09): late tokens after an abort, silently dropped overlong prompts
//! (plus the upstream overlong drop/clamp rule), reply batching, several
//! behaviors in one process, and CLI validation.

#[allow(dead_code)] // shared test helpers; this file uses only some of them
mod common;

use common::{MockScheduler, Observed, echo_tokens, recv_frame, user_msg};
use rsg_server::transport::BackendSink;
use rsg_wire::{BackendMsg, TokenizerMsg, encode_backend};
use std::time::{Duration, Instant};

#[test]
fn late_abort_token_sends_three_tokens_after_abort() {
    let mut mock = MockScheduler::spawn(&[
        "--decode-delay-ms",
        "20",
        "--misbehave-uids",
        "5",
        "--behavior",
        "late-abort-token",
    ]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(5, &[51, 52, 53], 1000),
            user_msg(6, &[61, 62, 63], 1000),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    // Read frames until both uids have produced at least one token.
    let mut uid5_tokens: Vec<i64> = Vec::new();
    let mut uid6_seen = false;
    loop {
        match recv_frame(&frontend, 2000).expect("reply frame before abort") {
            TokenizerMsg::DetokenizeMsg {
                uid, next_token, ..
            } => {
                if uid == 5 {
                    uid5_tokens.push(next_token);
                } else if uid == 6 {
                    uid6_seen = true;
                }
            }
            other => panic!("expected bare DetokenizeMsg, got {other:?}"),
        }
        if !uid5_tokens.is_empty() && uid6_seen {
            break;
        }
    }

    let abort_batch = BackendMsg::BatchBackendMsg {
        data: vec![
            BackendMsg::AbortBackendMsg { uid: 5 },
            BackendMsg::AbortBackendMsg { uid: 6 },
        ],
    };
    frontend
        .send_backend(&encode_backend(&abort_batch).expect("encode abort batch"))
        .expect("send abort batch");

    let deadline = Instant::now() + Duration::from_millis(400);
    let mut uid5_after_abort = 0u32;
    let mut uid6_after_abort = 0u32;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match recv_frame(&frontend, remaining.as_millis() as i64) {
            Some(TokenizerMsg::DetokenizeMsg {
                uid,
                next_token,
                finished,
            }) => match uid {
                5 => {
                    assert!(!finished, "uid 5's late tokens must never be finished");
                    uid5_tokens.push(next_token);
                    uid5_after_abort += 1;
                }
                6 => {
                    assert!(!finished, "uid 6 should not finish after its own abort");
                    uid6_after_abort += 1;
                    assert!(
                        uid6_after_abort <= 1,
                        "at most one token should follow uid 6's (unflagged) abort"
                    );
                }
                other => panic!("unexpected uid {other} after abort"),
            },
            Some(other) => panic!("expected bare DetokenizeMsg, got {other:?}"),
            None => break,
        }
    }

    assert!(
        (3..=4).contains(&uid5_after_abort),
        "uid 5 should receive 3 or 4 tokens after its abort (3 late plus at most 1 \
         already in flight), got {uid5_after_abort}"
    );

    // The full uid-5 token sequence (before + after abort) must be a
    // gap-free, repeat-free prefix of the echo sequence — no dropped or
    // duplicated token across the abort boundary.
    let expected = echo_tokens(&[51, 52, 53], uid5_tokens.len());
    assert_eq!(uid5_tokens, expected, "uid 5 token sequence");

    assert!(
        mock.stderr().contains("late-abort-token"),
        "{}",
        mock.stderr()
    );

    frontend
        .send_backend(&encode_backend(&BackendMsg::ExitMsg {}).expect("encode ExitMsg"))
        .expect("send ExitMsg");
    assert_eq!(mock.wait_exit(), 0, "stderr:\n{}", mock.stderr());

    let observed = mock.observed();
    let submit5_idx = observed
        .iter()
        .position(|o| {
            *o == Observed::Submit {
                uid: 5,
                input_len: 3,
            }
        })
        .expect("Submit{uid:5} present in observe file");
    let submit6_idx = observed
        .iter()
        .position(|o| {
            *o == Observed::Submit {
                uid: 6,
                input_len: 3,
            }
        })
        .expect("Submit{uid:6} present in observe file");
    let abort5_idx = observed
        .iter()
        .position(|o| *o == Observed::Abort { uid: 5 })
        .expect("Abort{uid:5} present in observe file");
    let abort6_idx = observed
        .iter()
        .position(|o| *o == Observed::Abort { uid: 6 })
        .expect("Abort{uid:6} present in observe file");
    assert!(
        abort5_idx > submit5_idx && abort5_idx > submit6_idx,
        "Abort{{5}} must come after both Submits: {observed:?}"
    );
    assert!(
        abort6_idx > submit5_idx && abort6_idx > submit6_idx,
        "Abort{{6}} must come after both Submits: {observed:?}"
    );
}

#[test]
fn drop_overlong_flagged_uid_never_replies() {
    let mut mock = MockScheduler::spawn(&["--misbehave-uids", "9", "--behavior", "drop-overlong"]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![user_msg(9, &[1, 2], 3), user_msg(10, &[4, 5], 3)],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    let mut frames = 0;
    loop {
        match recv_frame(&frontend, 2000).expect("reply frame") {
            TokenizerMsg::DetokenizeMsg { uid, finished, .. } => {
                assert_eq!(uid, 10, "only uid 10 should ever reply");
                frames += 1;
                if finished {
                    break;
                }
            }
            other => panic!("expected bare DetokenizeMsg, got {other:?}"),
        }
    }
    assert_eq!(frames, 3);

    assert!(
        recv_frame(&frontend, 500).is_none(),
        "uid 9 must never reply"
    );

    assert!(
        mock.stderr().contains("dropped overlong prompt"),
        "{}",
        mock.stderr()
    );

    let observed = mock.observed();
    assert!(
        observed.contains(&Observed::Submit {
            uid: 9,
            input_len: 2
        }),
        "{observed:?}"
    );
    assert!(
        observed.contains(&Observed::Submit {
            uid: 10,
            input_len: 2
        }),
        "{observed:?}"
    );
}

#[test]
fn prompt_at_max_seq_len_is_dropped_and_shorter_prompt_is_clamped() {
    let mut mock = MockScheduler::spawn(&["--max-seq-len", "8"]);
    let handshake = mock.wait_ready();
    assert_eq!(handshake.max_seq_len, 8);
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(1, &[1, 2, 3, 4, 5, 6, 7, 8], 5),
            user_msg(2, &[70, 71, 72, 73, 74, 75, 76], 5),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    match recv_frame(&frontend, 2000).expect("reply frame") {
        TokenizerMsg::DetokenizeMsg {
            uid,
            next_token,
            finished,
        } => {
            assert_eq!(uid, 2);
            assert_eq!(next_token, 70);
            assert!(finished);
        }
        other => panic!("expected bare DetokenizeMsg, got {other:?}"),
    }

    assert!(
        recv_frame(&frontend, 300).is_none(),
        "nothing more should arrive (uid 1 is dropped, uid 2 already finished)"
    );
}

#[test]
fn batch_size_four_sends_one_batch_tokenizer_msg() {
    let mut mock = MockScheduler::spawn(&["--batch-size", "4", "--prefill-delay-ms", "50"]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let data: Vec<BackendMsg> = (1..=4i64)
        .map(|u| user_msg(u, &[(u * 10) as i32, (u * 10 + 1) as i32], 1))
        .collect();
    frontend
        .send_backend(&encode_backend(&BackendMsg::BatchBackendMsg { data }).expect("encode"))
        .expect("send batch");

    match recv_frame(&frontend, 2000).expect("reply frame") {
        TokenizerMsg::BatchTokenizerMsg { data } => {
            assert_eq!(data.len(), 4, "{data:?}");
            for (i, entry) in data.iter().enumerate() {
                let uid = (i + 1) as i64;
                match entry {
                    TokenizerMsg::DetokenizeMsg {
                        uid: got_uid,
                        next_token,
                        finished,
                    } => {
                        assert_eq!(*got_uid, uid, "entry {i}");
                        assert_eq!(*next_token, uid * 10, "entry {i}");
                        assert!(*finished, "entry {i}");
                    }
                    other => panic!("expected DetokenizeMsg entries, got {other:?}"),
                }
            }
        }
        other => panic!("expected a BatchTokenizerMsg, got {other:?}"),
    }

    assert!(
        recv_frame(&frontend, 300).is_none(),
        "no further frame should arrive"
    );
}

#[test]
fn partial_batch_flushes_on_timer() {
    let mut mock = MockScheduler::spawn(&["--batch-size", "4"]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![user_msg(1, &[11], 1), user_msg(2, &[22], 1)],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    match recv_frame(&frontend, 1000).expect("reply frame") {
        TokenizerMsg::BatchTokenizerMsg { data } => assert_eq!(data.len(), 2, "{data:?}"),
        other => panic!("expected a BatchTokenizerMsg, got {other:?}"),
    }

    frontend
        .send_backend(&encode_backend(&user_msg(3, &[33], 1)).expect("encode"))
        .expect("send UserMsg");
    match recv_frame(&frontend, 1000).expect("reply frame") {
        TokenizerMsg::DetokenizeMsg { uid, .. } => assert_eq!(uid, 3),
        other => panic!("expected a bare DetokenizeMsg, got {other:?}"),
    }
}

#[test]
fn default_batch_size_sends_every_reply_bare() {
    let mut mock = MockScheduler::spawn(&[]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![user_msg(1, &[11, 12], 2), user_msg(2, &[21, 22], 2)],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    for i in 0..4 {
        match recv_frame(&frontend, 2000).expect("reply frame") {
            TokenizerMsg::DetokenizeMsg { .. } => {}
            other => panic!("frame {i}: expected a bare DetokenizeMsg, got {other:?}"),
        }
    }
}

#[test]
fn uid_range_applies_behavior_to_each_uid() {
    let mut mock = MockScheduler::spawn(&[
        "--misbehave-uids",
        "3-5",
        "--behavior",
        "drop-overlong",
    ]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(3, &[1], 1),
            user_msg(4, &[1], 1),
            user_msg(5, &[1], 1),
            user_msg(6, &[1], 1),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    match recv_frame(&frontend, 500).expect("reply frame") {
        TokenizerMsg::DetokenizeMsg { uid, finished, .. } => {
            assert_eq!(uid, 6);
            assert!(finished);
        }
        other => panic!("expected bare DetokenizeMsg, got {other:?}"),
    }
    assert!(
        recv_frame(&frontend, 300).is_none(),
        "uids 3-5 must never reply"
    );
}

#[test]
fn several_behaviors_in_one_process() {
    let mut mock = MockScheduler::spawn(&[
        "--decode-delay-ms",
        "20",
        "--misbehave-uids",
        "1",
        "--behavior",
        "late-abort-token",
        "--misbehave-uids",
        "2",
        "--behavior",
        "drop-overlong",
    ]);
    mock.wait_ready();
    let frontend = mock.frontend();

    let batch = BackendMsg::BatchBackendMsg {
        data: vec![
            user_msg(1, &[11, 12, 13], 1000),
            user_msg(2, &[21], 5),
            user_msg(3, &[31, 32, 33], 3),
        ],
    };
    frontend
        .send_backend(&encode_backend(&batch).expect("encode batch"))
        .expect("send batch");

    let mut uid3_count = 0u32;
    let mut uid1_seen = false;
    loop {
        match recv_frame(&frontend, 2000).expect("reply frame") {
            TokenizerMsg::DetokenizeMsg { uid, finished, .. } => match uid {
                1 => uid1_seen = true,
                3 => {
                    uid3_count += 1;
                    if finished {
                        break;
                    }
                }
                other => panic!("unexpected uid {other} (uid 2 must never reply)"),
            },
            other => panic!("expected bare DetokenizeMsg, got {other:?}"),
        }
    }
    assert_eq!(uid3_count, 3);
    assert!(
        uid1_seen,
        "uid 1 should have emitted at least one token before its abort"
    );

    frontend
        .send_backend(&encode_backend(&BackendMsg::AbortBackendMsg { uid: 1 }).expect("encode"))
        .expect("send abort");

    let deadline = Instant::now() + Duration::from_millis(400);
    let mut uid1_after_abort = 0u32;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match recv_frame(&frontend, remaining.as_millis() as i64) {
            Some(TokenizerMsg::DetokenizeMsg { uid, finished, .. }) => {
                assert_eq!(uid, 1, "uid 2 must never reply");
                assert!(!finished, "late tokens must never be finished");
                uid1_after_abort += 1;
            }
            Some(other) => panic!("expected bare DetokenizeMsg, got {other:?}"),
            None => break,
        }
    }
    assert!(
        (3..=4).contains(&uid1_after_abort),
        "got {uid1_after_abort}"
    );
}

#[test]
fn unpaired_flags_exit_2() {
    let mut mock = MockScheduler::spawn(&["--misbehave-uids", "3"]);
    assert_eq!(mock.wait_exit(), 2, "stderr:\n{}", mock.stderr());
    assert!(mock.handshake_line().is_none());
}

#[test]
fn overlapping_uid_lists_exit_2() {
    let mut mock = MockScheduler::spawn(&[
        "--misbehave-uids",
        "3-5",
        "--behavior",
        "late-abort-token",
        "--misbehave-uids",
        "5",
        "--behavior",
        "drop-overlong",
    ]);
    assert_eq!(mock.wait_exit(), 2, "stderr:\n{}", mock.stderr());
    assert!(mock.stderr().contains('5'), "{}", mock.stderr());
    assert!(mock.handshake_line().is_none());
}

#[test]
fn malformed_uid_list_exits_2() {
    for bad in ["5-3", "abc", "1,,2", "-4"] {
        let mut mock =
            MockScheduler::spawn(&["--misbehave-uids", bad, "--behavior", "drop-overlong"]);
        assert_eq!(mock.wait_exit(), 2, "input {bad:?}; stderr:\n{}", mock.stderr());
        assert!(mock.handshake_line().is_none(), "input {bad:?}");
    }
}

#[test]
fn zero_batch_size_exits_2() {
    let mut mock = MockScheduler::spawn(&["--batch-size", "0"]);
    assert_eq!(mock.wait_exit(), 2, "stderr:\n{}", mock.stderr());
    assert!(mock.handshake_line().is_none());
}
