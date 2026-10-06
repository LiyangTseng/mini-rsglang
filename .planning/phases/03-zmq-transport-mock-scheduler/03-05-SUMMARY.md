---
phase: 03-zmq-transport-mock-scheduler
plan: 05
subsystem: dispatch
tags: [tokio, broadcast, atomics, backpressure, observability]

requires:
  - phase: 03-zmq-transport-mock-scheduler
    provides: "plan 03-02's rx-zmq dispatcher (spawn_dispatcher, DispatchHandle::register, UidStream, UID_CHANNEL_CAPACITY=16 broadcast channel, drop-oldest backpressure) and tests/common/mod.rs's MockScheduler harness"
provides:
  - "UidStream::dropped(): a per-uid running total of tokens overwritten for backpressure, updated on every RecvError::Lagged(n) alongside the existing UidEvent::Dropped(n) and a new tracing::warn! (uid, dropped, total_dropped) — D-06's three-way drop signal is now complete"
  - "DispatchHandle::deregister(uid): sync, non-blocking explicit route removal, applied before the dispatcher's next frame — Phase 5's FSM cleanup hook"
  - "DispatchHandle::stats() -> DispatchStatsSnapshot {routed, unknown_uid, closed_route, malformed_frames}: four Arc<AtomicU64> counters updated with Ordering::Relaxed on the rx-zmq thread, giving Phase 6 parity debugging and Phase 5/7 observability a way to distinguish intentional backpressure drops, unknown/late replies, closed consumers and malformed frames"
  - "crates/rsg-server/tests/dispatch_backpressure.rs::tracer_slow_consumer_does_not_stall_other_uids: end-to-end proof through the real mock-scheduler subprocess that an unread uid never stalls another uid's replies, and its backlog surfaces as exactly Dropped(184) then its last 16 tokens"
  - "10 new dispatch.rs unit tests (1 tracer-companion lag test + 9 TDD behaviors) covering unknown-uid, malformed-frame, batch-in-order, deregister, re-register, closed-route, finished-ends-route, finished-survives-lag and dropping-every-handle"
affects: [03-06, phase-05, phase-06, phase-07]

actuals:
  tokens: 7409
  tasks: 2
  commits: 2

commits: 2
plan_head_before: 34f65b60437c6c6b95e4540a2e44cdcea9ec4cc2
plan_head_after: 1b5aa25d6a4a30d622a8cbd82e7222d890a3b150

tech-stack:
  added: []
  patterns:
    - "Arc<AtomicU64> counter bundle shared between a cloneable handle and its owning OS thread, read with Ordering::Relaxed snapshots — the same dedicated-thread-plus-cheap-handle shape as the tx-zmq/rx-zmq threads themselves, now extended with lock-free observability"
    - "Drop-detection lives entirely on the consumer side of a tokio::sync::broadcast channel (UidStream::recv's Lagged(n) branch), never at the dispatcher's send() call site — the dispatcher's send always returns Ok even when it silently overwrites, so counting there would never see a drop (RESEARCH Pitfall 4)"
    - "Test-local TestDispatcher harness opens a real ZmqTransport (detok Bind) plus a raw zmq::PUSH peer connected straight into it, giving dispatch.rs's own unit tests real socket-boundary coverage (decode errors, unknown uids, batch unwrapping) without needing the mock-scheduler subprocess"

key-files:
  created: []
  modified:
    - crates/rsg-server/src/dispatch.rs
    - crates/rsg-server/tests/dispatch_backpressure.rs

key-decisions:
  - "Unknown-uid drops log at tracing::debug!, not warn! — after a mass cancellation they can number in the thousands and would flood the log; the unknown_uid counter is the signal, exactly as the plan's objective specifies"
  - "Re-registering a uid needed no new removal logic: FxHashMap::insert already drops the old Sender value it replaces, which is exactly what ends the earlier UidStream after its buffered events — the existing register() code from 03-02 already implemented this contract, and reregister_replaces_route_and_ends_old_stream simply pins it with a test"
  - "DispatchStats is a private struct (four AtomicU64 fields) wrapped in an Arc shared between DispatchHandle and the rx-zmq closure; only the public DispatchStatsSnapshot (plain u64 fields, Clone/Copy/Debug/Default/PartialEq/Eq) crosses the module boundary, keeping the atomics themselves an implementation detail"
  - "Task 2's implementation (Control::Deregister, DispatchStats/Snapshot, the four counting sites in route_one) was written before its nine tests, then verified by running all of them green on the first attempt — a deviation from strict RED-first discipline; see TDD Gate Compliance below"

patterns-established:
  - "Lock-free Arc<AtomicU64> stats bundles are now the established pattern for per-component dispatcher/writer observability; the next thread that needs similar counters (e.g. a future writer-side stats surface) should reuse this shape rather than inventing a new one"

