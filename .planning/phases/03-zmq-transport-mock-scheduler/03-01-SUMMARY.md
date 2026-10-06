---
phase: 03-zmq-transport-mock-scheduler
plan: 01
subsystem: transport
tags: [zmq, msgpack, mock-scheduler, tokio, ipc]

requires:
  - phase: 01-vendored-base-wire-codec
    provides: rsg-wire's BackendMsg/TokenizerMsg/Tensor codec, and Phase 1's Endpoint/Role/ZmqTransport and Handshake types
provides:
  - "rsg_server library crate (lib.rs) exposing handshake and transport, consumed by both the rsg-server and mock-scheduler binaries"
  - "rsg_server::transport split into BackendSink/DetokSource traits, ZmqTransport::split, and ZmqSchedulerTransport (PULL backend, PUSH detok) with open/recv_backend/send_detok/shutdown"
  - "Handshake::to_json_line for emitting the Phase 1 readiness schema from a non-launcher process"
  - "mock-scheduler binary: a standalone subprocess that speaks only rsg-wire's existing wire types, emits deterministic echo tokens with fixed prefill/decode delays, and has a full process contract (exit codes 0/1/2/3/4, --observe-file, stdin-EOF guard, SIGINT/SIGTERM)"
  - "tests/common MockScheduler subprocess harness (spawn/wait_ready/frontend/observed/wait_exit/close_stdin/signal/wait_for_log) reused by later Phase 3 plans"
affects: [03-02, 03-03, 03-04, 03-05, 03-06, phase-05, phase-06, phase-07]

actuals:
  tokens: 12100
  tasks: 2
  commits: 3

commits: 3
plan_head_before: 11132653b18fe6bc3ae27e876d9c4f9a1473b9e6
plan_head_after: 6777f794b19bb9d764a89c0f00d76dc364784a78

tech-stack:
  added: []
  patterns:
    - "mock-scheduler as a same-package src/bin/ binary reusing the rsg_server library (D-08), not a separate workspace crate"
    - "tx-zmq/rx-zmq convention made literal: the engine's dedicated OS thread both opens its sockets and uses them, never receiving an already-open socket from another thread"
    - "Readiness travels out-of-band on stdout as the Phase 1 handshake JSON line (BASE-03 schema); observation travels to a side file (--observe-file); neither ever becomes a 9th wire tag"

key-files:
  created:
    - crates/rsg-server/src/lib.rs
    - crates/rsg-server/src/bin/mock-scheduler.rs
    - crates/rsg-server/tests/common/mod.rs
    - crates/rsg-server/tests/mock_scheduler_process.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - crates/rsg-server/Cargo.toml
    - crates/rsg-server/src/main.rs
    - crates/rsg-server/src/handshake.rs
    - crates/rsg-server/src/transport.rs

key-decisions:
  - "mock-scheduler reuses rsg_server::transport's Endpoint/Role/ZmqSchedulerTransport unmodified and rsg-wire's BackendMsg/TokenizerMsg unmodified — no new wire tag was introduced for readiness or observation (RESEARCH Anti-Pattern, plan prohibition)"
  - "ZmqSchedulerTransport is opened on the dedicated mock-engine thread itself, not created in main() and moved in — matches the project's own tx-zmq/rx-zmq pattern and avoids a socket-migration-adjacent first-send delay"
  - "ZMQ_RECONNECT_IVL lowered from libzmq's 100ms default to 1ms on every Connect-role socket (shared open_socket helper, so this also benefits the real rsg-server frontend, not just the mock) — turns a possible ~100ms first-message stall from a connect-before-bind race into a sub-millisecond one"
  - "Observe-file writes flush immediately after every line, which trivially satisfies the plan's 'flush after each drained frame and before any exit' requirement as a strict superset"

requirements-completed: [MOCK-01, WIRE-03]

