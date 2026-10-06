---
phase: 03-zmq-transport-mock-scheduler
plan: 02
subsystem: transport
tags: [tokio, mpsc, broadcast, zmq, dispatch, writer]

requires:
  - phase: 03-zmq-transport-mock-scheduler
    provides: "plan 03-01's rsg_server library split (BackendSink/DetokSource/ZmqTransport::split) and the MockScheduler subprocess test harness (tests/common/mod.rs)"
provides:
  - "rsg_server::writer: spawn_writer, WriterHandle::{submit,exit}, Submitted, WriterClosed, WRITER_QUEUE_CAPACITY=1024 — a single ordered tx-zmq writer thread that coalesces queued messages into bare-vs-BatchBackendMsg exactly like upstream (D-01)"
  - "rsg_server::dispatch: spawn_dispatcher, DispatchHandle::register, UidStream::recv, TokenReply, UidEvent, UID_CHANNEL_CAPACITY=16, DISPATCH_POLL_MS=50 — a rx-zmq dispatcher thread routing DetokenizeMsg replies to per-uid broadcast channels with drop-oldest backpressure (D-04/D-05/D-07)"
  - "crates/rsg-server/tests/transport_e2e.rs: tracer_submit_routes_tokens_back_by_uid and many_concurrent_requests_route_by_uid, proving the writer/dispatcher pair end to end through the real mock-scheduler subprocess"
affects: [03-04, 03-05, 03-06, phase-05, phase-06, phase-07]

actuals:
  tokens: 6923
  tasks: 2
  commits: 2

commits: 2
plan_head_before: cada2a715f97ac851e598cc2c266a984e796ea5e
plan_head_after: 582fc3eb0ffb7a1ba93907164086655e072d5191

tech-stack:
  added: []
  patterns:
    - "tx-zmq writer thread: a dedicated std::thread::Builder thread named \"tx-zmq\" owns the sole BackendSink; WriterHandle is a cheap Clone over a bounded tokio::sync::mpsc::Sender<BackendMsg>, so every caller enqueues into one FIFO and the enqueue (not the later wire send) is the ordering point"
    - "rx-zmq dispatcher thread: a dedicated thread named \"rx-zmq\" owns the sole DetokSource and an FxHashMap<i64, broadcast::Sender<TokenReply>>; registrations arrive on an unbounded control channel drained before every frame is routed, so the hot path never locks"
    - "per-uid drop-oldest reply channel via tokio::sync::broadcast::channel(16) instead of a hand-rolled ring buffer — RecvError::Lagged(n) becomes the consumer-visible UidEvent::Dropped(n); the dispatcher's own send() never observes drops by design"

key-files:
  created:
    - crates/rsg-server/src/writer.rs
    - crates/rsg-server/src/dispatch.rs
    - crates/rsg-server/tests/transport_e2e.rs
  modified:
    - crates/rsg-server/src/lib.rs

key-decisions:
  - "Writer coalescing mirrors upstream exactly: blocking_recv the first queued message, then try_recv-drain everything else already queued; a lone message sends bare, two or more send as one BatchBackendMsg in enqueue order (RESEARCH Pitfall 3) — proven deterministic in queued_messages_coalesce_into_one_batch_in_order via a FakeSink that gates its first send and signals when it has entered that send"
  - "Writer failure propagates by dropping the mpsc receiver on thread exit: once sink.send_backend errs, the tx-zmq thread returns Err and drops its receiver, so every later submit/exit sees WriterClosed instead of hanging or silently dropping a message"
  - "Dispatcher routes recursively through BatchTokenizerMsg in order, exactly mirroring upstream's _unwrap_msg; an unknown uid is a silent no-op (D-04) and a send error (no receivers) removes the route, same as a finished reply does after a successful send"
  - "Task 1's writer.rs already implemented the full D-01 coalescing/error contract per the plan's own action text, so Task 2's TDD tests for writer.rs passed immediately with no further production change — see TDD Gate Compliance below"

patterns-established:
  - "Dedicated-OS-thread + bounded tokio mpsc bridge (tx-zmq/rx-zmq) is now proven twice (writer and dispatcher), matching the project's documented stack pattern and ready for Phase 5's FSM to consume directly"

