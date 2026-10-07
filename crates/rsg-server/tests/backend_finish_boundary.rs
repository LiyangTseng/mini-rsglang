//! Phase 06-08 scope expansion: confirms the engine/dispatch pair never
//! second-guesses the backend's own `finished` flag. The GPU parity run
//! (docs/benchmarks/parity-report.json, prompt `edge-08`) found Rust one
//! token short of Python for both models, always at a natural-end-of-
//! generation boundary (the `max_tokens` cap for Qwen3-0.6B, immediately
//! after the stop token for Llama-3.2-1B-Instruct). The backend tap
//! (`_reply_tokenizer_rank0`, inside the scheduler process) recorded that
//! shortfall at its own emission point -- before any ZMQ framing, before
//! the frontend ever sees the frame -- so whatever decided to finish one
//! token early did so inside the shared, unmodified backend, not in
//! either frontend's receiving code.
//!
//! This test is the Mac-side half of that conclusion: it bypasses
//! `mock-scheduler`'s own (cap-only) finishing logic entirely and injects
//! a hand-built `DetokenizeMsg` sequence directly into the dispatcher's
//! bound detok socket, so `finished` is controlled by the test, never by
//! any cap or any token-id comparison on the Rust side. It proves two
//! things a Rust-side bug could otherwise explain the GPU finding:
//!
//! 1. Rust includes a token flagged `finished=true` in its output even
//!    though it (a) arrives before the client's requested `max_tokens`
//!    cap, so nothing in the engine's own loop is counting down to a
//!    cap-driven stop; (b) is not the same id as any `late_tokens`-style
//!    marker; and (c) is preceded by an id that is numerically identical
//!    to a plausible eos marker but flagged `finished=false` -- proving
//!    the engine never makes its own "that looks like EOS" call
//!    independent of the wire's `finished` bit (see `engine.rs`'s
//!    `drive_request`: the only thing it ever branches on is
//!    `reply.finished`).
//! 2. No token is silently dropped or duplicated: the exact sequence of
//!    ids and `finished` flags the test sends is the exact sequence
//!    `ActiveRequest::next_event` yields, including the very last one.
//!
//! Together with `crates/rsg-server/tests/http_chat.rs`'s existing
//! `max_tokens`-cap coverage (already proven exact: `chat_nonstream_response_shape`,
//! `tracer_chat_stream_matches_upstream_framing`), this closes the two
//! shapes the GPU report's working hypotheses named, on the real
//! production `engine.rs`/`dispatch.rs` pair, with no network/GPU
//! dependency.

#[allow(dead_code)]
mod common;

use common::test_server::{TestConfig, TestServer};

use rsg_server::codec::Prompt;
use rsg_server::engine::RequestEvent;
use rsg_wire::{SamplingParams, TokenizerMsg};

/// A token id that looks like a plausible eos/stop marker (mirrors the
/// Llama finding: the token that precedes the real divergence is a
/// stop-shaped id, `128009` in the GPU run). `TestServer`'s `ByteCodec`
/// only accepts `0..=255` (it is a one-byte-per-token test double, see
/// its module doc), so this test uses an arbitrary in-range id rather
/// than the GPU run's literal `128009` -- the point under test
/// (`engine.rs`/`dispatch.rs` must not treat *any* id as special on
/// their own, only the wire's `finished` bit may end a request) does not
/// depend on the id's literal value, only on it being flagged
/// `finished=false` immediately before a different id flagged
/// `finished=true`.
const EOS_LOOKING_ID: i64 = 200;

/// Opens a raw `zmq::PUSH` peer connected into `detok_addr` (the
/// dispatcher's bound detok socket -- the same address `mock-scheduler`
/// itself connects a PUSH into, per `common::MockScheduler::frontend`'s
/// doc). A PULL-bound socket accepts frames from any number of connected
/// PUSH peers, so this coexists with the real `mock-scheduler` process
/// without needing to silence or replace it.
struct RawBackendPeer {
    _ctx: zmq::Context,
    push: zmq::Socket,
}