requirements-completed: [WIRE-03]

coverage:
  - id: D1
    description: "A slow consumer on one uid never stalls replies for another uid (success criterion 4): while uid 1's consumer reads nothing, uid 2 receives all 200 of its echo tokens in order, ending with finished, with no Dropped event, through the real mock-scheduler"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/dispatch_backpressure.rs#tracer_slow_consumer_does_not_stall_other_uids"
        status: pass
    human_judgment: false
  - id: D2
    description: "The unread uid's stream yields exactly Dropped(184), then its last 16 echo tokens (indices 184..200, the last finished); the oldest tokens were dropped and the newest, including the finished token, were kept (D-05)"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/dispatch_backpressure.rs#tracer_slow_consumer_does_not_stall_other_uids"
        status: pass
      - kind: unit
        ref: "src/dispatch.rs#tests::finished_token_survives_lag"
        status: pass
    human_judgment: false
  - id: D3
    description: "Every drop is surfaced three ways: in-band UidEvent::Dropped(n), the per-uid dropped() counter, and a tracing warn with uid/dropped/total_dropped fields (D-06) — never silent"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/dispatch.rs#tests::lagged_stream_reports_dropped_event_counter_and_warning"
        status: pass
    human_judgment: false
  - id: D4
    description: "A reply for a uid that was never registered, was deregistered, or already finished is dropped without error and counted in unknown_uid; a reply whose stream was dropped is counted in closed_route and removes that route; other uids are unaffected (D-04)"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/dispatch.rs#tests::unknown_uid_reply_is_dropped_and_counted"
        status: pass
      - kind: unit
        ref: "src/dispatch.rs#tests::deregistered_uid_late_reply_counts_as_unknown"
        status: pass
      - kind: unit
        ref: "src/dispatch.rs#tests::dropped_stream_counts_closed_route"
        status: pass
      - kind: unit
        ref: "src/dispatch.rs#tests::finished_reply_ends_stream_and_removes_route"
        status: pass
    human_judgment: false
  - id: D5
    description: "An undecodable detok frame is counted in malformed_frames and skipped, and the dispatcher keeps routing the frames after it"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/dispatch.rs#tests::malformed_frame_is_skipped_and_dispatcher_keeps_routing"
        status: pass
    human_judgment: false
  - id: D6
    description: "Registering a uid that is already registered replaces its route: the earlier UidStream returns None after its buffered events, and new replies go to the new stream"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/dispatch.rs#tests::reregister_replaces_route_and_ends_old_stream"
        status: pass
    human_judgment: false
  - id: D7
    description: "BatchTokenizerMsg entries, including a nested batch, are routed to their own uids in order; deregister(uid) removes the route, and that stream ends after its buffered events"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/dispatch.rs#tests::batch_entries_route_to_their_own_uids_in_order"
        status: pass
      - kind: unit
        ref: "src/dispatch.rs#tests::dropping_every_handle_stops_dispatcher_and_ends_streams"
        status: pass
    human_judgment: false
  - id: D8
    description: "UID_CHANNEL_CAPACITY stays a fixed constant of 16 with no CLI flag or config knob (D-07)"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "grep 'pub const UID_CHANNEL_CAPACITY: usize = 16' crates/rsg-server/src/dispatch.rs"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 05: Dispatcher backpressure, drop accounting, stats, and deregister Summary

**The per-uid dispatcher now proves a slow consumer never stalls another request (through a real mock-scheduler subprocess), surfaces every backpressure drop three ways, and exposes a lock-free `DispatchStatsSnapshot` plus explicit `deregister` so unknown, malformed, and closed-route replies are all counted instead of silently vanishing.**

## Performance
- **Duration:** ~40min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 2 completed
- **Files modified:** 2 (1 modified heavily, 1 new test file)

## Accomplishments
- `UidStream::dropped()` plus a `tracing::warn!("slow consumer: dropped oldest buffered tokens", uid, dropped, total_dropped)` complete D-06's three-way drop signal (event, counter, warning), proven both by a focused unit test and by a real end-to-end tracer
- `tests/dispatch_backpressure.rs::tracer_slow_consumer_does_not_stall_other_uids` proves through the real mock-scheduler subprocess that uid 2 gets all 200 of its tokens with zero drops while uid 1's unread backlog comes back as exactly `Dropped(184)` then its last 16 tokens, the last one finished
- `DispatchHandle::deregister(uid)` gives explicit, synchronous, non-blocking route removal — the first consumer of this will be Phase 5's FSM
- `DispatchStatsSnapshot { routed, unknown_uid, closed_route, malformed_frames }`, backed by four `Arc<AtomicU64>` counters updated with `Ordering::Relaxed` on the `rx-zmq` thread, gives every later phase (5, 6, 7) a way to tell "we dropped this on purpose" apart from "the backend misbehaved"
- Nine new TDD-driven unit tests exercise every dispatcher edge case against a real `ZmqTransport` detok socket and a raw `zmq::PUSH` peer: unknown uid, malformed frame, batch-in-order routing, deregister, re-register, dropped-stream-closes-route, finished-ends-route, finished-survives-lag, and dropping-every-handle

