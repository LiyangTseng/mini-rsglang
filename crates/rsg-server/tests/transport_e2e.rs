//! End-to-end tests for the single ordered writer (D-01) and per-uid
//! dispatcher (D-04/D-05/D-07): writer -> real mock-scheduler subprocess ->
//! dispatcher, over real `ipc://` sockets.

#[allow(dead_code)]
mod common;

use std::time::Duration;

use rsg_server::dispatch::{UidEvent, spawn_dispatcher};
use rsg_server::writer::spawn_writer;
use rsg_wire::{SamplingParams, Tensor};

use common::{MockScheduler, Observed, echo_tokens};

#[test]
fn tracer_submit_routes_tokens_back_by_uid() {
    let mut mock = MockScheduler::spawn(&["--prefill-delay-ms", "5", "--decode-delay-ms", "1"]);
    mock.wait_ready();

    let (tx, rx) = mock.frontend().split();
    let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
    let dispatcher = spawn_dispatcher(rx).expect("spawn dispatcher");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build runtime");

    rt.block_on(async {
        let mut stream = dispatcher.register(7);
        let submitted = writer
            .submit(
                7,
                Tensor::from_i32_slice(&[11, 22, 33]),
                SamplingParams {
                    max_tokens: 5,
                    ..SamplingParams::default()
                },
            )
            .await
            .expect("submit");
        assert_eq!(submitted.uid(), 7);

        let mut tokens = Vec::new();
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), stream.recv())
                .await
                .expect("timed out waiting for tokens")
                .expect("stream ended before finished");
            match event {
                UidEvent::Token(reply) => {
                    assert_eq!(reply.uid, 7);
                    tokens.push(reply.next_token);
                    if reply.finished {
                        break;
                    }
                }
                UidEvent::Dropped(n) => panic!("unexpected drop of {n} tokens"),
            }
        }
        assert_eq!(tokens, echo_tokens(&[11, 22, 33], 5));

        let next = tokio::time::timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timed out waiting for stream to end");
        assert_eq!(next, None, "stream must end after the finished token");

        writer.exit().await.expect("exit");
    });

    assert_eq!(mock.wait_exit(), 0);
    assert_eq!(
        mock.observed(),
        vec![
            Observed::Submit {
                uid: 7,
                input_len: 3
            },
            Observed::Exit,
        ]
    );
}
