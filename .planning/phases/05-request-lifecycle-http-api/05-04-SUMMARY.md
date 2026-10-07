---
phase: 05-request-lifecycle-http-api
plan: 04
subsystem: api
tags: [tokio, cancellation, timeout, lifecycle-fsm, rust-frontend]

requires:
  - phase: 05-01
    provides: "Engine/ActiveRequest/RequestEvent driver, AbortGuard wired through but inert, EngineConfig::{abort_timing, backend_timeout}, DEFAULT_BACKEND_TIMEOUT_MS"
provides:
  - "rsg_server::engine driver cancellation (biased tokio::select!, is_cancelled() checks before register and right after submit)"
  - "rsg_server::engine abort-timing modes: AbortTiming::Immediate (abort right away) and AbortTiming::Deferred (wait for first token or backend_timeout)"
  - "rsg_server::engine per-request backend-inactivity timeout (pinned sleep_until, reset on every Token)"
  - "rsg_server::engine overlong-prompt rejection against EngineConfig::max_seq_len (400 before register/submit/abort)"
  - "tests/http_cancellation.rs, tests/abort_timing.rs, tests/http_errors.rs shared test patterns (wait_for_abort/wait_for_observed polling idiom) for later plans"
affects: [05-05, 05-06, 05-07, 05-08, 05-09, 06]

actuals:
  tokens: 10674
  tasks: 3
  commits: 5
  plan_head_before: 6a776516f9492a62df974daa3a5ab3488460def9
  plan_head_after: 687f018f0c0b18b6f879456d08dc3611caf8568e

tech-stack:
  added: []
  patterns:
    - "biased tokio::select! with the cancellation branch first, so an abort is observed before the next stream event even while tokens are actively arriving"
    - "A pinned, reset-in-place tokio::time::sleep_until (tokio::pin! + .as_mut().reset(...)) for the per-request inactivity deadline, rearmed on every Token rather than recreated"
    - "abort_now (deregister then abort via the Submitted ticket) is the single chokepoint every cancellation/timeout/slow-consumer/decode-error exit path goes through, so the dispatcher's unknown_uid/closed_route counters always reflect every post-abort reply"
    - "Polling a mock-scheduler subprocess's observe file for eventual consistency (wait_for_abort/wait_for_observed helpers) rather than asserting immediately after a registry snapshot goes idle: an abort reaching Cancelled in the registry only means it was enqueued onto the writer's channel, not that the subprocess has received and recorded it yet across the real ipc boundary"

key-files:
  created:
    - crates/rsg-server/tests/http_cancellation.rs
    - crates/rsg-server/tests/abort_timing.rs
    - crates/rsg-server/tests/http_errors.rs
  modified:
    - crates/rsg-server/src/engine.rs

key-decisions:
  - "cancel_after_submit(engine, uid, submitted, stream, first_token_seen) is the single decision point for what a post-submit cancellation does: Immediate mode or an already-seen first token aborts right away (abort_now); Deferred mode with no first token yet enters deferred_wait. Both of the driver's two post-submit cancellation checkpoints (right after submit, and the decode loop's select) call through this one function, so the Immediate/Deferred split lives in exactly one place"
  - "deferred_wait's four outcomes (finished-token/non-finished-token/Dropped/timeout) all end Cancelled, never Decoding -> Cancelled: the client never received a token during the wait, so the transition is always Submitted -> Cancelled, reported after deferred_wait returns rather than inside it"
  - "finish_silent(engine, uid, to) reports a terminal state to the registry without sending any RequestEvent: every cancellation path is reached only because something dropped the AbortGuard/events receiver, so nobody is listening on the events channel by construction — this makes that explicit instead of inventing an event nobody consumes"
  - "The overlong check binds `config = &engine.config` once at the top of drive_request and uses `config.max_seq_len`/`config.abort_timing`/`config.backend_timeout` throughout, rather than reaching through `engine.config.*` at each call site"
  - "No tower-http timeout layer: the backend-inactivity deadline lives entirely inside the driver as a pinned, resettable sleep_until, per CLAUDE.md's prohibition on tower_http::timeout for streaming routes"

patterns-established:
  - "abort_now(engine, uid, submitted) — deregister before abort, so every reply arriving after the abort is counted by the dispatcher instead of silently routed — is reused identically by cancellation, the backend timeout, the slow-consumer path and the decode-error path"