requirements-completed: [WIRE-03]

coverage:
  - id: D1
    description: "Every message bound for the scheduler goes through one tx-zmq writer thread exclusively owning the single BackendSink; WriterHandle clones only enqueue into one FIFO mpsc queue (D-01)"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "grep -rl send_backend crates/rsg-server/src (exactly transport.rs and writer.rs)"
        status: pass
      - kind: integration
        ref: "tests/transport_e2e.rs#tracer_submit_routes_tokens_back_by_uid"
        status: pass
    human_judgment: false
  - id: D2
    description: "A request registered with DispatchHandle::register(uid) and then submitted with WriterHandle::submit receives exactly its own DetokenizeMsg tokens, in order, through its UidStream; the stream ends (recv returns None) after the finished token"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/transport_e2e.rs#tracer_submit_routes_tokens_back_by_uid"
        status: pass
    human_judgment: false
  - id: D3
    description: "32 requests submitted concurrently from 32 tokio tasks each receive exactly their own 8 echo tokens in order, with no Dropped event; the mock observes each uid's submit exactly once, followed by Exit"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/transport_e2e.rs#many_concurrent_requests_route_by_uid"
        status: pass
    human_judgment: false
  - id: D4
    description: "Messages queued while the writer is busy leave as one BatchBackendMsg in enqueue order; a lone message leaves bare, never a one-element batch"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/writer.rs#tests::single_message_is_sent_bare"
        status: pass
      - kind: unit
        ref: "src/writer.rs#tests::queued_messages_coalesce_into_one_batch_in_order"
        status: pass
    human_judgment: false
  - id: D5
    description: "If the writer's sink fails, the tx-zmq thread ends with Err and every later submit or exit returns Err(WriterClosed) instead of blocking or dropping silently; dropping every WriterHandle stops the thread cleanly with Ok(())"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/writer.rs#tests::sink_error_closes_writer"
        status: pass
      - kind: unit
        ref: "src/writer.rs#tests::dropping_every_handle_stops_writer_cleanly"
        status: pass
    human_judgment: false
  - id: D6
    description: "Each uid gets a broadcast channel of fixed capacity UID_CHANNEL_CAPACITY=16; the dispatcher never waits on a consumer, and a full channel overwrites its oldest token while the consumer sees UidEvent::Dropped(n) on its next recv"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "src/dispatch.rs — UidStream::recv maps RecvError::Lagged(n) to UidEvent::Dropped(n); UID_CHANNEL_CAPACITY = 16 constant"
        status: pass
    human_judgment: true
    rationale: "The drop-oldest mechanism is implemented and code-reviewable, but this plan's own tests never exercise an actual overflow (every uid's max_tokens=5 or 8 stays well under the 16-slot capacity, by plan design). CONTEXT.md and this plan's own objective explicitly defer the backpressure/drop-counter test to Plan 03-05 ('Plan 03-05 adds drop counters, deregister, stats and the backpressure tests') — flagging for human sign-off that this is an intentional scope boundary, not an oversight."
  - id: D7
    description: "A DetokenizeMsg for a uid that has no route is dropped without error, panic or effect on other uids (D-04)"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "src/dispatch.rs#route_one — unknown uid is a no-op"
        status: pass
    human_judgment: true
    rationale: "Implemented and code-reviewable (route_one returns immediately when routes.get(&uid) misses), but this plan has no dedicated test sending a reply for a never-registered uid at the dispatcher layer — 03-01's abort_for_unknown_uid_is_noop covers the analogous case one layer down (mock-scheduler itself), not this dispatcher. Flagging for human sign-off; a dedicated test is reasonable to add in 03-05 alongside the other dispatch-table edge cases."
  - id: D8
    description: "No second write path to the scheduler's backend socket exists in production code under crates/rsg-server/src — send_backend is called only from the tx-zmq writer thread (and defined in transport.rs)"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "grep -rl send_backend crates/rsg-server/src | sort"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 02: Single ordered writer and per-uid dispatcher Summary

