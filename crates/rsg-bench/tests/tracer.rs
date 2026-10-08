//! Tracer round trip (D-01/D-02/D-03): launch `bench-stub` in its own
//! process group, wait for readiness, stream one chat completion through the
//! hand-written SSE parser, record TTFT/ITL/E2E in hdrhistogram, and tear the
//! process group down.

#[allow(dead_code)]
mod common;

use std::time::Duration;

use rsg_bench::client::{self, CancelPlan, ChatRequest, Outcome};
use rsg_bench::metrics::{self, LatencyHistograms};
use rsg_bench::procs;

#[tokio::test(flavor = "multi_thread")]
async fn tracer_one_request_end_to_end() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--ttft-ms", "50", "--itl-ms", "5"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");

    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model_id = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");
    assert_eq!(model_id, "stub-model");

    let req = ChatRequest {
        model: model_id,
        prompt: "hello".to_string(),
        max_tokens: 8,
    };
    let record = client::stream_chat(&client, &base_url, &req, CancelPlan::None).await;

    assert_eq!(record.outcome, Outcome::Completed, "record: {record:?}");
    assert_eq!(record.chunks, 8, "record: {record:?}");
    let ttft = record.ttft.expect("ttft recorded for a completed request");
    assert!(ttft >= Duration::from_millis(50), "ttft {ttft:?} < 50ms");
    assert_eq!(record.itl.len(), 7, "expected 7 ITL gaps for 8 chunks");

    let mut hist = LatencyHistograms::new();
    hist.record(&record);
    let p50 = metrics::percentile_ms(&hist.ttft_us, 0.50).expect("non-empty ttft histogram");
    assert!(p50 >= 50.0, "ttft p50 {p50} < 50.0ms");

    let report = procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
    assert!(
        report.survivors.is_empty(),
        "survivors: {:?}",
        report.survivors
    );

    procs::ensure_port_free(port).expect("port free after teardown");

    let events = common::read_stub_events(&log_path);
    let done_events: Vec<_> = events.iter().filter(|e| e.kind == "done").collect();
    assert_eq!(done_events.len(), 1, "events: {events:?}");
    assert_eq!(
        done_events[0].fields.get("tokens").map(String::as_str),
        Some("8")
    );
}

/// D-02 / RESEARCH Pitfall 5: a mid-stream client cancel (`AfterChunks(3)`)
/// is observed server-side as a disconnect, with no `done` event for that
/// request.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_after_chunks_disconnects() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--ttft-ms", "10", "--itl-ms", "20"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model_id = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let req = ChatRequest {
        model: model_id,
        prompt: "hello".to_string(),
        max_tokens: 40,
    };
    let record = client::stream_chat(&client, &base_url, &req, CancelPlan::AfterChunks(3)).await;
    assert_eq!(record.outcome, Outcome::Cancelled, "record: {record:?}");
    assert_eq!(record.chunks, 3, "record: {record:?}");

    let ev = common::wait_for_event(
        &log_path,
        |e| e.kind == "disconnect",
        Duration::from_secs(2),
    );
    let tokens: u32 = ev
        .fields
        .get("tokens")
        .and_then(|v| v.parse().ok())
        .expect("disconnect event has a tokens field");
    assert!((3..=5).contains(&tokens), "tokens {tokens} not in [3, 5]");

    let events = common::read_stub_events(&log_path);
    assert!(
        !events.iter().any(|e| e.kind == "done"),
        "unexpected done event: {events:?}"
    );

    let report = procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
    assert!(
        report.survivors.is_empty(),
        "survivors: {:?}",
        report.survivors
    );
}

/// D-02 / RESEARCH Assumption A1: dropping right after the headers aborts
/// during the prefill (TTFT) wait, and the stub notices quickly — not only
/// on its next write attempt.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_after_headers_disconnects_during_prefill() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--ttft-ms", "800"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model_id = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let req = ChatRequest {
        model: model_id,
        prompt: "hello".to_string(),
        max_tokens: 8,
    };
    let t0 = std::time::Instant::now();
    let record = client::stream_chat(&client, &base_url, &req, CancelPlan::AfterHeaders).await;
    let client_elapsed = t0.elapsed();

    assert_eq!(record.outcome, Outcome::Cancelled, "record: {record:?}");
    assert!(record.ttft.is_none(), "record: {record:?}");
    assert!(
        client_elapsed < Duration::from_millis(300),
        "client-side cancel took {client_elapsed:?}"
    );

    let ev = common::wait_for_event(
        &log_path,
        |e| e.kind == "disconnect",
        Duration::from_millis(1500),
    );
    let server_elapsed = t0.elapsed();
    assert_eq!(ev.fields.get("tokens").map(String::as_str), Some("0"));
    assert!(
        server_elapsed < Duration::from_millis(800),
        "disconnect observed {server_elapsed:?} after launch; stub's 800ms TTFT deadline already elapsed"
    );

    procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
}

/// `--fail-every 2`: the second chat request gets a 500, recorded as Failed.
#[tokio::test(flavor = "multi_thread")]
async fn failed_status_is_recorded() {
    let port = common::free_port();
    let log_path = common::unique_log_path();
    let spec = common::stub_spec(port, &log_path, &["--fail-every", "2"]);

    let handle = procs::launch(&spec, port).expect("launch bench-stub");
    let client = client::build_client().expect("build client");
    procs::wait_ready(
        &handle,
        &client,
        port,
        Duration::from_secs(10),
        Duration::from_millis(50),
    )
    .await
    .expect("bench-stub became ready");

    let base_url = client::local_base_url(port);
    let model_id = client::fetch_model_id(&client, &base_url)
        .await
        .expect("fetch_model_id");

    let req = ChatRequest {
        model: model_id,
        prompt: "hello".to_string(),
        max_tokens: 4,
    };
    let first = client::stream_chat(&client, &base_url, &req, CancelPlan::None).await;
    assert_eq!(first.outcome, Outcome::Completed, "first: {first:?}");

    let second = client::stream_chat(&client, &base_url, &req, CancelPlan::None).await;
    assert_eq!(second.outcome, Outcome::Failed, "second: {second:?}");
    assert_eq!(second.error.as_deref(), Some("status 500"));

    let events = common::read_stub_events(&log_path);
    assert!(
        events.iter().any(|e| e.kind == "failed"),
        "events: {events:?}"
    );

    procs::teardown(handle, Duration::from_secs(5))
        .await
        .expect("teardown");
}