requirements-completed: [LIFE-02, LIFE-04, LIFE-05]

coverage:
  - id: D1
    description: "A streaming client disconnect sends an abort to the backend promptly (within ~1s against a 20ms decode delay), late tokens the backend sends after that abort are dropped and counted via dispatch_stats (unknown_uid + closed_route), and the server keeps serving other requests afterward"
    requirement: "LIFE-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_cancellation.rs#tracer_stream_disconnect_sends_abort_and_counts_late_tokens"
        status: pass
    human_judgment: false
  - id: D2
    description: "The D-03 disconnect-detection bound for a request still queued (no token emitted yet, prefill in progress) when the client disconnects is measured, not eliminated — bounded by hyper's next-write detection plus slack"
    requirement: "LIFE-02"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_cancellation.rs#queued_stream_disconnect_abort_bound"
        status: pass
    human_judgment: false
  - id: D3
    description: "Abort timing is a server-wide, per-process setting (CONTEXT D-01): Immediate aborts during prefill well before the first token; Deferred waits for the first token (aborting immediately if it has already arrived, or at the backend timeout if the backend stays silent), and sends no abort when the first token is also the final one"
    requirement: "LIFE-05"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#tracer_immediate_aborts_during_prefill"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#deferred_waits_for_first_token"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#deferred_after_first_token_aborts_immediately"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#deferred_single_token_request_sends_no_abort"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#deferred_backend_silence_still_aborts_at_timeout"
        status: pass
    human_judgment: false
  - id: D4
    description: "Cancelling a request at any point in its lifecycle (before submit, during prefill, mid-decode) leaves no orphaned backend request: every submitted uid either finishes or gets exactly one abort, no uid is submitted twice, and every observed abort has an earlier submit"
    requirement: "LIFE-05"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/abort_timing.rs#cancel_at_any_point_leaves_no_orphan"
        status: pass
    human_judgment: false
  - id: D5
    description: "A prompt whose token count is >= the handshake's max_seq_len gets an immediate 400 invalid_request_error with no Submit ever reaching the backend; a prompt of exactly max_seq_len - 1 tokens is accepted and served"
    requirement: "LIFE-04"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_errors.rs#overlong_prompt_gets_immediate_400_and_boundary_is_accepted"
        status: pass
    human_judgment: false
  - id: D6
    description: "A request whose backend stops responding (no reply within backend_timeout after submit, or after its last token, including mid-stream via SIGSTOP) is aborted on the backend and failed: the streaming response ends without data: [DONE] and without a clean terminator, and the registry records Failed exactly once"
    requirement: "LIFE-04"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_errors.rs#backend_timeout_fails_streaming_request_without_done"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/http_errors.rs#backend_stall_mid_stream_times_out"
        status: pass
    human_judgment: false
  - id: D7
    description: "A backend timeout on one request does not affect any other in-flight request on the same server: one request times out and fails while a concurrent one completes normally"
    requirement: "LIFE-04"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/http_errors.rs#timeout_is_isolated_per_request"
        status: pass
    human_judgment: false

duration: 70min
completed: 2026-10-06
status: complete
---

# Phase 5 Plan 4: Cancellation, Abort Timing, Overlong Prompts and Backend Timeout Summary

**A client disconnect turns into a prompt abort on the backend, server-wide abort timing is configurable (immediate during prefill, or deferred to the first token), overlong prompts get an instant 400, and a silent or stalled backend fails the request instead of hanging it forever.**

## Performance

- **Duration:** ~70 min
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 4 (3 created, 1 modified)

## Accomplishments
- A client walking away from a streaming `/generate` request aborts its request on the backend within one decode step; the backend's late tokens (sent after the abort, per the mock's `late-abort-token` behavior) are dropped and counted by the dispatcher, and the server keeps serving other requests — proven end to end against a real `mock-scheduler` subprocess.
- The `AbortGuard`'s D-03 disconnect-detection bound (hyper only notices a dead connection on its next write) is measured, not eliminated: `queued_stream_disconnect_abort_bound` shows a request still in prefill gets its abort by the time the first (never-delivered) chunk would have been written.
- Abort timing is now a real server-wide behavior, not just a config field: `Immediate` aborts during prefill well before any token exists; `Deferred` waits for the first token (aborting immediately once it arrives, or at the backend timeout if the backend stays silent), and correctly sends no abort at all when that first token is also the last one.
- A 30-request concurrent cancellation stress test (`cancel_at_any_point_leaves_no_orphan`) proves no cancellation path — before submit, during prefill, or mid-decode — ever leaves a request orphaned on the backend: every submit either finishes or gets exactly one abort.
- Overlong prompts (`input_len >= max_seq_len`) now get an immediate 400 before anything reaches the backend, mirroring the scheduler's own silent-drop rule; a prompt one token under the limit is still accepted.
- A backend that stops responding — never answering a submit, or going silent mid-stream (including a real SIGSTOP) — is aborted and fails the request with no `[DONE]` and no clean terminator, and this failure is isolated to the one affected request.