**A dedicated `tx-zmq` writer thread serializes every outgoing message to the scheduler through one FIFO queue with upstream-matching bare-vs-batch coalescing, paired with a `rx-zmq` dispatcher thread that routes replies to per-uid `tokio::sync::broadcast` channels with drop-oldest backpressure — proven end to end through the real mock-scheduler subprocess with both a single-uid tracer and a 32-way concurrent stress test.**

## Performance
- **Duration:** ~25min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 2 completed
- **Files modified:** 4 (3 created, 1 modified)

## Accomplishments
- `rsg_server::writer` ships the single ordered writer (D-01): one `tx-zmq` OS thread exclusively owning the backend `BackendSink`, fed by a bounded `tokio::sync::mpsc` queue, coalescing anything already queued into a `BatchBackendMsg` in enqueue order (bare when there's exactly one) — mirroring upstream's own `scheduler/io.py`/`tokenizer/server.py` coalescing byte-for-byte in shape
- `rsg_server::dispatch` ships the per-uid reply dispatcher (D-04/D-05/D-07): one `rx-zmq` OS thread owning the detokenizer `DetokSource`, routing every `DetokenizeMsg` (recursing through `BatchTokenizerMsg`) to the `broadcast::channel(16)` registered for its uid, with unknown uids dropped silently and a full channel overwriting its oldest entry
- `tracer_submit_routes_tokens_back_by_uid` proves a single submit through the real mock-scheduler subprocess returns exactly its own echo tokens by uid and ends cleanly after the finished token
- `many_concurrent_requests_route_by_uid` proves 32 tokio tasks submitting concurrently each get exactly their own 8 tokens with zero cross-talk and zero drops
- Writer unit tests pin the exact coalescing/failure contract: bare single message, deterministic batch-of-three-plus-exit via a gated `FakeSink`, bare `ExitMsg`, `WriterClosed` after a sink failure, and clean `Ok(())` shutdown when every handle drops

## Task Commits
1. **Task 1: Tracer — submit through the single writer and get tokens back through the dispatcher** - `6f62bfc` (feat)
2. **Task 2: Coalescing and writer-failure unit tests, plus 32 concurrent requests routed by uid** - `582fc3e` (test)

## Files Created/Modified
- `crates/rsg-server/src/writer.rs` — `WRITER_QUEUE_CAPACITY`, `spawn_writer`, `WriterHandle::{submit,exit}`, `Submitted`, `WriterClosed`; `tx-zmq` thread with bare-vs-batch coalescing; 5 unit tests
- `crates/rsg-server/src/dispatch.rs` — `UID_CHANNEL_CAPACITY`, `DISPATCH_POLL_MS`, `TokenReply`, `UidEvent`, `UidStream`, `DispatchHandle::register`, `spawn_dispatcher`; `rx-zmq` thread with control-channel draining and recursive `BatchTokenizerMsg` routing
- `crates/rsg-server/src/lib.rs` — now also exports `pub mod writer;` and `pub mod dispatch;`
- `crates/rsg-server/tests/transport_e2e.rs` — `tracer_submit_routes_tokens_back_by_uid`, `many_concurrent_requests_route_by_uid`

## Decisions Made
See `key-decisions` in frontmatter. Most consequential for later plans: the writer's ordering point is the *enqueue*, not the wire send — 03-04's abort ticket API builds directly on that guarantee.

## TDD Gate Compliance

Task 2 was marked `tdd="true"` with an explicit `<behavior>` block. Following the RED-GREEN-REFACTOR discipline:

- **RED:** The test code was written and compiled/run first. Four of the six behaviors (`single_message_is_sent_bare`, `exit_sends_bare_exit_msg`, `sink_error_closes_writer`, `dropping_every_handle_stops_writer_cleanly`, and `many_concurrent_requests_route_by_uid`) passed on the first run with **no production-code change**, because Task 1's `writer.rs`/`dispatch.rs` were already built to the exact coalescing/error/routing contract specified in Task 1's own action text — Task 1's implementation *is* the GREEN for these behaviors, written before Task 2's tests, by this plan's own design (the plan explicitly separates "build to spec" (Task 1) from "pin the spec with tests" (Task 2)). This is an **expected, not unexpected, GREEN**: the feature already existed by design, not by accident — the fail-fast rule's "investigate" branch was taken, and the finding was "built correctly in Task 1."
- **Genuine RED found and fixed:** `many_concurrent_requests_route_by_uid`'s first compile failed with a type mismatch (`Tensor::from_i32_slice` expects `&[i32]`, the test built `[i64; 3]` from `u * 10 + N` where `u: i64`) — a real, if trivial, RED. Fixed by casting each element to `i32` before building the array. Re-ran: compiled and passed.
- **One clippy-driven refactor (not a behavior change):** `cargo clippy --all-targets -- -D warnings` flagged `FakeSink::gated`'s 4-tuple return type as `clippy::type_complexity`. Factored it into a `type GatedSink = (...)` alias inside the test module. No test assertions changed; all five writer unit tests and both `transport_e2e` tests still pass identically.
- **No separate GREEN or REFACTOR commit:** because the only production-code module touched by Task 2 (`writer.rs`) needed zero implementation change, only a type-alias change inside its own `#[cfg(test)]` module, the whole task landed in a single `test(03-02)` commit. This is documented here per the gate-enforcement rule ("If RED or GREEN commits end up missing for Task 2, add a TDD Gate Compliance section").

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Type mismatch in `many_concurrent_requests_route_by_uid`'s per-uid input ids**
- **Found during:** Task 2, first compile of the new concurrency test
- **Issue:** `let ids = [u * 10 + 1, u * 10 + 2, u * 10 + 3]` inferred `[i64; 3]` (since `u: i64`), but `Tensor::from_i32_slice` and `echo_tokens` both require `&[i32]`.
- **Fix:** Explicitly typed and cast: `let ids: [i32; 3] = [(u * 10 + 1) as i32, (u * 10 + 2) as i32, (u * 10 + 3) as i32];`.
- **Files modified:** `crates/rsg-server/tests/transport_e2e.rs`
- **Verification:** `cargo test -p rsg-server --test transport_e2e` compiles and passes (2/2), including 5 repeated runs of the concurrency test alone to check for flakiness — all green.
- **Commit:** `582fc3e`

**2. [Rule 1 - Bug] `clippy::type_complexity` on `FakeSink::gated`'s return type**
- **Found during:** Task 2, `cargo clippy -p rsg-server --all-targets -- -D warnings`
- **Issue:** `fn gated() -> (Self, Arc<Mutex<Vec<Vec<u8>>>>, mpsc::Receiver<()>, mpsc::Sender<()>)` tripped clippy's complexity lint under `-D warnings`.
- **Fix:** Introduced `type GatedSink = (FakeSink, Arc<Mutex<Vec<Vec<u8>>>>, mpsc::Receiver<()>, mpsc::Sender<()>);` inside the test module and used it as the return type.
- **Files modified:** `crates/rsg-server/src/writer.rs`
- **Verification:** `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0; all writer unit tests still pass.
- **Commit:** `582fc3e`

**Total deviations:** 2 auto-fixed (both Rule 1 — a genuine test-code type bug and a clippy lint, neither touching the writer/dispatcher's actual behavior).
**Impact on plan:** None beyond the two fixes themselves; both are confined to test code and landed in Task 2's own commit.

## Issues Encountered
None beyond the two deviations above.

## User Setup Required
None — no external service configuration required.

## Next Phase Readiness
- `rsg_server::writer` and `rsg_server::dispatch` are ready for Plan 03-04 (adds `WriterHandle::abort` and the ordering proptest) and Plan 03-05 (adds drop counters, `deregister`, stats, and the backpressure/unknown-uid tests flagged under D6/D7 above as deferred, not missing).
- Ready for 03-04 and 03-05 (Wave 3), and for 03-03 (still in Wave 2, dispatched separately).
- No blockers identified.

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All created files verified present on disk (`writer.rs`, `dispatch.rs`, `transport_e2e.rs`, this SUMMARY); both task commits (`6f62bfc`, `582fc3e`) verified present in `git log --oneline --all`.
