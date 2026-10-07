---
phase: 07-frontend-benchmarks
plan: 01
subsystem: benchmarking
tags: [rust, hdrhistogram, reqwest, nix, sysinfo, sse, process-groups, tdd]

requires:
  - phase: 01-rust-frontend-skeleton
    provides: "crates/rsg-server, rsg-wire conventions (workspace Cargo.toml style, clap CLI pattern, tests/common subprocess harness pattern) copied into the new crate"
provides:
  - "New workspace member crate rsg-bench (lib rsg_bench + bin bench-stub)"
  - "rsg_bench::sse::{SseData, SseLineSplitter} — hand-written data: line parser, chunk-boundary safe"
  - "rsg_bench::client::{local_base_url, build_client, fetch_model_id, ChatRequest, CancelPlan, Outcome, RequestRecord, stream_chat} — loopback-only streaming chat client with client-side cancellation"
  - "rsg_bench::metrics::{HIST_MAX_US, LatencyHistograms, LatencySummary, Percentiles, percentile_ms} — hdrhistogram TTFT/ITL/E2E recording"
  - "rsg_bench::procs::{LaunchSpec, ServerHandle, TeardownReport, ensure_port_free, launch, wait_ready, leader_exited, teardown, group_members_alive, check_signalable_pgid} — process-group launch/teardown with pgid safety guards and a Drop guard"
  - "bench-stub test-fixture HTTP/SSE server (127.0.0.1-only) with server-observed disconnect detection, --fail-every, --spawn-child, --idle, --ready-delay-ms, --marker-after-ms"
  - "Test helpers tests/common/{free_port, stub_bin, stub_spec, read_stub_events, wait_for_event, StubEvent}"
affects: [07-04-load-generator, 07-10-mac-dev-pass]

actuals:
  tokens: 13500
  tasks: 2
  commits: 2

tech-stack:
  added: [hdrhistogram 7.6.0, nix 0.31.3, reqwest 0.13.5 (default-features off), sysinfo 0.39.6]
  patterns:
    - "Hand-written SSE data: line splitter buffering across chunk boundaries (eventsource-stream ruled out as unmaintained)"
    - "Loopback-only HTTP client: local_base_url is the crate's only base-URL constructor, client has every proxy disabled"
    - "Process-group launch/teardown: CommandExt::process_group(0) at spawn, SIGINT-then-SIGKILL at teardown, non-reaping until the final SIGKILL so the leader's zombie keeps the pgid reserved, ESRCH/EPERM treated as already-gone"
    - "Server-observed disconnect via split read/write halves: a watcher task reads to EOF/error and fires a oneshot; tokio::select! against the TTFT/ITL sleep catches a prefill-stage abort immediately instead of only on the next write"
    - "check_signalable_pgid guard: every killpg call (teardown and Drop) routes through one function that rejects pgid <= 1 and the harness's own process group"

key-files:
  created:
    - crates/rsg-bench/Cargo.toml
    - crates/rsg-bench/src/lib.rs
    - crates/rsg-bench/src/sse.rs
    - crates/rsg-bench/src/client.rs
    - crates/rsg-bench/src/metrics.rs
    - crates/rsg-bench/src/procs.rs
    - crates/rsg-bench/src/bin/bench-stub.rs
    - crates/rsg-bench/tests/common/mod.rs
    - crates/rsg-bench/tests/tracer.rs
    - crates/rsg-bench/tests/teardown.rs
  modified:
    - Cargo.toml (workspace dependency pins for hdrhistogram, nix, reqwest, sysinfo)
    - Cargo.lock

key-decisions:
  - "Implemented client.rs's full CancelPlan (AfterHeaders/AfterChunks) logic in Task 1 rather than deferring to Task 2, since it's simple and correct; Task 2's RED still held because the server-side disconnect proof (bench-stub's watcher/select logic, --fail-every, --spawn-child) didn't exist yet"
  - "reqwest added with default-features = false and no extra features (no 'json'): request/response bodies are built with serde_json::to_vec/from_slice and raw .body()/.bytes() instead of RequestBuilder::json()/Response::json(), since the 'json' feature isn't in this plan's dependency list"
  - "bench-stub's disconnect watcher uses tokio::io::split via TcpStream::into_split() plus a oneshot channel, selected against the per-chunk sleep, so a drop during the TTFT wait is caught without waiting for the next write attempt"

