//! Process-level tests of `mock-scheduler` (D-08, D-10): the tracer proves
//! one submitted `UserMsg` comes back as its echo tokens over a real
//! `ipc://` round trip through a spawned subprocess.

#[allow(dead_code)] // shared test helpers; this file uses only some of them
mod common;

use common::{MockScheduler, echo_tokens, recv_frame, user_msg};
use rsg_server::handshake::EXPECTED_UPSTREAM_SHA;
use rsg_server::transport::BackendSink;
use rsg_wire::{TokenizerMsg, encode_backend};

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
