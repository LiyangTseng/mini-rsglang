---
phase: 03-zmq-transport-mock-scheduler
plan: 04
subsystem: transport
tags: [writer, abort, proptest, concurrency, tokio, zmq]

requires:
  - phase: 03-zmq-transport-mock-scheduler
    provides: "plan 03-02's single ordered writer and per-uid dispatcher (WriterHandle::submit, Submitted, spawn_dispatcher, the tests/common MockScheduler harness)"
provides:
  - "rsg_server::writer::WriterHandle::abort(&Submitted) -> Result<(), WriterClosed>, enqueuing AbortBackendMsg through the same FIFO as submit, so an abort can never be enqueued before the submit that produced its ticket"
  - "A compile_fail doctest on Submitted proving its uid field cannot be forged outside writer.rs"
  - "Deterministic cross-task abort tracer (tests/ordering_proptest.rs#tracer_abort_from_another_task_follows_submit) and the D-02/D-03 property test (abort_never_precedes_its_own_submit) proving the ordering guarantee under random concurrent interleavings of up to 48 uids, run end to end through a real mock-scheduler subprocess"
affects: [03-05, 03-06, phase-05, phase-06, phase-07]

actuals:
  tokens: 5885
  tasks: 3
  commits: 3

commits: 3
plan_head_before: 2565f510f6c5744d337f8c66a38f9e92b6bb8d29
plan_head_after: cbadfa69fcfd8289b4c3b80737c1d1a56924723d

tech-stack:
  added: []
  patterns:
    - "Ticket-gated abort: WriterHandle::abort(&Submitted) makes 'an abort can never be enqueued before its own submit' structural, not conventional. Submitted's uid field is private; only submit's completed enqueue can mint one. A compile_fail doctest proves the field is unreachable from outside writer.rs (D-01, WIRE-03)."
    - "Property-based ordering proof against a real subprocess (D-02/D-03): a per-uid plan strategy (yields-before-submit, AbortMode::{None,SameTask,OtherTask}, yields-before-abort, max_tokens) combined with a spawn-order permutation (Just(Vec<usize>).prop_shuffle()) drives up to 48 uids concurrently through the real writer, real ipc sockets and a real mock-scheduler subprocess. Each case builds its own 4-worker tokio Runtime and calls block_on (RESEARCH A4's manual-Runtime pattern, since no proptest+tokio glue crate targets tokio)."

key-files:
  created:
    - crates/rsg-server/tests/ordering_proptest.rs
  modified:
    - crates/rsg-server/src/writer.rs

key-decisions:
  - "Final proptest case count: 64 (the largest of 64/32/16 that keeps the full test under the 60s budget). The RESEARCH A4 spike measured cases:8 at ~1.08s internal test time; cases:64 measured ~7.0s internal test time across 4 repeated runs (6.93s, 6.96s, 7.01s, 7.05s) — well under budget, so no further reduction was needed. See 'Proptest Case Count Spike' below."
  - "AbortMode::OtherTask spawns the abort task into the JoinSet before the submitting task, bridging the Submitted ticket over a tokio::sync::oneshot — the same cross-task pattern the Task 1 tracer proves deterministically, now exercised at property-test scale with random yield counts on both sides."
  - "The property test's dispatcher is spawned with zero uid registrations: every reply is dropped, which only drains the detok socket so the mock's PUSH never blocks. The test's assertions are entirely against the mock's own --observe-file order, not token content, so no per-uid stream consumption is needed."

patterns-established:
  - "Ticket-gated abort (WriterHandle::abort(&Submitted)) is now the abort contract Phase 5's FSM will call on every cancellation; Phase 5 does not rebuild ordering, it consumes this ticket."

requirements-completed: [WIRE-03]

