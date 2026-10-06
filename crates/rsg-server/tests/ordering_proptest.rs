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

use proptest::prelude::*;
use proptest::prop_oneof;

use rsg_server::dispatch::{UidEvent, spawn_dispatcher};
use rsg_server::writer::{Submitted, WriterHandle, spawn_writer};
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

// --- D-02/D-03 property test: random concurrent submit/abort interleavings
// across up to 48 uids, run end to end through the real transport and a
// real mock-scheduler subprocess, never let the scheduler observe an abort
// before its own submit.

/// How (and whether) a uid's own request is aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbortMode {
    None,
    SameTask,
    OtherTask,
}

/// One uid's randomly generated plan.
#[derive(Debug, Clone)]
struct UidPlan {
    yields_before_submit: u8,
    abort_mode: AbortMode,
    yields_before_abort: u8,
    max_tokens: i64,
}

fn uid_plan_strategy() -> impl Strategy<Value = UidPlan> {
    (
        0u8..4,
        prop_oneof![
            Just(AbortMode::None),
            Just(AbortMode::SameTask),
            Just(AbortMode::OtherTask),
        ],
        0u8..4,
        1i64..=4,
    )
        .prop_map(
            |(yields_before_submit, abort_mode, yields_before_abort, max_tokens)| UidPlan {
                yields_before_submit,
                abort_mode,
                yields_before_abort,
                max_tokens,
            },
        )
}

/// A scenario: one plan per uid (uid `i+1` belongs to `plans[i]`), plus the
/// order tasks are spawned in (a permutation of `0..plans.len()`).
#[derive(Debug, Clone)]
struct Scenario {
    plans: Vec<UidPlan>,
    order: Vec<usize>,
}

fn scenario_strategy() -> impl Strategy<Value = Scenario> {
    prop::collection::vec(uid_plan_strategy(), 1..=48).prop_flat_map(|plans| {
        let n = plans.len();
        let order_strategy = Just((0..n).collect::<Vec<usize>>()).prop_shuffle();
        (Just(plans), order_strategy).prop_map(|(plans, order)| Scenario { plans, order })
    })
}

/// The three ids derived from a uid, matching the pattern already proven in
/// `transport_e2e.rs`'s concurrency test.
fn prompt_ids(uid: i64) -> [i32; 3] {
    [(uid * 10 + 1) as i32, (uid * 10 + 2) as i32, (uid * 10 + 3) as i32]
}

/// Runs one generated scenario against `writer`: spawns one task per uid
/// (in `scenario.order`), each yielding, submitting, and (depending on its
/// `AbortMode`) aborting — from the same task or from a second task that
/// receives the ticket over a `oneshot` — then joins every task and sends
/// `ExitMsg`.
async fn run_scenario(writer: &WriterHandle, scenario: &Scenario) {
    let mut set = tokio::task::JoinSet::new();

    for &i in &scenario.order {
        let plan = scenario.plans[i].clone();
        let uid = (i + 1) as i64;

        match plan.abort_mode {
            AbortMode::OtherTask => {
                let (ticket_tx, ticket_rx) = tokio::sync::oneshot::channel::<Submitted>();

                let abort_writer = writer.clone();
                let yields_before_abort = plan.yields_before_abort;
                set.spawn(async move {
                    let ticket = ticket_rx.await.expect("receive ticket over oneshot");
                    for _ in 0..yields_before_abort {
                        tokio::task::yield_now().await;
                    }
                    abort_writer.abort(&ticket).await.expect("abort");
                });

                let submit_writer = writer.clone();
                let yields_before_submit = plan.yields_before_submit;
                let max_tokens = plan.max_tokens;
                set.spawn(async move {
                    for _ in 0..yields_before_submit {
                        tokio::task::yield_now().await;
                    }
                    let ticket = submit_writer
                        .submit(
                            uid,
                            Tensor::from_i32_slice(&prompt_ids(uid)),
                            SamplingParams {
                                max_tokens,
                                ..SamplingParams::default()
                            },
                        )
                        .await
                        .expect("submit");
                    let _ = ticket_tx.send(ticket);
                });
            }
            AbortMode::SameTask => {
                let task_writer = writer.clone();
                let yields_before_submit = plan.yields_before_submit;
                let yields_before_abort = plan.yields_before_abort;
                let max_tokens = plan.max_tokens;
                set.spawn(async move {
                    for _ in 0..yields_before_submit {
                        tokio::task::yield_now().await;
                    }
                    let ticket = task_writer
                        .submit(
                            uid,
                            Tensor::from_i32_slice(&prompt_ids(uid)),
                            SamplingParams {
                                max_tokens,
                                ..SamplingParams::default()
                            },
                        )
                        .await
                        .expect("submit");
                    for _ in 0..yields_before_abort {
                        tokio::task::yield_now().await;
                    }
                    task_writer.abort(&ticket).await.expect("abort");
                });
            }
            AbortMode::None => {
                let task_writer = writer.clone();
                let yields_before_submit = plan.yields_before_submit;
                let max_tokens = plan.max_tokens;
                set.spawn(async move {
                    for _ in 0..yields_before_submit {
                        tokio::task::yield_now().await;
                    }
                    task_writer
                        .submit(
                            uid,
                            Tensor::from_i32_slice(&prompt_ids(uid)),
                            SamplingParams {
                                max_tokens,
                                ..SamplingParams::default()
                            },
                        )
                        .await
                        .expect("submit");
                });
            }
        }
    }

    while let Some(res) = set.join_next().await {
        res.expect("task panicked");
    }

    writer.exit().await.expect("exit");
}