patterns-established:
  - "New rsg-bench crate follows rsg-server's exact Cargo.toml/CLI/tracing-subscriber conventions"
  - "bench-stub is the first Phase 7 fixture to speak OpenAI-style SSE without touching the ZMQ wire protocol — it is a separate concern from mock-scheduler"

requirements-completed: []  # BENCH-02 is shared with 07-04 and 07-10 (shared-ID gate); not all three are done yet, so it stays open in REQUIREMENTS.md per requirements.ready-ids (0/1 ready).

coverage:
  - id: D1
    description: "rsg-bench crate scaffolding and workspace dependency pins (hdrhistogram/nix/reqwest/sysinfo)"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "cargo build -p rsg-bench"
        status: pass
    human_judgment: false
  - id: D2
    description: "Tracer round trip: launch bench-stub in its own process group, wait_ready, stream_chat (CancelPlan::None), record TTFT/ITL/E2E in LatencyHistograms, teardown with zero survivors"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/tracer.rs#tracer_one_request_end_to_end"
        status: pass
    human_judgment: false
  - id: D3
    description: "Client-side mid-stream cancellation (AfterChunks) proven by a server-observed disconnect event, with no done event for that request"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/tracer.rs#cancel_after_chunks_disconnects"
        status: pass
    human_judgment: false
  - id: D4
    description: "Client-side headers-only cancellation (AfterHeaders) proven by a server-observed disconnect during the TTFT/prefill wait, logged before the stub's TTFT deadline would have elapsed"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/tracer.rs#cancel_after_headers_disconnects_during_prefill"
        status: pass
    human_judgment: false
  - id: D5
    description: "A non-200 chat-completion response (--fail-every) is recorded as Outcome::Failed with error 'status 500', and the stub logs kind=failed"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/tracer.rs#failed_status_is_recorded"
        status: pass
    human_judgment: false
  - id: D6
    description: "SseLineSplitter yields the same payload sequence regardless of chunk boundaries or CRLF-vs-LF line endings (property test)"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "crates/rsg-bench/src/sse.rs#chunk_boundary_proptests::yields_payloads_then_done_regardless_of_chunk_boundaries"
        status: pass
    human_judgment: false
  - id: D7
    description: "Teardown reaches a grandchild process (--spawn-child) and check_signalable_pgid refuses pgid <= 1 and the harness's own process group"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/teardown.rs#teardown_kills_grandchild"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/teardown.rs#refuses_unsafe_pgids"
        status: pass
    human_judgment: false

duration: 55min
completed: 2026-10-07
status: complete
---

# Phase 07 Plan 01: Tracer Round Trip for the rsg-bench Harness Summary

**New `rsg-bench` crate proves launch -> stream -> cancel -> teardown end to end against a hand-rolled OpenAI-SSE test fixture, with hdrhistogram TTFT/ITL/E2E recording and process-group-safe teardown.**

## Performance
- **Duration:** 55min
- **Started:** 2026-10-07T02:54:00Z
- **Completed:** 2026-10-07T03:49:00Z
- **Tasks:** 2
- **Files modified:** 11 (9 created, 2 modified: Cargo.toml, Cargo.lock)