## Task Commits

Each task was committed atomically (Tasks 2 and 3 followed the TDD RED/GREEN cycle per their `tdd="true"` attribute):

1. **Task 1: Tracer — streaming disconnect sends an abort and counts late tokens** - `61cfefd` (feat)
2. **Task 2: Abort timing and no-orphan cancellation (RED)** - `24b4370` (test)
3. **Task 2: Abort timing and no-orphan cancellation (GREEN)** - `b367aa1` (feat)
4. **Task 3: Overlong prompts and backend timeout (RED)** - `2a1c0b5` (test)
5. **Task 3: Overlong prompts and backend timeout (GREEN)** - `687f018` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/src/engine.rs` — biased `tokio::select!` cancellation (cancel branch first), `abort_now`, `cancel_after_submit`, `deferred_wait`, `finish_silent`, the overlong-prompt check, and the pinned/resettable backend-inactivity `sleep_until`; `AbortGuard`'s doc comment documents the D-03 contract
- `crates/rsg-server/tests/http_cancellation.rs` — `tracer_stream_disconnect_sends_abort_and_counts_late_tokens`, `queued_stream_disconnect_abort_bound`
- `crates/rsg-server/tests/abort_timing.rs` — `tracer_immediate_aborts_during_prefill`, `deferred_waits_for_first_token`, `deferred_after_first_token_aborts_immediately`, `deferred_single_token_request_sends_no_abort`, `deferred_backend_silence_still_aborts_at_timeout`, `cancel_at_any_point_leaves_no_orphan`
- `crates/rsg-server/tests/http_errors.rs` — `overlong_prompt_gets_immediate_400_and_boundary_is_accepted`, `backend_timeout_fails_streaming_request_without_done`, `backend_stall_mid_stream_times_out`, `timeout_is_isolated_per_request`

## Decisions Made
- `cancel_after_submit` is the one place the Immediate/Deferred split lives: both post-submit cancellation checkpoints (the one-shot check right after `submit` returns, and the decode loop's `select!` branch) call through it with the current `reported_decoding` flag, rather than duplicating the Immediate-vs-Deferred-vs-first-token-seen logic at each call site.
- `deferred_wait`'s four outcomes all end `Cancelled`, never `Decoding -> Cancelled` — the terminal report happens once, in the caller, after `deferred_wait` returns, so the state machine never risks reporting a transition the client's own view of the request never passed through.
- `finish_silent` exists instead of inventing a `RequestEvent::Cancelled` variant nobody would consume: every cancellation path is reached only because the `AbortGuard`/events receiver was already dropped, so sending an event there would be a silently-ignored no-op anyway.
- The task's planned two-arm `biased` select (Task 1) was deliberately kept to just `cancel` + `stream.recv()` until Task 3 added the third `sleep_until` arm, so each task's diff stayed scoped to what that task's tests actually required.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed a timing race between the registry reaching a terminal state and the mock-scheduler subprocess recording that abort in its observe file**
- **Found during:** Task 2 (RED run of `cancel_at_any_point_leaves_no_orphan`) and Task 3 (`backend_timeout_fails_streaming_request_without_done`, run as part of the full `cargo test -p rsg-server` suite after Task 3's GREEN)
- **Issue:** `abort_now`'s `writer.abort(&submitted).await` only waits for the abort message to be **enqueued** onto the single ordered writer's channel, not for the separate `mock-scheduler` subprocess to receive it over the real `ipc://` socket and write it to its `--observe-file`. Two tests asserted `mock.observed()` contains the `Abort` entry immediately after the registry snapshot went idle (or the HTTP response ended), which is a real but narrow race — it only showed up under certain scheduling/timing, not every run.
- **Fix:** Added a bounded polling helper (`wait_for_observed` in `abort_timing.rs`, `wait_for_abort` in `http_errors.rs`) that waits up to 2s for the expected observe-file state instead of asserting immediately, matching the polling idiom already used elsewhere in these same test files (and in plan 05-01's `snapshot_when_idle`).
- **Files modified:** `crates/rsg-server/tests/abort_timing.rs`, `crates/rsg-server/tests/http_errors.rs`
- **Verification:** Reran the full `cargo test -p rsg-server` suite (99 tests) clean; reran `--test abort_timing` and `--test http_errors` individually twice each with no flakes.
- **Committed in:** `24b4370` (Task 2 RED commit, for `abort_timing.rs`) and `687f018` (Task 3 GREEN commit, for `http_errors.rs` — the race in `backend_timeout_fails_streaming_request_without_done` only surfaced once run alongside the rest of the suite, after Task 3's implementation was already in place)

**2. [Rule 3 - Blocking] Added `#[allow(dead_code)] mod common;` to the new `http_cancellation.rs`, matching the existing per-file convention**
- **Found during:** Task 1
- **Issue:** `cargo clippy -p rsg-server --all-targets -- -D warnings` compiles each integration-test binary separately; this new test file doesn't call every helper in `tests/common/` (e.g. `OpenStream::header`, `user_msg`), so clippy flagged them as dead code in this binary even though other test binaries use them. Every existing test file already works around this the same way (`#[allow(dead_code)] mod common;`), confirmed by reading `http_generate.rs`/`http_chat.rs`.
- **Fix:** Added the same `#[allow(dead_code)]` attribute above `mod common;` in the new file.
- **Files modified:** `crates/rsg-server/tests/http_cancellation.rs`
- **Verification:** `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0.
- **Committed in:** `61cfefd` (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking clippy-gate fix)
**Impact on plan:** Both are narrow, test-only fixes required to satisfy the plan's own verification commands. No production-code behavior changed by either fix; no scope creep.

## Issues Encountered
- **The overlong-prompt RED test hung the test binary for 120+ seconds** when run against the pre-Task-3 driver (no 400 check and no backend timeout yet): `http_client::send()` has no built-in timeout (unlike `OpenStream::next_chunk`, which the other three `http_errors.rs` tests use), so the request sat in `stream.recv().await` forever — exactly the bug LIFE-04 exists to fix. Had to manually find and `kill -9` the orphaned `cargo test`/`mock-scheduler` processes. Fixed by wrapping that one HTTP call in a bounded `tokio::time::timeout(500ms, ...)` so the RED run fails fast with a clear panic message instead of hanging; this wrapper stays in the final GREEN test too (it's a no-op once the real 400 arrives well under 500ms) and doubles as a stricter assertion of the "immediate" requirement.
- Both RED verification runs (`abort_timing.rs`, `http_errors.rs`) produced some already-GREEN tests at RED time (3 of 6 in `abort_timing.rs`, both of `http_cancellation.rs`'s tests) because they only exercise Immediate-mode or already-built Task 1 cancellation paths. Investigated per the TDD "unexpected GREEN" rule and confirmed as expected overlap, not a wrong or pre-existing test — documented in both RED commit messages, matching the pattern already established in plan 05-03's SUMMARY.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- The engine's cancellation, abort-timing, overlong-prompt, and backend-timeout behavior is now complete and stable; plans 05-05 through 05-09 (observability, the 128-agent stress test, Phase 4's tokenizer wiring, non-streaming disconnect handling, and the API fixture-diff) can build on `Engine`/`ActiveRequest`/`EngineConfig` without needing to reopen `engine.rs`'s cancellation/timeout internals.
- `wait_for_abort`/`wait_for_observed`-style polling (rather than asserting immediately after a registry snapshot goes idle) should be the default pattern for any future test that checks `mock.observed()` right after a cancellation or timeout — the enqueue-vs-subprocess-write race this plan found is general, not specific to this plan's own tests.
- Plan 05-07's non-streaming disconnect test (`http_nonstream::tracer_nonstream_disconnect_reaches_one_terminal_state`) is explicitly named in this plan's `AbortGuard` doc comment as the companion measurement for the D-03 bound on non-streaming requests; this plan only measures the streaming-and-queued case.
- No blockers.

## Self-Check: PASSED

All created/modified files verified present on disk; all 5 task commits
(`61cfefd`, `24b4370`, `b367aa1`, `2a1c0b5`, `687f018`) verified present in
`git log`.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-06*