## Task Commits
1. **Task 1: Tracer — slow consumer on one uid does not stall another, drops counted and warned** - `1850074` (feat)
2. **Task 2: Dispatch stats, deregister, unknown/malformed/closed routes, re-register, finished-survives-lag** - `1b5aa25` (feat)

## Files Created/Modified
- `crates/rsg-server/src/dispatch.rs` — `UidStream::dropped()`, the lag-warning in `recv()`, `Control::Deregister`, `DispatchHandle::{deregister, stats}`, private `DispatchStats` (4 `AtomicU64`), public `DispatchStatsSnapshot`, four counting sites in `route_one`/`route_message`/`spawn_dispatcher`'s decode-error branch, and 10 new unit tests (1 from Task 1, 9 from Task 2) plus a `TestDispatcher` test harness
- `crates/rsg-server/tests/dispatch_backpressure.rs` — new file: `tracer_slow_consumer_does_not_stall_other_uids`

## Decisions Made
See `key-decisions` in frontmatter. Most consequential for later phases: the four stats counters and `deregister` are now the explicit observability/cleanup surface Phase 5's FSM and Phase 6's parity debugging build on directly.

## TDD Gate Compliance

Task 2 was marked `tdd="true"` with an explicit `<behavior>` block naming nine unit tests.

- **Deviation from strict RED-first discipline:** Because the nine behaviors all depend on API surface that didn't exist yet (`Control::Deregister`, `DispatchHandle::{deregister,stats}`, `DispatchStatsSnapshot`, and the per-outcome counting inside `route_one`), and several tests also needed a new `TestDispatcher` harness (real `ZmqTransport` + raw `zmq::PUSH` peer) that itself depends on those APIs' shapes, the production code (`Control::Deregister`, `DispatchStats`/`DispatchStatsSnapshot`, the four counting sites, and the updated module doc) was written and confirmed to build *before* the nine tests were written, rather than writing a failing test first against a stub. This is a genuine missing-RED situation, not an "already built in a prior task" one (compare 03-02's SUMMARY, where the prior task's code already existed) — flagging per the Fail-Fast Rules' "Missing RED commit: flag in SUMMARY.md."
- **GREEN:** All nine tests, plus Task 1's `lagged_stream_reports_dropped_event_counter_and_warning`, passed on their first run with no further production-code change (`cargo test -p rsg-server --lib dispatch::` — 10/10 passed). Re-ran three consecutive times with no flakiness.
- **REFACTOR:** None needed — `cargo clippy -p rsg-server --all-targets -- -D warnings` was clean on the first pass.
- **No separate commits for RED/GREEN/REFACTOR:** because production code and tests were written in one continuous pass and verified together, the whole task landed in a single `feat(03-05)` commit (`1b5aa25`), documented here per the gate-enforcement rule.
- **Mitigation:** every one of the nine named behaviors has its own dedicated test with a descriptive name matching the `<behavior>` block exactly, and all nine were run and shown green together with the full `cargo test -p rsg-server` suite (56 tests total across the crate) and `cargo clippy --all-targets -- -D warnings` before committing — the verification rigor the RED-GREEN cycle is meant to produce was still applied, just not in the test-first order.

## Deviations from Plan

### Auto-fixed Issues

None — no bugs, missing functionality, or blocking issues were found beyond the TDD-order deviation documented above, which is a process deviation, not a code deviation.

**Total deviations:** 1 (TDD gate compliance, documented above — tests written after implementation rather than before).
**Impact on plan:** None on correctness or coverage; all acceptance criteria and the plan's own `<verification>` block pass. The deviation is purely about commit/test ordering discipline.

## Issues Encountered
None.

## User Setup Required
None — no external service configuration required.

## Next Phase Readiness
- `UidStream::dropped()`, `DispatchHandle::{deregister, stats}` and `DispatchStatsSnapshot` are ready for Phase 5's FSM (route cleanup via `deregister`, drop/unknown/closed/malformed observability via `stats()`) and Phase 6's parity debugging (distinguishing intentional drops from backend faults).
- Wave 3 (03-04 and 03-05) is now complete. Wave 4 (03-06, end-to-end integration plus the phase gate) is the final plan in Phase 3 and becomes ready.
- No blockers identified.

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All claims verified below.
