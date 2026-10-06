---
phase: 05-request-lifecycle-http-api
plan: 06
subsystem: observability
tags: [prometheus, metrics, axum, health-check, rust-frontend]

requires:
  - phase: 05-03
    provides: "rsg_server::http::{AppState, router, serve, ApiError}; the existing /generate, /v1/chat/completions, /v1/models, /v1 routes this plan adds /health, /health/ready, /metrics alongside"
  - phase: 05-04
    provides: "rsg_server::engine::{Engine::dispatch_stats, DispatchStatsSnapshot}; the cancellation/timeout/overlong driver paths this plan's registry-level metrics observe"
provides:
  - "rsg_server::metrics::{ServerMetrics, TTFT_BUCKETS_SECONDS, METRICS_CONTENT_TYPE} — a per-server PrometheusRecorder, never process-global"
  - "rsg_server::fsm::spawn_registry(metrics: ServerMetrics) — records request/terminal/active/TTFT metrics on every applied transition"
  - "rsg_server::http::{AppState::new(model, metrics), AppState::metrics()} and GET /health, /health/ready, /metrics"
affects: [05-07, 05-08, 05-09, 06, 07]

actuals:
  tokens: 9226
  tasks: 2
  commits: 3
  plan_head_before: 28bf8064743c9a9a606bbb5f93c86d38253ef3c4
  plan_head_after: 9668fb04b8e5df54ebcc12e7cd01f45e3bd80f69

tech-stack:
  added:
    - "metrics 0.24.6"
    - "metrics-exporter-prometheus 0.18.3 (default-features = false)"
  patterns:
    - "A per-server PrometheusRecorder, built via PrometheusBuilder::new().build_recorder() and kept only inside ServerMetrics — never installed process-wide (no set_global_recorder/install_recorder anywhere), so parallel in-process test servers stay isolated from each other"
    - "Every counter/gauge is touched once at ServerMetrics::new() construction (increment(0)/set(0.0)) so all seven series render from the very first scrape, even before any request exists"
    - "rsg_late_tokens_dropped_total is computed at scrape time, not accumulated event-by-event: render(dispatch) calls Counter::absolute(unknown_uid + closed_route) from the dispatcher's own live snapshot, so it can never drift from the single source of truth"

key-files:
  created:
    - crates/rsg-server/src/metrics.rs
    - crates/rsg-server/src/http/health.rs
    - crates/rsg-server/tests/observability.rs
  modified:
    - Cargo.toml
    - crates/rsg-server/Cargo.toml
    - crates/rsg-server/src/lib.rs
    - crates/rsg-server/src/fsm/mod.rs
    - crates/rsg-server/src/http/mod.rs
    - crates/rsg-server/tests/common/test_server.rs
    - crates/rsg-server/tests/http_models.rs

key-decisions:
  - "No labels on any of the seven series (T-05-13/T-05-14): cardinality is fixed regardless of traffic, and nothing request-specific (uid, model text, prompt) can appear in the exposition"
  - "TTFT buckets in seconds: 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30 — Claude's discretion per CONTEXT, sized to span a fast mock round-trip up to a multi-second tail"
  - "/health means process liveness only (always 200 once the HTTP listener is up); /health/ready means the readiness handshake has been received and AppState::set_engine called — the front-half/end-to-end split Phase 7's cold-start scenario measures"
  - "spawn_registry's TTFT recording reads the pre-advance `from` state before calling ReqState::advance (which mutates state in place), since the Submitted -> Decoding check needs both the state before and after the transition in the same match arm"

patterns-established:
  - "ServerMetrics::render(dispatch: Option<DispatchStatsSnapshot>) is the one place rsg_late_tokens_dropped_total gets set, always from a live dispatch_stats() snapshot passed in by the caller, never from an internally-accumulated counter that could diverge"

requirements-completed: [API-02]