coverage:
  - id: D1
    description: "WriterHandle::abort(&self, ticket: &Submitted) enqueues AbortBackendMsg through the same mpsc Sender as submit; Submitted's uid field is private so only a completed submit can produce one (D-01, WIRE-03)"
    requirement: "WIRE-03"
    verification:
      - kind: other
        ref: "src/writer.rs — Submitted { uid: i64 } has no pub field; grep -n 'pub struct Submitted' -A2 crates/rsg-server/src/writer.rs shows no pub uid"
        status: pass
      - kind: unit
        ref: "src/writer.rs — doctest writer::Submitted (compile_fail): `rsg_server::writer::Submitted { uid: 7 }` fails to compile outside the crate"
        status: pass
    human_judgment: false
  - id: D2
    description: "An abort issued from a different tokio task than its submit, after receiving the ticket over a oneshot, reaches the real mock-scheduler after that submit"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/ordering_proptest.rs#tracer_abort_from_another_task_follows_submit"
        status: pass
    human_judgment: false
  - id: D3
    description: "Calling abort twice with the same ticket forwards two AbortBackendMsg frames to the scheduler, both after the uid's UserMsg, in call order; the writer never deduplicates, merges or drops a message"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/writer.rs#tests::abort_twice_is_forwarded_twice_in_order"
        status: pass
    human_judgment: false
  - id: D4
    description: "A submit and its abort queued while the writer is busy leave in one BatchBackendMsg, with the UserMsg before the AbortBackendMsg"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/writer.rs#tests::submit_and_abort_coalesce_in_enqueue_order"
        status: pass
    human_judgment: false
  - id: D5
    description: "Once the writer thread has stopped, abort returns Err(WriterClosed) instead of blocking"
    requirement: "WIRE-03"
    verification:
      - kind: unit
        ref: "src/writer.rs#tests::abort_after_writer_stopped_returns_writer_closed"
        status: pass
    human_judgment: false
  - id: D6
    description: "Property test: random concurrent submit/abort interleavings across 1-48 uids, random yield counts, random spawn order, and aborts issued from the submitting task or from a different task that received the ticket, run end to end through the real transport and a real mock-scheduler subprocess (D-02/D-03). In every scenario the mock's observed order shows, per uid, exactly one submit before its abort (if any), each issued abort exactly once, no abort for a None-mode uid, and Exit last"
    requirement: "WIRE-03"
    verification:
      - kind: integration
        ref: "tests/ordering_proptest.rs#abort_never_precedes_its_own_submit (proptest, 64 cases, real MockScheduler subprocess per case)"
        status: pass
    human_judgment: false
  - id: D7
    description: "Interrupted mid-send: each coalesced frame reaches the scheduler whole or not at all, because ZMQ delivers whole messages only, so the scheduler never processes part of a BatchBackendMsg (e.g. a submit without its following abort)"
    requirement: "WIRE-03"
    verification: []
    human_judgment: true
    rationale: "This is a libzmq delivery guarantee (whole-message framing over ipc://), not something any Rust-side test in this phase can force by injecting a partial send — the plan's own must_haves block marks this truth's verification kind as 'backstop'. It is documented on WriterHandle::abort's module doc and relied upon structurally; flagging for human sign-off that this is the intended, deliberate testing boundary rather than an oversight."

duration: ~50min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 04: Ticket-gated abort and the abort-ordering property proof Summary

**`WriterHandle::abort` now requires a `Submitted` ticket whose private `uid` field only a completed `submit` enqueue can mint, making "an abort can never overtake its own submit" structural rather than conventional — proven by a compile_fail doctest, three deterministic unit tests, a cross-task tracer, and a 64-case property test driving up to 48 uids through a real mock-scheduler subprocess.**

## Performance
- **Duration:** ~50min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 3 completed
- **Files modified:** 2 (1 created, 1 modified)