## Accomplishments
- New workspace member `crates/rsg-bench` (lib `rsg_bench` + bin `bench-stub`), with exactly the four new workspace dependency pins the RESEARCH Package Legitimacy Audit approved
- `bench-stub`: a 127.0.0.1-only HTTP/SSE test fixture that serves `/v1/models`, `/health`, and streamed `/v1/chat/completions`, and detects a client disconnect server-side even during the TTFT (prefill) wait
- `rsg_bench::sse::SseLineSplitter`: hand-written, chunk-boundary-safe `data: ` line parser, covered by unit tests and a proptest
- `rsg_bench::client`: loopback-only streaming chat client with `CancelPlan::{None, AfterHeaders, AfterChunks}` client-side cancellation
- `rsg_bench::metrics::LatencyHistograms`: hdrhistogram-backed TTFT/ITL/E2E recording with percentile summaries
- `rsg_bench::procs`: process-group launch/wait_ready/teardown, with `check_signalable_pgid` guarding every `killpg` call (teardown and `ServerHandle`'s `Drop`) against signalling pgid <= 1 or the harness's own group
- Six integration/unit tests pass: `tracer_one_request_end_to_end`, `cancel_after_chunks_disconnects`, `cancel_after_headers_disconnects_during_prefill`, `failed_status_is_recorded`, `teardown_kills_grandchild`, `refuses_unsafe_pgids`, plus the SSE proptest and four SSE unit tests

## Task Commits
1. **Task 1: Tracer — launch bench-stub, stream one chat completion, record TTFT, tear the group down** - `d621d96` (feat)
2. **Task 2: Client-side cancellation with server-observed disconnects, SSE chunk-boundary property, teardown guards** - `4ebe1f7` (feat)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/Cargo.toml` - new crate manifest, workspace-dep references
- `crates/rsg-bench/src/lib.rs` - crate doc comment, `pub mod client/metrics/procs/sse`
- `crates/rsg-bench/src/sse.rs` - `SseData`/`SseLineSplitter`, unit tests, chunk-boundary proptest
- `crates/rsg-bench/src/client.rs` - `local_base_url`, `build_client`, `fetch_model_id`, `ChatRequest`, `CancelPlan`, `Outcome`, `RequestRecord`, `stream_chat`
- `crates/rsg-bench/src/metrics.rs` - `HIST_MAX_US`, `LatencyHistograms`, `LatencySummary`, `Percentiles`, `percentile_ms`
- `crates/rsg-bench/src/procs.rs` - `LaunchSpec`, `ServerHandle`, `TeardownReport`, `ensure_port_free`, `launch`, `wait_ready`, `leader_exited`, `teardown`, `group_members_alive`, `check_signalable_pgid`, `safe_killpg`, `Drop for ServerHandle`
- `crates/rsg-bench/src/bin/bench-stub.rs` - clap CLI, raw HTTP/1.1 parsing, SSE chunk streaming, disconnect watcher, `--fail-every`/`--spawn-child`/`--idle`/`--ready-delay-ms`/`--marker-after-ms`
- `crates/rsg-bench/tests/common/mod.rs` - `free_port`, `stub_bin`, `unique_log_path`, `stub_spec`, `StubEvent`, `read_stub_events`, `wait_for_event`
- `crates/rsg-bench/tests/tracer.rs` - `tracer_one_request_end_to_end`, `cancel_after_chunks_disconnects`, `cancel_after_headers_disconnects_during_prefill`, `failed_status_is_recorded`
- `crates/rsg-bench/tests/teardown.rs` - `teardown_kills_grandchild`, `refuses_unsafe_pgids`
- `Cargo.toml` - added `hdrhistogram`, `nix`, `reqwest`, `sysinfo` to `[workspace.dependencies]`
- `Cargo.lock` - resolved the new dependency tree

## Decisions Made
- Implemented the full `CancelPlan` match (including `AfterHeaders`/`AfterChunks`) in `client.rs` during Task 1 instead of stubbing it, since the logic is simple and self-contained. This didn't invalidate Task 2's TDD RED phase: the behaviors Task 2 tests (server-observed disconnect, `--fail-every`, `--spawn-child`, pgid guards) all still required real stub/procs changes that didn't exist yet.
- `reqwest` carries `default-features = false` with no additional features (per the plan's exact dependency list, no `"json"` feature). Request/response JSON is handled manually via `serde_json::to_vec`/`from_slice` over `.body()`/`.bytes()` rather than `RequestBuilder::json()`/`Response::json()`, which require the `json` feature.
- `bench-stub`'s prefill-stage disconnect detection uses `TcpStream::into_split()` into owned read/write halves, a watcher task that reads to EOF/error and fires a `tokio::sync::oneshot`, and `tokio::select!` between that signal and the per-chunk sleep — this is what makes `cancel_after_headers_disconnects_during_prefill` observe the disconnect in well under the stub's 800ms TTFT deadline, rather than only on the next write attempt (RESEARCH Pitfall 5 / Assumption A1).

## Deviations from Plan

### Auto-fixed Issues

None — no bugs, missing functionality, or blocking issues required deviation from the plan. Every workspace/crate dependency, module shape, and CLI flag matches the plan's `<interfaces>` contract exactly.

**Total deviations:** 0.
**Impact:** None.

## TDD Gate Compliance (Task 2, tdd="true")

`workflow.tdd_mode` is off for this project run, so the orchestrator-level RED-commit hard gate did not apply, but the RED-GREEN discipline was followed and verified before committing:

- **RED (verified, not a separate commit):** added `tests/teardown.rs` (`teardown_kills_grandchild`, `refuses_unsafe_pgids`), the three new `tests/tracer.rs` tests, and the SSE chunk-boundary proptest, plus an intentionally-wrong placeholder `check_signalable_pgid` (`Ok(())` always) so the test crate would compile. Ran `cargo test -p rsg-bench`:
  - `refuses_unsafe_pgids` — **failed** (placeholder always allows; genuine RED)
  - `teardown_kills_grandchild` — **failed**, `--spawn-child` unrecognized by clap (genuine RED)
  - `failed_status_is_recorded` — **failed**, `--fail-every` unrecognized by clap (genuine RED)
  - `cancel_after_headers_disconnects_during_prefill` — **failed**, stub logged `tokens=1` instead of `tokens=0` (write-failure-only detection doesn't fire during the prefill sleep; genuine RED)
  - `cancel_after_chunks_disconnects` — **passed immediately**. Task 1's bench-stub already treats a failed in-stream write as a disconnect, and this test's tolerance (3-5 tokens) is loose enough that the naive write-failure path already satisfies it. Investigated: this is not a bug, just overlap between Task 1's baseline behavior and Task 2's tolerance band; no fix needed.
  - SSE chunk-boundary proptest — **passed immediately**. `sse.rs`'s splitter was already fully and correctly implemented in Task 1 (required for the Task 1 tracer test itself); the proptest adds regression coverage for already-correct behavior rather than exercising new behavior.
- **GREEN:** implemented the watcher/select disconnect detection, `--fail-every`, `--spawn-child`, `--idle`, `--ready-delay-ms`, `--marker-after-ms`, the `kind=env` startup line, and the real `check_signalable_pgid` wired into `teardown`/`Drop`. Re-ran `cargo test -p rsg-bench` (3 consecutive runs) and `cargo clippy -p rsg-bench --all-targets -- -D warnings`: all green, no flakiness observed.
- **REFACTOR:** none needed; no separate refactor commit.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required. All four new crates (`hdrhistogram`, `nix`, `reqwest`, `sysinfo`) were pre-approved in the 07-RESEARCH.md Package Legitimacy Audit.

## Next Phase Readiness
`rsg_bench::{client, metrics, procs, sse}` and `bench-stub` are ready for 07-04 (load generator) to build on: the `stream_chat`/`CancelPlan`/`RequestRecord`/`LatencyHistograms` contract and the `procs::{launch, wait_ready, teardown}` contract are both exercised end-to-end here. No blockers. `BENCH-02` stays open in REQUIREMENTS.md (shared with 07-04 and 07-10 per the shared-ID gate; `requirements.ready-ids` reports 0/1 ready until those plans also complete).

## Self-Check: PASSED

All 10 created files plus this SUMMARY.md confirmed present on disk; both task commits (`d621d96`, `4ebe1f7`) confirmed in `git log`.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07*
