//! Cross-plan end-to-end proof (03-06): the misbehaving mock (03-03) driving
//! the full transport (03-04's ticket-gated writer, 03-05's counting
//! dispatcher), plus the real `rsg-server` binary accepting the mock's
//! readiness handshake. This file composes the 03-01 through 03-05
//! contracts; it adds no new production code.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use rsg_server::dispatch::{UidEvent, spawn_dispatcher};
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
