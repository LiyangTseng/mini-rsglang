//! D-02/D-03: proof that an abort can never overtake its own submit.
//!
//! `tracer_abort_from_another_task_follows_submit` is the deterministic
//! cross-task tracer: a task other than the submitter holds the
//! [`rsg_server::writer::Submitted`] ticket (received over a `oneshot`,
//! after the submit has already produced its first token) and calls
//! `abort`. `abort_never_precedes_its_own_submit` (03-04 Task 3) is the
//! property test proving the same guarantee across random concurrent
//! interleavings of up to 48 uids, run end to end through the real
//! transport and a real `mock-scheduler` subprocess (D-03).

#[allow(dead_code)]
mod common;

use std::time::Duration;

use rsg_server::dispatch::{UidEvent, spawn_dispatcher};
use rsg_server::writer::spawn_writer;
use rsg_wire::{SamplingParams, Tensor};

use common::{MockScheduler, Observed};

#[test]
fn tracer_abort_from_another_task_follows_submit() {
    let mut mock = MockScheduler::spawn(&["--decode-delay-ms", "10"]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatcher = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build runtime");

    rt.block_on(async {
        let mut stream = dispatcher.register(1);
        let (ticket_tx, ticket_rx) = tokio::sync::oneshot::channel();

        let abort_writer = writer.clone();
        let abort_task = tokio::spawn(async move {
            let ticket = ticket_rx.await.expect("receive ticket over oneshot");
            abort_writer.abort(&ticket).await.expect("abort");
        });

        let submitted = writer
            .submit(
                1,
                Tensor::from_i32_slice(&[11, 22, 33]),
                SamplingParams {
                    max_tokens: 1000,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit");
        assert_eq!(submitted.uid(), 1);

        // Wait for the first token before aborting, so this genuinely
        // cancels an in-flight request rather than racing an empty queue.
        let event = tokio::time::timeout(Duration::from_secs(5), stream.recv())
            .await
            .expect("timed out waiting for first token")
            .expect("stream ended before first token");
        match event {
            UidEvent::Token(reply) => assert_eq!(reply.uid, 1),
            UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens"),
        }

        ticket_tx
            .send(submitted)
            .expect("send ticket to abort task");
        abort_task.await.expect("abort task panicked");

        writer.exit().await.expect("exit");
    });

    assert_eq!(mock.wait_exit(), 0);
    assert_eq!(
        mock.observed(),
        vec![
            Observed::Submit {
                uid: 1,
                input_len: 3
            },
            Observed::Abort { uid: 1 },
            Observed::Exit,
        ]
    );
}
