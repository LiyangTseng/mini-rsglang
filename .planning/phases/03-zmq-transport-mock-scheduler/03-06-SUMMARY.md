---
phase: 03-zmq-transport-mock-scheduler
plan: 06
subsystem: integration
tags: [e2e, integration-test, mock-scheduler, zmq, phase-gate]

requires:
  - phase: 03-zmq-transport-mock-scheduler (plans 03-01 through 03-05)
    provides: "The misbehaving mock's CLI vocabulary (03-03), the ticket-gated writer's abort ordering (03-04), and the counting/backpressure dispatcher (03-05) — all composed here against the real rsg-server binary"
provides:
  - "crates/rsg-server/tests/transport_misbehavior_e2e.rs: tracer_late_tokens_after_abort_are_dropped_and_counted, batched_replies_are_routed_to_each_uid, silent_uids_do_not_stall_other_requests, and rsg_server_binary_accepts_mock_handshake"
  - "A test-local RsgServer harness (stderr line collector, wait-for-substring, kill-on-Drop) spawning the real rsg-server binary against a real mock-scheduler's handshake, without editing tests/cli.rs or tests/common/mod.rs"
  - "Proof that all four of Phase 3's success criteria hold end to end on the Mac, and a green scripts/check_all.sh --offline (Phase 1 + Phase 3 gate)"
affects: [phase-05, phase-06, phase-07]

actuals:
  tokens: 4800
  tasks: 2
  commits: 2

commits: 2
plan_head_before: a454cc6c6cd24d6e9c8985dde7a00ee3a9db5a8e
plan_head_after: 607f94f1d7f0d2ab71e3a237fe3b90c7c69a7ce7

tech-stack:
  added: []
  patterns:
    - "Cross-plan composition test: one test file imports only the public API surfaces 03-01 through 03-05 already exposed (WriterHandle, DispatchHandle, MockScheduler, Handshake) and adds zero production code — the plan's own objective was proving integration, not building anything new"
    - "A second, file-local process-spawn harness (RsgServer) mirrors tests/cli.rs's Server pattern (stderr line collector thread, wait_for_log, kill -TERM, Drop cleanup) rather than editing the shared tests/cli.rs or tests/common/mod.rs, keeping those two files' diff empty per the plan's own acceptance criteria"

key-files:
  created:
    - crates/rsg-server/tests/transport_misbehavior_e2e.rs
  modified: []

key-decisions:
  - "Task 1 and Task 2 landed as two separate commits even though they share one file, to keep the per-task atomic-commit contract: Task 1's commit contains only the tracer test; Task 2's commit adds the three remaining tests on top"
  - "rsg_server_binary_accepts_mock_handshake gives rsg-server the opposite role from the mock on each endpoint (backend: mock binds, rsg-server connects; detok: mock connects, rsg-server binds), exactly mirroring the real launcher's topology, and only exercises the handshake/signal contract — no actual UserMsg/DetokenizeMsg traffic crosses these sockets in this test"

requirements-completed: [WIRE-03, MOCK-01]

coverage:
  - id: D1
    description: "Through the full Rust transport, late tokens after an abort are dropped and counted (unknown_uid lands in 3..=4), uid 5's stream never finishes, nothing panics, and uid 6 keeps streaming its echo tokens in order with no Dropped event"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/transport_misbehavior_e2e.rs#tracer_late_tokens_after_abort_are_dropped_and_counted"
        status: pass
    human_judgment: false
  - id: D2
    description: "--batch-size 4 routes each of four requests exactly its own 10 echo tokens in order through the dispatcher's BatchTokenizerMsg unwrapping; final stats routed=40, unknown_uid=0, malformed_frames=0"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/transport_misbehavior_e2e.rs#batched_replies_are_routed_to_each_uid"
        status: pass
    human_judgment: false
  - id: D3
    description: "A drop-overlong-flagged uid and a 64-token prompt at --max-seq-len 64 both get no reply within 500ms while a normal request submitted alongside them completes; the transport never stalls on a silent uid"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/transport_misbehavior_e2e.rs#silent_uids_do_not_stall_other_requests"
        status: pass
    human_judgment: false
  - id: D4
    description: "The real rsg-server binary accepts mock-scheduler's own handshake line relayed verbatim to its stdin, logs handshake received with max_seq_len=4096, eos_token_id=151645 and the vendored upstream_sha, and exits 0 on SIGTERM"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/transport_misbehavior_e2e.rs#rsg_server_binary_accepts_mock_handshake"
        status: pass
    human_judgment: false
  - id: D5
    description: "scripts/check_all.sh --offline passes with every Phase 3 test included, and the vendored tree is unmodified"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "scripts/check_all.sh --offline (ends with 'check_all: OK')"
        status: pass
      - kind: other
        ref: "git status --porcelain -- vendor/mini-sglang (empty)"
        status: pass
    human_judgment: false

