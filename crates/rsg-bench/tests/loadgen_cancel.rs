//! Closed-loop seeded agents (Task 1): client-side outcome counts reconcile
//! with the stub's server-observed events, and the planned workload is
//! deterministic for a fixed seed (BENCH-02/03, D-01/D-02).
//!
//! Open-loop Poisson and fixed-concurrency drivers (Task 2): both share the
//! same recording path and respect their scheduling contracts (BENCH-02).

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use rsg_bench::client;
use rsg_bench::loadgen::{self, AgentParams, ClosedParams, OpenLoopParams};
use rsg_bench::procs;
use rsg_bench::rng::SplitMix64;

#[tokio::test(flavor = "multi_thread")]
async fn loadgen_counts_match_server() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(
        port,
        &log_path,
        &["--ttft-ms", "20", "--itl-ms", "5", "--fail-every", "7"],
    );

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(&handle, &client, port, Duration::from_secs(10), Duration::from_millis(50))
        .await
        .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let params = AgentParams {
        agents: 16,
        duration: Duration::from_secs(3),
        cancel_fraction: 0.5,
        max_tokens: 20,
        think_max: Duration::from_millis(50),
        seed: 7,
        prompt_words: (16, 128),
    };

    let result = loadgen::run_agents(&client, &base_url, &model, &params).await;
    let summary = loadgen::summarize(&result);

    // Poll the stub log for up to 2s until its event count stops changing:
    // the stub's event lines are flushed synchronously as they're written,
    // but this process may still be draining its own task joins.
    let mut prev_len = usize::MAX;
    let poll_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let len = common::read_stub_events(&log_path).len();
        if len == prev_len {
            break;
        }
        prev_len = len;
        if Instant::now() > poll_deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let events = common::read_stub_events(&log_path);
    let done = events.iter().filter(|e| e.kind == "done").count() as u64;
    let disconnect = events.iter().filter(|e| e.kind == "disconnect").count() as u64;
    let failed = events.iter().filter(|e| e.kind == "failed").count() as u64;

    assert_eq!(summary.counts.completed, done, "events: {events:?}");
    assert_eq!(summary.counts.cancelled, disconnect, "events: {events:?}");
    assert_eq!(summary.counts.failed, failed, "events: {events:?}");
    assert_eq!(
        summary.counts.sent,
        summary.counts.completed + summary.counts.cancelled + summary.counts.failed,
        "counts: {:?}",
        summary.counts
    );
    assert!(summary.counts.cancelled > 0, "counts: {:?}", summary.counts);
    assert!(summary.counts.completed > 0, "counts: {:?}", summary.counts);
    assert!(summary.counts.failed > 0, "counts: {:?}", summary.counts);

    procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
}

/// The planned workload for a fixed seed is identical across calls (D-01).
#[test]
fn plan_is_deterministic() {
    let params = AgentParams {
        agents: 16,
        duration: Duration::from_secs(3),
        cancel_fraction: 0.5,
        max_tokens: 20,
        think_max: Duration::from_millis(50),
        seed: 7,
        prompt_words: (16, 128),
    };

    let mut rng_a = SplitMix64::new(42).fork(3);
    let mut rng_b = SplitMix64::new(42).fork(3);
    for _ in 0..50 {
        let a = loadgen::plan_agent_request(&mut rng_a, &params);
        let b = loadgen::plan_agent_request(&mut rng_b, &params);
        assert_eq!(a, b);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn open_loop_respects_schedule() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--ttft-ms", "5", "--itl-ms", "2"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(&handle, &client, port, Duration::from_secs(10), Duration::from_millis(50))
        .await
        .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let params = OpenLoopParams {
        rate_rps: 50.0,
        requests: 50,
        max_tokens: 4,
        prompt_words: (1, 32),
        seed: 11,
    };

    let offsets = loadgen::poisson_offsets(params.rate_rps, params.requests as usize, params.seed);
    let last_offset = *offsets.last().expect("at least one offset");

    let t0 = Instant::now();
    let result = loadgen::run_open_loop(&client, &base_url, &model, &params).await;
    let elapsed = t0.elapsed();

    let summary = loadgen::summarize(&result);
    assert_eq!(summary.counts.completed, 50, "counts: {:?}", summary.counts);

    let last_send_ns = result
        .records
        .iter()
        .map(|r| r.t_send_unix_ns)
        .max()
        .expect("at least one record");
    let planned_last_ns = result.window_start_unix_ns + last_offset.as_nanos() as u64;
    // 2ms slack for clock/scheduling granularity: the assertion is
    // one-directional (no send ahead of its planned offset).
    assert!(
        last_send_ns + 2_000_000 >= planned_last_ns,
        "last send {last_send_ns} earlier than planned {planned_last_ns}"
    );
    assert!(elapsed < Duration::from_secs(3), "elapsed {elapsed:?} >= 3s");

    procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_loop_bounds_concurrency() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--ttft-ms", "50", "--itl-ms", "5"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(&handle, &client, port, Duration::from_secs(10), Duration::from_millis(50))
        .await
        .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let params = ClosedParams {
        concurrency: 4,
        requests: 20,
        max_tokens: 1,
        prompt_words: (1, 32),
        seed: 13,
    };

    let result = loadgen::run_closed(&client, &base_url, &model, &params).await;
    let summary = loadgen::summarize(&result);
    assert_eq!(summary.counts.completed, 20, "counts: {:?}", summary.counts);
    assert!(
        result.window >= Duration::from_millis(250) && result.window <= Duration::from_millis(900),
        "window {:?} not in [250ms, 900ms]",
        result.window
    );

    procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
}