proptest! {
    // Case count spike (RESEARCH A4): cases: 8 measured ~1.08s internal
    // test time (debug build, Mac). Extrapolating and then measuring
    // directly at cases: 64 gave ~7.0s internal test time across 4 runs
    // (6.93s-7.05s), well under the 60s budget, so 64 (the largest of
    // 64/32/16) is the final value. See 03-04-SUMMARY.md for the full
    // measured timings. Shrinking and failure persistence are left at
    // their defaults.
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn abort_never_precedes_its_own_submit(scenario in scenario_strategy()) {
        let mut mock = MockScheduler::spawn(&[]);
        mock.wait_ready();

        let (tx, rx) = mock.frontend().split();
        let (writer, _writer_join) = spawn_writer(tx).expect("spawn writer");
        // Keep the dispatcher alive with no registrations: every reply is
        // dropped, which drains the detok socket so the mock's PUSH never
        // blocks. This test only checks the mock's own observed order.
        let _dispatcher = spawn_dispatcher(rx).expect("spawn dispatcher");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("build runtime");

        rt.block_on(run_scenario(&writer, &scenario));

        prop_assert_eq!(mock.wait_exit(), 0);
        let observed = mock.observed();

        let n = scenario.plans.len();
        let expected_aborts = scenario
            .plans
            .iter()
            .filter(|p| p.abort_mode != AbortMode::None)
            .count();
        prop_assert_eq!(
            observed.len(),
            n + expected_aborts + 1,
            "observed: {:?}, scenario: {:?}",
            observed,
            scenario
        );
        prop_assert_eq!(
            observed.last(),
            Some(&Observed::Exit),
            "last entry must be Exit; observed: {:?}",
            observed
        );

        for (i, plan) in scenario.plans.iter().enumerate() {
            let uid = (i + 1) as i64;

            let submit_positions: Vec<usize> = observed
                .iter()
                .enumerate()
                .filter_map(|(idx, o)| match o {
                    Observed::Submit { uid: u, .. } if *u == uid => Some(idx),
                    _ => None,
                })
                .collect();
            prop_assert_eq!(
                submit_positions.len(),
                1,
                "uid {} submit count != 1; observed: {:?}, scenario: {:?}",
                uid,
                observed,
                scenario
            );
            let submit_idx = submit_positions[0];
            match &observed[submit_idx] {
                Observed::Submit { input_len, .. } => {
                    prop_assert_eq!(
                        *input_len,
                        3,
                        "uid {} input_len != 3; observed: {:?}",
                        uid,
                        observed
                    );
                }
                other => prop_assert!(false, "expected Submit at {}, got {:?}", submit_idx, other),
            }

            let abort_positions: Vec<usize> = observed
                .iter()
                .enumerate()
                .filter_map(|(idx, o)| match o {
                    Observed::Abort { uid: u } if *u == uid => Some(idx),
                    _ => None,
                })
                .collect();

            match plan.abort_mode {
                AbortMode::None => {
                    prop_assert!(
                        abort_positions.is_empty(),
                        "uid {} (mode None) has unexpected Abort; observed: {:?}, scenario: {:?}",
                        uid,
                        observed,
                        scenario
                    );
                }
                AbortMode::SameTask | AbortMode::OtherTask => {
                    prop_assert_eq!(
                        abort_positions.len(),
                        1,
                        "uid {} abort count != 1; observed: {:?}, scenario: {:?}",
                        uid,
                        observed,
                        scenario
                    );
                    prop_assert!(
                        abort_positions[0] > submit_idx,
                        "uid {} abort at {} not after its submit at {}; observed: {:?}, scenario: {:?}",
                        uid,
                        abort_positions[0],
                        submit_idx,
                        observed,
                        scenario
                    );
                }
            }
        }
    }
}
