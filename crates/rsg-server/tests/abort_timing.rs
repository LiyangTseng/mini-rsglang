//! Abort timing (immediate vs. deferred), at the engine level (CONTEXT
//! D-01): `Immediate` aborts as soon as cancellation is observed, even
//! during prefill; `Deferred` waits for the first token (or the backend
//! timeout) before sending the abort. Cancellation at any point in a
//! request's lifecycle must leave no orphaned backend request (LIFE-01).

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use rsg_server::codec::Prompt;
use rsg_server::engine::{AbortTiming, RequestEvent};
use rsg_wire::SamplingParams;

use common::Observed;
use common::test_server::{TestConfig, TestServer};

fn sampling(max_tokens: i64) -> SamplingParams {
    SamplingParams {
        max_tokens,
        ..SamplingParams::default()
    }
}

fn has_abort(observed: &[Observed], uid: i64) -> bool {
    observed
        .iter()
        .any(|o| matches!(o, Observed::Abort { uid: u } if *u == uid))
}

fn has_submit(observed: &[Observed], uid: i64) -> bool {
    observed
        .iter()
        .any(|o| matches!(o, Observed::Submit { uid: u, .. } if *u == uid))
}

/// Polls `server.mock.observed()` every 5ms until `pred` holds. Panics
/// after `deadline`.
async fn wait_for_observed(
    server: &TestServer,
    deadline: Duration,
    pred: impl Fn(&[Observed]) -> bool,
) {
    let start = Instant::now();
    loop {
        let observed = server.mock.observed();
        if pred(&observed) {
            return;
        }
        assert!(
            start.elapsed() < deadline,
            "timed out waiting for observed predicate; last observed: {observed:?}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tracer_immediate_aborts_during_prefill() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "800", "--decode-delay-ms", "10"],
        TestConfig {
            abort_timing: AbortTiming::Immediate,
            ..TestConfig::default()
        },
    )
    .await;

    let mut active = server
        .engine
        .start(Prompt::Text("abc".to_string()), sampling(50));
    active.accepted().await.expect("accepted");
    wait_for_observed(&server, Duration::from_secs(2), |o| has_submit(o, 0)).await;

    let t0 = Instant::now();
    drop(active);

    wait_for_observed(&server, Duration::from_secs(2), |o| has_abort(o, 0)).await;
    assert!(
        t0.elapsed() < Duration::from_millis(400),
        "abort took too long: {:?}",
        t0.elapsed()
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.cancelled, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deferred_waits_for_first_token() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "800", "--decode-delay-ms", "10"],
        TestConfig {
            abort_timing: AbortTiming::Deferred,
            ..TestConfig::default()
        },
    )
    .await;

    let mut active = server
        .engine
        .start(Prompt::Text("abc".to_string()), sampling(50));
    active.accepted().await.expect("accepted");
    wait_for_observed(&server, Duration::from_secs(2), |o| has_submit(o, 0)).await;

    let t0 = Instant::now();
    drop(active);

    wait_for_observed(&server, Duration::from_secs(2), |o| has_abort(o, 0)).await;
    let elapsed = t0.elapsed();
    assert!(
        elapsed >= Duration::from_millis(700),
        "abort too early: {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_millis(1500),
        "abort too late: {elapsed:?}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.cancelled, 1);
    assert_eq!(snap.finished, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deferred_after_first_token_aborts_immediately() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "0", "--decode-delay-ms", "50"],
        TestConfig {
            abort_timing: AbortTiming::Deferred,
            ..TestConfig::default()
        },
    )
    .await;

    let mut active = server
        .engine
        .start(Prompt::Text("abc".to_string()), sampling(100));
    active.accepted().await.expect("accepted");
    match active.next_event().await {
        Some(RequestEvent::Token { .. }) => {}
        other => panic!("expected a Token event, got {other:?}"),
    }

    let t0 = Instant::now();
    drop(active);

    wait_for_observed(&server, Duration::from_secs(2), |o| has_abort(o, 0)).await;
    assert!(
        t0.elapsed() < Duration::from_millis(300),
        "abort took too long: {:?}",
        t0.elapsed()
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.cancelled, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deferred_single_token_request_sends_no_abort() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "300", "--decode-delay-ms", "10"],
        TestConfig {
            abort_timing: AbortTiming::Deferred,
            ..TestConfig::default()
        },
    )
    .await;

    let mut active = server
        .engine
        .start(Prompt::Text("abc".to_string()), sampling(1));
    active.accepted().await.expect("accepted");
    drop(active);

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.cancelled, 1);

    let observed = server.mock.observed();
    assert!(has_submit(&observed, 0), "expected a Submit for uid 0");
    assert!(
        !has_abort(&observed, 0),
        "expected no Abort for uid 0, got {observed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deferred_backend_silence_still_aborts_at_timeout() {
    let server = TestServer::start(
        &["--misbehave-uids", "0", "--behavior", "drop-overlong"],
        TestConfig {
            abort_timing: AbortTiming::Deferred,
            backend_timeout_ms: 300,
        },
    )
    .await;

    let mut active = server
        .engine
        .start(Prompt::Text("abc".to_string()), sampling(50));
    active.accepted().await.expect("accepted");

    let t0 = Instant::now();
    drop(active);

    wait_for_observed(&server, Duration::from_secs(2), |o| has_abort(o, 0)).await;
    let elapsed = t0.elapsed();
    assert!(
        elapsed >= Duration::from_millis(250),
        "abort too early: {elapsed:?}"
    );
    assert!(
        elapsed <= Duration::from_millis(1500),
        "abort too late: {elapsed:?}"
    );

    let snap = server.snapshot_when_idle(Duration::from_secs(2)).await;
    assert_eq!(snap.cancelled, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_at_any_point_leaves_no_orphan() {
    let server = TestServer::start(
        &["--prefill-delay-ms", "2", "--decode-delay-ms", "2"],
        TestConfig {
            abort_timing: AbortTiming::Immediate,
            ..TestConfig::default()
        },
    )
    .await;

    let start = Instant::now();
    let actives: Vec<_> = (0..30i64)
        .map(|_| {
            server
                .engine
                .start(Prompt::Text("abcdefgh".to_string()), sampling(20))
        })
        .collect();

    let mut handles = Vec::new();
    for (i, active) in actives.into_iter().enumerate() {
        let drop_at = start + Duration::from_millis(((i % 10) * 3) as u64);
        handles.push(tokio::spawn(async move {
            let now = Instant::now();
            if drop_at > now {
                tokio::time::sleep(drop_at - now).await;
            }
            drop(active);
        }));
    }
    for h in handles {
        h.await.expect("drop task panicked");
    }

    let snap = server.snapshot_when_idle(Duration::from_secs(5)).await;
    assert_eq!(snap.cancelled + snap.finished, 30);
    assert_eq!(snap.failed, 0);
    assert_eq!(snap.invalid_transitions, 0);

    // An abort reaching the registry (Cancelled, counted above) only means
    // the abort was enqueued onto the writer's channel, not that the
    // mock-scheduler subprocess has received and recorded it yet in its
    // observe file across the real ipc boundary. Poll for that eventual
    // consistency before checking the observed invariants below.
    let (submits, aborts) = {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let observed = server.mock.observed();
            let submits: Vec<i64> = observed
                .iter()
                .filter_map(|o| match o {
                    Observed::Submit { uid, .. } => Some(*uid),
                    _ => None,
                })
                .collect();
            let aborts: Vec<i64> = observed
                .iter()
                .filter_map(|o| match o {
                    Observed::Abort { uid } => Some(*uid),
                    _ => None,
                })
                .collect();
            if submits.len() == snap.finished as usize + aborts.len() {
                break (submits, aborts);
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for submits == finished + aborts; \
                 submits={}, finished={}, aborts={}",
                submits.len(),
                snap.finished,
                aborts.len()
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };

    let mut seen = std::collections::HashSet::new();
    for &uid in &submits {
        assert!(seen.insert(uid), "uid {uid} submitted twice");
    }
    for &uid in &aborts {
        assert!(submits.contains(&uid), "abort for uid {uid} with no submit");
    }
}