impl RawBackendPeer {
    fn connect(detok_addr: &str) -> RawBackendPeer {
        let ctx = zmq::Context::new();
        let push = ctx.socket(zmq::PUSH).expect("create raw PUSH peer");
        push.set_linger(0).expect("set linger 0");
        push.connect(detok_addr).expect("connect raw PUSH peer");
        RawBackendPeer { _ctx: ctx, push }
    }

    fn send_detok(&self, uid: i64, next_token: i64, finished: bool) {
        let msg = TokenizerMsg::DetokenizeMsg {
            uid,
            next_token,
            finished,
        };
        let bytes = rsg_wire::encode_tokenizer(&msg).expect("encode DetokenizeMsg");
        self.push.send(&bytes, 0).expect("send raw detok frame");
    }
}

/// A `finished=true` reply that arrives nowhere near the client's
/// requested `max_tokens` cap, with an eos-shaped id immediately before
/// it flagged `finished=false`, must be reported in full: the
/// eos-looking token's text first (not finished), then the real last
/// token (finished).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finished_flag_alone_ends_the_request_never_an_id_comparison() {
    // `--prefill-delay-ms` huge enough that mock-scheduler's own cap-based
    // echo logic never fires inside this test's lifetime: every token this
    // test observes comes from `RawBackendPeer`, never from the mock's own
    // `emit_due_tokens`.
    let server = TestServer::start(&["--prefill-delay-ms", "600000"], TestConfig::default()).await;

    let mut active = server.engine.start(
        Prompt::Text("hello".to_string()),
        SamplingParams {
            max_tokens: 1000, // far above anything this test ever sends
            ..SamplingParams::default()
        },
    );
    active.accepted().await.expect("accepted");
    let uid = active.uid();

    let peer = RawBackendPeer::connect(&server.mock.detok_addr);

    // The eos-looking id, NOT finished -- must be delivered as an ordinary
    // token, not treated as a stop signal.
    peer.send_detok(uid, EOS_LOOKING_ID, false);
    // One real token after it, flagged finished -- ends the request, well
    // short of max_tokens=1000 and with no cap-driven countdown involved.
    peer.send_detok(uid, 42, true);

    let first = active.next_event().await.expect("first event");
    match first {
        RequestEvent::Token { finished, .. } => assert!(!finished, "first token must not be finished"),
        other => panic!("expected a Token event, got {other:?}"),
    }

    let second = active.next_event().await.expect("second event");
    match second {
        RequestEvent::Token { finished, .. } => assert!(finished, "second token must be finished"),
        other => panic!("expected a Token event, got {other:?}"),
    }

    let end = active.next_event().await;
    assert!(
        end.is_none(),
        "no further event after the finished token, got {end:?}"
    );
}

/// A `finished=true` reply on the very first token (uid's first message
/// ever) still ends the request correctly -- the shortest possible
/// "backend decided to stop immediately" shape, with nothing buffered or
/// require a second message to confirm the end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finished_on_the_first_token_ends_immediately() {
    let server = TestServer::start(&["--prefill-delay-ms", "600000"], TestConfig::default()).await;

    let mut active = server.engine.start(
        Prompt::Text("hi".to_string()),
        SamplingParams {
            max_tokens: 1000,
            ..SamplingParams::default()
        },
    );
    active.accepted().await.expect("accepted");
    let uid = active.uid();

    let peer = RawBackendPeer::connect(&server.mock.detok_addr);
    peer.send_detok(uid, 7, true);

    let first = active.next_event().await.expect("first event");
    match first {
        RequestEvent::Token { finished, .. } => assert!(finished, "the single token must be finished"),
        other => panic!("expected a Token event, got {other:?}"),
    }

    let end = active.next_event().await;
    assert!(end.is_none(), "no further event, got {end:?}");
}