duration: 45min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 06: End-to-end integration and the phase gate Summary

**One cross-plan test file composes the misbehaving mock (03-03), the ticket-gated writer (03-04) and the counting dispatcher (03-05) against the real transport and the real `rsg-server` binary, adding zero production code, and closes Phase 3 with a green `scripts/check_all.sh --offline`.**

## Performance
- **Duration:** ~45min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 2 completed
- **Files modified:** 1 (created)

## Accomplishments
- `tracer_late_tokens_after_abort_are_dropped_and_counted`: a real misbehaving mock (`--behavior late-abort-token` on uid 5) sends late tokens after `deregister(5)`/`abort(&t5)`; `unknown_uid` lands in `3..=4`, uid 5's stream never finishes, and uid 6 keeps streaming its echo tokens in order throughout, with no `Dropped` event
- `batched_replies_are_routed_to_each_uid`: `--batch-size 4` routes each of four concurrently-registered uids exactly its own 10 echo tokens in order via the dispatcher's `BatchTokenizerMsg` unwrapping; final stats `routed=40, unknown_uid=0, malformed_frames=0`
- `silent_uids_do_not_stall_other_requests`: a `drop-overlong`-flagged uid and a 64-token prompt at `--max-seq-len 64` both get no reply within 500ms while a normal uid submitted alongside them completes within 5s — the transport never stalls on a silent uid
- `rsg_server_binary_accepts_mock_handshake`: the real `rsg-server` binary, given the mock's own handshake line relayed verbatim to stdin (as the launcher would relay the real backend's), logs `handshake received` with the expected `max_seq_len`, `eos_token_id` and `upstream_sha` fields, then exits 0 on `SIGTERM` — closing criterion 1 for the actual frontend binary, not only the library-level handshake parser
- Bootstrapped the Mac dev env (`.venv` was missing in this checkout) via `bash scripts/bootstrap_mac_env.sh`, which succeeded on the first run (uv venv, 40 packages synced from the hashed lock, `minisgl`/`rsglang` editable-installed, smoke import passed)
- Full phase gate green: `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0, and `scripts/check_all.sh --offline` ends with `check_all: OK` across all 5 steps (cargo test --workspace, pytest, fixture freshness, WIRE-02 decode, vendored-tree check)

## Task Commits
1. **Task 1: Tracer — late tokens after an abort travel the full transport and are dropped and counted, while another uid streams on** - `a43123f` (feat)
2. **Task 2: Batched replies routed per uid, silent uids that stall nobody, rsg-server accepting the mock's handshake, and the phase gate** - `607f94f` (feat)

## Files Created/Modified
- `crates/rsg-server/tests/transport_misbehavior_e2e.rs` — 4 tests (one per `<task>`'s acceptance criteria) plus a file-local `RsgServer` process-spawn harness (stderr collector, `wait_for_log`, `signal`, `wait_exit`, `Drop`-based kill), modeled on `tests/cli.rs`'s own `Server` but kept local to this file so `tests/cli.rs` and `tests/common/mod.rs` stay untouched (`git diff --quiet` on both confirmed clean)

## Decisions Made
See `key-decisions` in frontmatter. Most consequential: Task 1 and Task 2 share one file but landed as two separate commits (tracer first, then the three remaining tests), preserving the one-commit-per-task contract despite the single-file plan.

## Deviations from Plan

None — plan executed exactly as written. Every test passed on its first implementation attempt, with no production-code bugs found and no auto-fixes needed (Rules 1-3 never triggered). The only non-trivial step was a single clippy fix (`while let` instead of a `loop`/`match`/`break` in the tracer's drain loop), caught by the plan's own mandatory `cargo clippy -p rsg-server --all-targets -- -D warnings` gate before the Task 1 commit — not a deviation from the plan's instructions, just satisfying a gate the plan itself specifies.

## Issues Encountered
None. `.venv/bin/python` was missing as the plan anticipated; `bash scripts/bootstrap_mac_env.sh` created it successfully on the first attempt (network access to PyPI via `uv` was available, same as plan 03-01's crates.io access).

## Timing Data (required by this plan's action block)

Measured via `time cargo test -p rsg-server` (debug build, warm cache, this Mac):

- **Total wall time:** 10.9s (`cargo test -p rsg-server 2>&1  1.14s user 1.17s system 21% cpu 10.899 total`)
- **`tests/ordering_proptest.rs` (03-04's 64-case property test plus its tracer):** 6.75s internal test time — **~62% of the total wall time**. This single file dominates the suite's runtime; every other test file (including this plan's own `transport_misbehavior_e2e.rs` at 1.12-1.41s across repeated runs) is comparatively fast.
- This plan's own 4 tests ran in **1.12-1.41s** across 4 repeated runs with 0 flakiness.

## Phase 3 Success Criteria — Explicit Status

This is the phase-closing plan; each of Phase 3's four success criteria (ROADMAP.md) is checked explicitly below, not just "tests pass":

1. **"The transport and the real rsg-server binary accept the mock's readiness handshake, and replies route by uid."** — **HOLDS.** `rsg_server_binary_accepts_mock_handshake` proves the real binary (not just `handshake.rs`'s parser) accepts the mock's handshake line and exits cleanly on signal. uid-routing is proven by `batched_replies_are_routed_to_each_uid` (4 uids, 40 tokens, each routed to its own stream) and by 03-05's own `tracer_slow_consumer_does_not_stall_other_uids` (still part of the full suite).
2. **"Ordering is proven by 03-04's property test, inside this gate."** — **HOLDS.** `cargo test -p rsg-server` (run as part of `check_all.sh --offline` step 1) includes `tests/ordering_proptest.rs::abort_never_precedes_its_own_submit` (64 cases) and its companion tracer, both passing, inside the full gate run captured above.
3. **"Each mock misbehavior is exercised, both raw (03-03) and through the transport."** — **HOLDS.** Raw: 03-03's `tests/mock_scheduler_behaviors.rs` (12 tests, still in the suite) exercises `late-abort-token`, `drop-overlong`/length-clamp and `--batch-size` directly against the mock's own wire output. Through the transport: this plan's `tracer_late_tokens_after_abort_are_dropped_and_counted` (late-abort-token), `silent_uids_do_not_stall_other_requests` (drop-overlong plus the max-seq-len rule), and `batched_replies_are_routed_to_each_uid` (`--batch-size`) exercise the same three misbehaviors end to end through the real writer and dispatcher.
4. **"Unknown-uid late tokens are dropped and counted, and silent or slow uids stall no one."** — **HOLDS.** `tracer_late_tokens_after_abort_are_dropped_and_counted` proves `unknown_uid` lands in `3..=4` for late tokens after an abort, with no panic and uid 6 unaffected. `silent_uids_do_not_stall_other_requests` proves two permanently-silent uids (drop-overlong and overlong-by-length) never block a normal uid's completion.

**The full `check_all` gate is green:** `scripts/check_all.sh --offline` completed all 5 steps and printed `check_all: OK` (captured output: step 1 `cargo test --workspace` all green including this plan's 4 new tests; step 2 `pytest python/tests` 172 passed, 37 skipped; step 3 fixture freshness matched 34 cases; step 4 WIRE-02 decode 37 passed; step 5 vendored-tree check matched the pristine tree with 0 listed modifications). `git status --porcelain -- vendor/mini-sglang` printed nothing.

## User Setup Required
None — no external service configuration required. `bash scripts/bootstrap_mac_env.sh` needed network access to PyPI (via `uv`), which was available in this environment; no human action was needed.

## Next Phase Readiness

Phase 3 (ZMQ Transport & Mock Scheduler) is **complete**: all 6 plans have SUMMARYs, and all 4 of the phase's success criteria hold end to end on the Mac per the explicit section above. WIRE-03 and MOCK-01 are both now ready to mark complete in `REQUIREMENTS.md` (this plan is the last of the phase to declare either). Phase 5 (request-lifecycle FSM) can build directly on:
- `rsg_server::writer::{WriterHandle, Submitted, spawn_writer}` for ordered submit/abort
- `rsg_server::dispatch::{DispatchHandle, UidStream, UidEvent, DispatchStatsSnapshot, spawn_dispatcher}` for per-uid routing, backpressure, and observability
- `mock-scheduler`'s full misbehavior CLI vocabulary for its own test harnesses

No blockers identified. Ready for the orchestrator's phase-level verification and the code-review gate.

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All claims verified below.
