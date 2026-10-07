//! End-to-end tracer for the per-uid dispatcher's backpressure contract
//! (D-05/D-06): a slow consumer on one uid never stalls another uid's
//! replies, and every token the dispatcher overwrites for the slow
//! consumer comes back counted and at the exact position of the gap.

#[allow(dead_code)]
mod common;

use std::time::Duration;

use rsg_server::dispatch::{UID_CHANNEL_CAPACITY, UidEvent, spawn_dispatcher};
use rsg_server::writer::spawn_writer;
use rsg_wire::{SamplingParams, Tensor};

use common::{MockScheduler, echo_tokens};

#[test]
fn tracer_slow_consumer_does_not_stall_other_uids() {
    let mut mock = MockScheduler::spawn(&["--decode-delay-ms", "1"]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatcher = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build runtime");

    // Sent well past the channel's fixed capacity so the backlog overflows
    // by a fixed, meaningful margin (200 tokens dropped) regardless of the
    // exact capacity value -- this test asserts the overflow CONTRACT
    // (drop-oldest, exact count, exact tail), not the capacity constant
    // itself.
    const OVERFLOW_MARGIN: u64 = 200;
    let total_sent = UID_CHANNEL_CAPACITY as i64 + OVERFLOW_MARGIN as i64;

    rt.block_on(async {
        let mut stream1 = dispatcher.register(1);
        let mut stream2 = dispatcher.register(2);

        // Submit uid 1 before uid 2 from the same task: the FIFO writer
        // delivers uid 1's submit no later than uid 2's, so uid 1 is never
        // behind uid 2 once they share a step.
        writer
            .submit(
                1,
                Tensor::from_i32_slice(&[101, 102, 103]),
                SamplingParams {
                    max_tokens: total_sent,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit uid 1");
        writer
            .submit(
                2,
                Tensor::from_i32_slice(&[201, 202, 203]),
                SamplingParams {
                    max_tokens: total_sent,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit uid 2");

        // uid 1's stream is left unread while uid 2 drains to completion.
        let mut tokens2 = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(15), stream2.recv())
                .await
                .expect("uid 2 timed out waiting for tokens")
                .expect("uid 2 stream ended before finished");
            match event {
                UidEvent::Token(reply) => {
                    assert_eq!(reply.uid, 2);
                    tokens2.push(reply.next_token);
                    if reply.finished {
                        break;
                    }
                }
                UidEvent::Dropped(n) => panic!("uid 2 unexpectedly dropped {n} tokens"),
            }
        }
        assert_eq!(tokens2, echo_tokens(&[201, 202, 203], total_sent as usize));

        // Now read uid 1's backlog: exactly one Dropped(OVERFLOW_MARGIN),
        // then the last UID_CHANNEL_CAPACITY echo tokens, the last one
        // finished.
        let mut events1 = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), stream1.recv())
                .await
                .expect("uid 1 timed out waiting for backlog");
            match event {
                Some(e) => events1.push(e),
                None => break,
            }
        }

        assert_eq!(
            events1.first(),
            Some(&UidEvent::Dropped(OVERFLOW_MARGIN)),
            "uid 1's first event must be exactly Dropped({OVERFLOW_MARGIN}): {events1:?}"
        );
        let expected_tail = echo_tokens(&[101, 102, 103], total_sent as usize);
        let expected_tail = &expected_tail[(OVERFLOW_MARGIN as usize)..(total_sent as usize)];
        let got_tokens: Vec<i64> = events1[1..]
            .iter()
            .map(|e| match e {
                UidEvent::Token(reply) => {
                    assert_eq!(reply.uid, 1);
                    reply.next_token
                }
                UidEvent::Dropped(n) => panic!("unexpected extra drop of {n} for uid 1"),
            })
            .collect();
        assert_eq!(
            got_tokens, expected_tail,
            "uid 1's last {UID_CHANNEL_CAPACITY} echo tokens"
        );
        assert_eq!(
            events1.len(),
            UID_CHANNEL_CAPACITY + 1,
            "Dropped({OVERFLOW_MARGIN}) + {UID_CHANNEL_CAPACITY} tokens, then stream end"
        );
        match events1.last() {
            Some(UidEvent::Token(reply)) => assert!(reply.finished, "last token must be finished"),
            other => panic!("expected the last event to be a finished Token, got {other:?}"),
        }
        assert_eq!(stream1.dropped(), OVERFLOW_MARGIN);

        writer.exit().await.expect("exit");
    });

    assert_eq!(mock.wait_exit(), 0);
}
