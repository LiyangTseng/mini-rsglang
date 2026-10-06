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

#[test]
fn many_concurrent_requests_route_by_uid() {
    let mut mock = MockScheduler::spawn(&["--decode-delay-ms", "1"]);
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
        let mut set = tokio::task::JoinSet::new();
        for u in 1..=32i64 {
            let writer = writer.clone();
            let dispatcher = dispatcher.clone();
            set.spawn(async move {
                let ids: [i32; 3] = [(u * 10 + 1) as i32, (u * 10 + 2) as i32, (u * 10 + 3) as i32];
                let mut stream = dispatcher.register(u);
                let submitted = writer
                    .submit(
                        u,
                        Tensor::from_i32_slice(&ids),
                        SamplingParams {
                            max_tokens: 8,
                            ..SamplingParams::default()
                        },
                    )
                    .await
                    .expect("submit");
                assert_eq!(submitted.uid(), u);

                let mut tokens = Vec::new();
                loop {
                    let event = tokio::time::timeout(Duration::from_secs(10), stream.recv())
                        .await
                        .unwrap_or_else(|_| panic!("uid {u} timed out waiting for tokens"))
                        .unwrap_or_else(|| panic!("uid {u} stream ended before finished"));
                    match event {
                        UidEvent::Token(reply) => {
                            assert_eq!(reply.uid, u);
                            tokens.push(reply.next_token);
                            if reply.finished {
                                break;
                            }
                        }
                        UidEvent::Dropped(n) => {
                            panic!("uid {u} unexpectedly dropped {n} tokens")
                        }
                    }
                }
                let expected = echo_tokens(&ids, 8);
                assert_eq!(tokens, expected, "uid {u} token mismatch");
                u
            });
        }

        let mut seen = Vec::new();
        while let Some(res) = set.join_next().await {
            seen.push(res.expect("task panicked"));
        }
        seen.sort_unstable();
        assert_eq!(seen, (1..=32i64).collect::<Vec<_>>());

        writer.exit().await.expect("exit");
    });

    assert_eq!(mock.wait_exit(), 0);

    let observed = mock.observed();
    assert_eq!(observed.len(), 33, "32 submits plus exit");
    let mut submit_uids: Vec<i64> = observed
        .iter()
        .filter_map(|o| match o {
            Observed::Submit { uid, input_len } => {
                assert_eq!(*input_len, 3);
                Some(*uid)
            }
            _ => None,
        })
        .collect();
    submit_uids.sort_unstable();
    assert_eq!(submit_uids, (1..=32i64).collect::<Vec<_>>());
    assert_eq!(observed.last(), Some(&Observed::Exit), "Exit is last");
}