coverage:
  - id: D1
    description: "cargo build -p rsg-server produces one package with lib rsg_server (modules handshake, transport) and two binaries rsg-server and mock-scheduler"
    requirement: "MOCK-01"
    verification:
      - kind: other
        ref: "cargo build -p rsg-server"
        status: pass
    human_judgment: false
  - id: D2
    description: "mock-scheduler, spawned with --backend-role bind --detok-role connect, writes exactly one handshake JSON line to stdout that rsg_server::handshake::parse_handshake accepts (handshake_version 1, vendored upstream SHA, configurable max_seq_len, eos_token_id 151645) — readiness travels out of band, never as a wire message"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#tracer_one_request_echo_round_trip (wait_ready assertions)"
        status: pass
      - kind: unit
        ref: "src/handshake.rs#tests::to_json_line_round_trips_through_parse_handshake"
        status: pass
    human_judgment: false
  - id: D3
    description: "A UserMsg{uid 7, input_ids [11,22,33], max_tokens 5} sent over a raw frontend ZmqTransport comes back as exactly five bare DetokenizeMsg frames for uid 7 with next_token 11,22,33,11,22, finished=true only on the fifth (echo rule)"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#tracer_one_request_echo_round_trip"
        status: pass
    human_judgment: false
  - id: D4
    description: "With --prefill-delay-ms P --decode-delay-ms D, a request's first token leaves no earlier than P ms after its UserMsg arrived, and consecutive tokens are at least D ms apart (D-10)"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#prefill_and_decode_delays_are_honored"
        status: pass
    human_judgment: false
  - id: D5
    description: "An AbortBackendMsg for an in-flight uid stops that uid's tokens (at most one already-emitted token follows, never finished); an abort for a uid the mock is not running changes nothing (mirrors scheduler.py:190-195)"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#abort_stops_in_flight_tokens"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#abort_for_unknown_uid_is_noop"
        status: pass
    human_judgment: false
  - id: D6
    description: "BatchBackendMsg items are processed in order; ExitMsg makes the mock exit 0 after flushing; --observe-file lists every processed backend message in processing order as submit/abort/exit lines"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#exit_msg_exits_0_and_records_order"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#items_after_exit_in_a_batch_are_not_processed"
        status: pass
    human_judgment: false
  - id: D7
    description: "mock-scheduler exits 0 on ExitMsg/SIGINT/SIGTERM; 1 on socket/observe-file open failure; 2 on a clap usage error; 3 on stdin EOF; 4 on an undecodable backend frame"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_process.rs#{stdin_eof_exits_3,sigterm_exits_0,sigint_exits_0,undecodable_frame_exits_4,exit_msg_exits_0_and_records_order}"
        status: pass
    human_judgment: false
  - id: D8
    description: "rsg-server's Phase 1 CLI, exit codes and log lines are unchanged: crates/rsg-server/tests/cli.rs passes without edits"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "cargo test -p rsg-server --test cli"
        status: pass
      - kind: other
        ref: "git diff --quiet -- crates/rsg-server/tests/cli.rs"
        status: pass
    human_judgment: false
  - id: D9
    description: "The transport split (BackendSink/DetokSource/split/ZmqSchedulerTransport) round-trips correctly when each half is moved to its own OS thread — the seam Plan 03-02's writer/dispatcher plug into"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/transport.rs#tests::scheduler_side_round_trips_with_split_frontend"
        status: pass
    human_judgment: false

duration: 35min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 01: Library split and mock-scheduler Summary

**Standalone `mock-scheduler` subprocess that speaks rsg-wire's existing BackendMsg/TokenizerMsg over real ipc:// sockets, with echo tokens, D-10 fixed delays, and a full exit-code/observe-file/signal/stdin-EOF process contract — built on a new `rsg_server` library split that also gives the frontend transport a BackendSink/DetokSource seam for Plan 03-02's writer/dispatcher.**

## Performance
- **Duration:** ~35min
- **Tasks:** 2 completed
- **Files modified:** 10 (4 created, 6 modified)
- **Commits:** 3