coverage:
  - id: D1
    description: "GET /metrics returns 200 with content-type text/plain; version=0.0.4; charset=utf-8 and every required Prometheus series (rsg_requests_total, rsg_requests_cancelled_total, rsg_requests_finished_total, rsg_requests_failed_total, rsg_requests_active, rsg_late_tokens_dropped_total, and the rsg_ttft_seconds histogram with _bucket/_sum/_count) present at value 0 at startup; after one finished request, the counts update correctly and a rsg_ttft_seconds_bucket{le=\"+Inf\"} line shows 1"
    requirement: "API-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/observability.rs#tracer_metrics_count_a_finished_request"
        status: pass
    human_judgment: false
  - id: D2
    description: "GET /health always returns 200 {\"status\":\"ok\"} once the listener is up; GET /health/ready returns 503 {\"status\":\"starting\"} until the engine is set and 200 {\"status\":\"ready\"} afterwards"
    requirement: "API-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/observability.rs#health_and_readiness_before_and_after_engine"
        status: pass
    human_judgment: false
  - id: D3
    description: "A cancelled streaming request and late tokens dropped after its abort are both counted: rsg_requests_cancelled_total, rsg_ttft_seconds_count and rsg_late_tokens_dropped_total all reflect the dispatcher's own unknown_uid+closed_route sum, alongside a concurrently finished request"
    requirement: "API-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/observability.rs#cancelled_request_and_late_tokens_are_counted"
        status: pass
    human_judgment: false
  - id: D4
    description: "A request that times out against a silent backend is counted in rsg_requests_failed_total with no TTFT observation (rsg_ttft_seconds_count stays 0, since it never reached Decoding)"
    requirement: "API-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/observability.rs#failed_request_is_counted"
        status: pass
    human_judgment: false
  - id: D5
    description: "Under 16 concurrent in-flight requests, repeated /metrics scrapes every 10ms each parse cleanly, rsg_requests_total and rsg_requests_finished_total never decrease across scrapes, finished never exceeds total, and the final scrape shows total 16, finished 16, active 0"
    requirement: "API-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/observability.rs#concurrent_scrapes_are_monotonic"
        status: pass
    human_judgment: false

duration: 50min
completed: 2026-10-06
status: complete
---

# Phase 5 Plan 6: Observability — /health, /health/ready, /metrics Summary

**A per-server Prometheus recorder (`ServerMetrics`) feeds `GET /metrics` with the request count, cancellation count, failure count, active gauge, late-token-drop count and a TTFT histogram, all sourced from the same lifecycle registry that enforces exactly-one-terminal-state; `GET /health`/`GET /health/ready` separate process liveness from backend readiness.**

## Performance

- **Duration:** ~50 min
- **Completed:** 2026-10-06
- **Tasks:** 2
- **Files modified:** 10 (3 created, 7 modified)

## Accomplishments
- `rsg_server::metrics::ServerMetrics` builds its own private `PrometheusRecorder` (via `PrometheusBuilder::new().build_recorder()`), registers seven series through the `metrics::Recorder` trait, and touches every counter/gauge once at construction so `/metrics` shows every series at value 0 from the very first scrape — proven by the tracer test before any request is ever made.
- `fsm::spawn_registry(metrics)` now records a request on `Received`, a TTFT observation (a monotonic `Instant` difference, never the wall clock) on the `Submitted -> Decoding` transition, a terminal count on every terminal state, and the active gauge after every applied report — the exact same table that already proves LIFE-01's exactly-one-terminal-state invariant now also drives `/metrics`.
- `GET /health` is unconditionally 200 once the listener is up; `GET /health/ready` is 503 `{"status":"starting"}` until `AppState::set_engine` has been called and 200 `{"status":"ready"}` afterward — proven both before and after a real engine is attached.
- A cancelled streaming request and the 3 late tokens its abort produces are both counted (`rsg_requests_cancelled_total`, `rsg_late_tokens_dropped_total`), a backend timeout is counted as failed with no TTFT observation, and 16 concurrently in-flight requests scraped every 10ms never show `/metrics` counts going backwards or `finished` exceeding `total`.

## Task Commits

Each task was committed atomically (Task 2 followed the TDD RED/GREEN cycle per its `tdd="true"` attribute):

