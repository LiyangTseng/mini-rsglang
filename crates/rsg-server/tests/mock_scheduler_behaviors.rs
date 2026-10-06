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