## Accomplishments
- `rsg-server` now ships a library crate (`lib.rs`) shared by two binaries, with `transport.rs` split into `BackendSink`/`DetokSource` plus a scheduler-side `ZmqSchedulerTransport`, proven by moving split halves across real OS threads
- A real `mock-scheduler` subprocess binds/connects over `ipc://`, announces readiness via the Phase 1 handshake JSON line (no 9th wire tag), and echoes a submitted prompt's tokens back with configurable fixed prefill/decode delays
- The mock's full process contract — `--observe-file`, stdin-EOF orphan guard, SIGINT/SIGTERM, and exit codes 0/1/2/3/4 — is pinned by 9 integration tests plus the tracer, all built test-first (RED commit, then GREEN)
- Found and fixed a real ZMQ connect-before-bind race (libzmq's 100ms default `ZMQ_RECONNECT_IVL`) that was silently delaying the first message on any freshly-opened Connect-role socket by up to ~100ms — fixed in the shared `open_socket` helper, benefiting the real frontend transport too, not just the mock

## Task Commits
1. **Task 1: Tracer — UserMsg through a spawned mock-scheduler echoes over real ipc://** - `ab53176` (feat)
2. **Task 2: mock-scheduler process contract** - `1db0ac0` (test, RED) + `6777f79` (feat, GREEN)

_No REFACTOR commit: the GREEN implementation needed no follow-up cleanup._

## Files Created/Modified
- `crates/rsg-server/src/lib.rs` — library root: `pub mod handshake; pub mod transport;`
- `crates/rsg-server/src/transport.rs` — `BackendSink`/`DetokSource` traits, `ZmqTransport::split`, `ZmqSchedulerTransport` (`open`/`recv_backend`/`send_detok`/`shutdown`), and the `RECONNECT_IVL_MS` fix in `open_socket`
- `crates/rsg-server/src/handshake.rs` — `Handshake` now derives `Serialize`; added `to_json_line()`
- `crates/rsg-server/src/main.rs` — consumes `rsg_server::{handshake, transport}` instead of declaring its own modules; otherwise unchanged
- `crates/rsg-server/src/bin/mock-scheduler.rs` — the mock binary: CLI, echo engine, observe file, stdin guard, signals, exit codes
- `crates/rsg-server/tests/common/mod.rs` — `MockScheduler` subprocess harness, `Observed` enum, `user_msg`/`echo_tokens`/`recv_frame` helpers
- `crates/rsg-server/tests/mock_scheduler_process.rs` — the tracer plus 9 process-contract tests
- `Cargo.toml`, `crates/rsg-server/Cargo.toml` — added `proptest`, `rustc-hash`, `rsg-wire` (path) workspace deps for this and later Phase 3 plans

## Decisions Made
- See `key-decisions` in frontmatter. The most consequential for later plans: `ZmqSchedulerTransport` must be opened on the thread that will use it (not created elsewhere and moved in), and `RECONNECT_IVL_MS = 1` is now baked into the shared `open_socket` helper every future transport consumer gets for free.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `scheduler_side_round_trips_with_split_frontend` unit test was racy as originally sketched**
- **Found during:** Task 1, running the new transport unit test
- **Issue:** The plan's sketch sends `b"ping"` once from a just-spawned thread immediately after `split()`, then does a single `recv_backend(2000)`. A freshly connected PUSH socket's connection is established asynchronously by libzmq's io thread; a send issued immediately after `connect()` from a just-spawned OS thread can race that attach and be silently dropped. Confirmed with a minimal standalone repro (outside this test suite) that reproduces the same `ready=0` outcome.
- **Fix:** Retry sending `"ping"` in a loop (20ms between attempts) until the scheduler side observes it, bounded by a 2s deadline, instead of a single fire-and-forget send.
- **Files modified:** `crates/rsg-server/src/transport.rs`
- **Verification:** Ran the test 5 consecutive times with no failures (previously failed consistently).
- **Commit:** `ab53176`

**2. [Rule 1 - Bug] `prefill_and_decode_delays_are_honored` / `abort_stops_in_flight_tokens` were intermittently timing-wrong due to the same underlying race**
- **Found during:** Task 2, running the new behavior tests
- **Issue:** The mock's detok PUSH socket connects before the test harness's frontend PULL has bound (the test only creates `frontend()` after `wait_ready()` returns). The first connect attempt races the bind and fails; libzmq schedules a retry after `ZMQ_RECONNECT_IVL` (default 100ms), which happened to land close to the first token's due time, delaying the first message's delivery by up to ~46ms in measured runs — enough to make `prefill_and_decode_delays_are_honored`'s gap assertions fail, and to make `abort_stops_in_flight_tokens` observe timing it shouldn't.
- **Fix:** Two changes in `crates/rsg-server/src/transport.rs`: (a) moved `ZmqSchedulerTransport::open` and the engine loop onto the same dedicated `mock-engine` thread (the thread now creates the sockets it uses, instead of receiving them pre-opened from `main()`), matching the project's own tx-zmq/rx-zmq convention; (b) set `ZMQ_RECONNECT_IVL` to 1ms on every socket in the shared `open_socket` helper, so a connect-before-bind race retries almost immediately instead of waiting up to 100ms. Confirmed empirically with instrumented timestamps on both sides before and after the fix.
- **Files modified:** `crates/rsg-server/src/transport.rs`, `crates/rsg-server/src/bin/mock-scheduler.rs`
- **Verification:** Ran `prefill_and_decode_delays_are_honored` and `abort_stops_in_flight_tokens` 5 consecutive times each with no failures; full `cargo test -p rsg-server` and `cargo clippy -p rsg-server --all-targets -- -D warnings` clean.
- **Commit:** `6777f79`

**Total deviations:** 2 auto-fixed (both Rule 1 — genuine ZMQ timing bugs found while implementing the plan's own test designs, not scope creep).
**Impact:** Both fixes land in the shared `open_socket` helper and the mock's own thread structure, so every later Phase 3 plan (03-02's writer/dispatcher, 03-04's ordering proptest, 03-05, 03-06) and Phases 5-7's harnesses inherit the fix for free rather than rediscovering the same race.

## Issues Encountered
None beyond the two deviations above, which were root-caused and fixed within this plan's own scope.

## User Setup Required
None — no external service configuration required. The one documented precondition (crates.io reachability for `proptest`/`rustc-hash`) was met; both downloaded and compiled without incident.

## Next Phase Readiness
Wave 2 (plans 03-02 and 03-03) can now build on: the `rsg_server` library's `transport` module (`BackendSink`/`DetokSource`/`ZmqSchedulerTransport`), the proven `MockScheduler` test harness in `tests/common/mod.rs`, and a `mock-scheduler` binary whose CLI surface and process contract are now pinned by tests. No blockers identified for 03-02 (writer/dispatcher) or 03-03 (misbehavior CLI extensions to mock-scheduler).

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All created files verified present on disk; all three task commits (`ab53176`, `1db0ac0`, `6777f79`) verified present in `git log --oneline --all`.