## Accomplishments
- `WriterHandle::abort(&self, ticket: &Submitted) -> Result<(), WriterClosed>` enqueues `AbortBackendMsg` through the exact same FIFO `mpsc` queue as `submit`; because `Submitted`'s `uid` field is private, the only way to obtain a ticket is through a `submit` call whose enqueue has already completed — the ordering guarantee is structural, not a convention callers must honor
- A `compile_fail` doctest on `Submitted` proves `rsg_server::writer::Submitted { uid: 7 }` cannot compile outside `writer.rs`
- Three unit tests pin the exact abort contract: coalescing order inside a `BatchBackendMsg` (submit before its abort, even when a different uid's abort is enqueued in between), double-abort forwarding two frames in call order, and `Err(WriterClosed)` instead of blocking once the writer thread has stopped
- `tracer_abort_from_another_task_follows_submit` proves the cross-task case deterministically: a second task holds the ticket over a `oneshot`, waits for the submitter's first token, then aborts — the real mock-scheduler observes `Submit`, then `Abort`, then `Exit`
- `abort_never_precedes_its_own_submit` (D-02/D-03) is a `proptest` property test generating a per-uid plan (random yield counts, an `AbortMode` of `None`/`SameTask`/`OtherTask`, random `max_tokens`) and a random spawn-order permutation across 1-48 uids, run end to end through the real writer, real `ipc://` sockets and a real `mock-scheduler` subprocess per case; it checks exactly-one-submit, exactly-one-abort-strictly-after-its-submit for every aborted uid, zero aborts for `None`-mode uids, and `Exit` last

## Task Commits
1. **Task 1: Tracer — abort an in-flight request from another task; the mock sees submit, then abort** - `b3b7b06` (feat)
2. **Task 2: Abort ordering, idempotency and closed-writer unit tests, plus the unforgeable-ticket doctest** - `abdd063` (test)
3. **Task 3: Property test (D-02/D-03) — random concurrent submit/abort interleavings never put an abort before its submit** - `cbadfa6` (feat)

## Files Created/Modified
- `crates/rsg-server/src/writer.rs` — `WriterHandle::abort`, expanded doc comments on `Submitted` and `abort`, a `compile_fail` doctest, and three new unit tests (`submit_and_abort_coalesce_in_enqueue_order`, `abort_twice_is_forwarded_twice_in_order`, `abort_after_writer_stopped_returns_writer_closed`)
- `crates/rsg-server/tests/ordering_proptest.rs` — new file: `tracer_abort_from_another_task_follows_submit`, the `AbortMode`/`UidPlan`/`Scenario` strategy types, `scenario_strategy`/`run_scenario`, and the `abort_never_precedes_its_own_submit` proptest

## Decisions Made
See `key-decisions` in frontmatter. Most consequential for later plans: the ticket-gated abort API is what Phase 5's FSM will call on every cancellation — see `patterns-established`.

## Proptest Case Count Spike (RESEARCH A4)

Measured on this Mac, debug build, via `time cargo test -p rsg-server --test ordering_proptest abort_never -- --nocapture`:

| `cases` | Internal test time | Notes |
|---|---|---|
| 8 (spike) | ~1.08s | First run, used to extrapolate feasibility |
| 64 (final, run 1) | 7.01s | |
| 64 (final, run 2) | 6.96s | |
| 64 (final, run 3) | 7.05s | |
| 64 (final, run 4) | 6.93s | |

**Final `cases` value: 64** — the largest of 64/32/16, and comfortably under the 60s budget (4 repeated runs at 64 cases all landed in a tight 6.93s-7.05s band, with no sign of approaching the limit). Shrinking and failure persistence (`proptest-regressions/`) were left at their defaults; no failure was ever found, so no regressions file exists to commit.

## TDD Gate Compliance

Task 2 was marked `tdd="true"` with an explicit `<behavior>` block (3 unit tests + 1 `compile_fail` doctest):

- **RED (checked, not assumed):** Before writing the doctest, `Submitted`'s `uid` field was confirmed private (`grep -n "struct Submitted" -A2 crates/rsg-server/src/writer.rs` shows `uid: i64` with no `pub`). The `compile_fail` doctest's correct "RED" state is precisely that it **must not compile** — verified by running `cargo test -p rsg-server --doc`, which reported `1 passed` for a `compile fail` doctest, confirming it genuinely fails to compile (that failure-to-compile is the pass condition for a `compile_fail` block).
- **Unexpected-but-correct GREEN on the three unit tests:** `submit_and_abort_coalesce_in_enqueue_order`, `abort_twice_is_forwarded_twice_in_order`, and `abort_after_writer_stopped_returns_writer_closed` all passed on first run with **no production-code change beyond Task 1's own `abort` implementation**. This mirrors 03-02's own documented precedent: Task 1's `abort` already enqueues through the identical FIFO `mpsc` queue and coalescing logic that `submit` uses, so the coalescing/failure contract these tests pin was already correct by construction, not by accident. The fail-fast rule's "investigate" branch was taken; the finding was "built correctly in Task 1," consistent with this plan's own task split (Task 1 builds `abort` to spec; Task 2 pins the spec with tests).
- **No separate GREEN or REFACTOR commit:** because zero production code needed to change for the three unit tests, and the doctest only required a doc-comment addition (no behavior change to `writer.rs`'s logic), the whole task landed in a single `test(03-04)` commit, documented here per the gate-enforcement rule.

## Deviations from Plan

None — plan executed exactly as written. The one ambiguity worth noting (not a deviation): RESEARCH.md's informal phrase "flat-mapped together with" initially led to trying `.flat_map()` (the `Iterator` method), which does not exist on a `proptest::strategy::VecStrategy`; the correct API is `Strategy::prop_flat_map`. This was caught immediately by the compiler (`cargo build -p rsg-server --tests`) before any test ran, and fixed before the first test execution — not a Rule 1/2/3 fix against working code, just resolving a naming ambiguity while first writing Task 3.

## Issues Encountered
None beyond the naming resolution above.

## User Setup Required
None — no external service configuration required.

## Next Phase Readiness
- `rsg_server::writer::WriterHandle::abort` and `Submitted` are ready for Phase 5's FSM to call directly on every cancellation.
- Wave 3 in progress — 03-05 still to come (dispatched separately); Wave 4 (03-06) unblocks once both land.
- WIRE-03 is also declared by 03-02 (already complete) and 03-05 (not yet run); it intentionally stays not-ready in REQUIREMENTS.md until 03-05 also has a SUMMARY.
- No blockers identified.

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All created/modified files verified present on disk (`crates/rsg-server/src/writer.rs`, `crates/rsg-server/tests/ordering_proptest.rs`, this SUMMARY); all three task commits (`b3b7b06`, `abdd063`, `cbadfa6`) verified present in `git log --oneline --all`.