1. **Task 1: Tracer — one finished request shows up in /metrics as a request, a finish and a TTFT observation** - `9113b25` (feat)
2. **Task 2: /health, /health/ready, cancellation and late-token counters, and consistency under concurrent scrapes (RED)** - `10cae7c` (test)
3. **Task 2: /health, /health/ready, cancellation and late-token counters, and consistency under concurrent scrapes (GREEN)** - `9668fb0` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/src/metrics.rs` - `ServerMetrics` (new, record_received, record_terminal, set_active, record_ttft, render), `TTFT_BUCKETS_SECONDS`, `METRICS_CONTENT_TYPE`
- `crates/rsg-server/src/http/health.rs` - `health`/`ready`/`metrics` handlers for `/health`, `/health/ready`, `/metrics`
- `crates/rsg-server/src/fsm/mod.rs` - `spawn_registry(metrics: ServerMetrics)` records every transition into the new metrics
- `crates/rsg-server/src/http/mod.rs` - `AppState` holds `ServerMetrics`, `AppState::metrics()`, registers the three new routes
- `crates/rsg-server/src/lib.rs` - `pub mod metrics;`
- `Cargo.toml`, `crates/rsg-server/Cargo.toml` - add `metrics 0.24.6`, `metrics-exporter-prometheus 0.18.3` (default features off)
- `crates/rsg-server/tests/common/test_server.rs` - `TestServer::start` builds `ServerMetrics::new()` and passes it to `spawn_registry`/`AppState::new`, signature unchanged
- `crates/rsg-server/tests/http_models.rs` - `not_ready_routes_return_503` updated to the two-argument `AppState::new`
- `crates/rsg-server/tests/observability.rs` - 5 tests: the Task 1 tracer plus 4 Task 2 behavior tests

## Decisions Made
- No labels on any series (T-05-13/T-05-14): fixed cardinality of seven series regardless of traffic, nothing request-specific can leak into the exposition.
- TTFT buckets (seconds): 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30 — Claude's discretion per CONTEXT, spanning a fast mock round-trip through a multi-second tail.
- `/health` = process liveness only; `/health/ready` = backend-readiness-handshake-received — the front-half/end-to-end split Phase 7's cold-start scenario will measure.
- `spawn_registry`'s Occupied-entry branch now captures `let from = entry.state;` before calling `entry.advance(to, at)` (which mutates `entry.state` in place), since the `Submitted -> Decoding` TTFT check needs both the pre- and post-transition state in the same match arm.
- `ServerMetrics::render` sets `rsg_late_tokens_dropped_total` via `Counter::absolute(unknown_uid + closed_route)` from a live `DispatchStatsSnapshot` passed in by the caller at scrape time, rather than accumulating the count itself — this keeps the dispatcher's own counters as the single source of truth, with no risk of drift between two independently-incremented counts.

## Deviations from Plan

None - plan executed exactly as written. The plan's suggested `metrics::Key::from_static_name` does not exist in metrics 0.24.6 (verified by reading the crate source at `~/.cargo/registry/src/.../metrics-0.24.6/src/key.rs`); `Key::from_name("...")` with a `&'static str` argument is the equivalent API in this version and was used instead — a naming correction, not a behavior or scope change.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- `rsg_server::metrics::ServerMetrics` and the `/health`/`/health/ready`/`/metrics` routes are locked in exactly as this plan's interfaces-block contract specifies; `spawn_registry(metrics)` and `AppState::new(model, metrics)` are the new signatures every later plan in this phase (05-07, 05-08, 05-09) and Phase 6/7 must build against.
- Phase 7's benchmark harness can scrape `/metrics` directly for TTFT percentiles cross-checking, and Phase 7's cold-start scenario can poll `/health/ready` for the front-half/end-to-end timing split named in PROJECT.md's Key Decisions.
- No blockers.

## Self-Check: PASSED

All created/modified files verified present on disk; all 3 task commits
(`9113b25`, `10cae7c`, `9668fb0`) verified present in `git log`.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*
