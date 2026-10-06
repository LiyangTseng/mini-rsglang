---
phase: 03-zmq-transport-mock-scheduler
plan: 03
subsystem: mock-scheduler
tags: [zmq, mock-scheduler, clap, msgpack, tdd]

requires:
  - phase: 03-zmq-transport-mock-scheduler (plan 03-01)
    provides: "mock-scheduler's BTreeMap<i64, Running> step-model engine, the ZmqSchedulerTransport, the observe-file/exit-code process contract, and the tests/common MockScheduler subprocess harness"
provides:
  - "mock-scheduler --misbehave-uids/--behavior CLI (repeatable, paired by position) selecting late-abort-token or drop-overlong per uid, plus uid-range parsing stored as intervals (never expanded)"
  - "late-abort-token: a flagged uid's abort switches it to a draining engine state that keeps emitting its echo sequence (always finished=false) for LATE_TOKENS_AFTER_ABORT=3 more tokens before removal"
  - "drop-overlong plus the upstream overlong rule (scheduler.py:177-188): a flagged uid, or any uid whose input_len >= max_seq_len, is silently dropped (recorded as submit, never answered); otherwise max_tokens is clamped to max_seq_len - input_len"
  - "--batch-size N (default 1, >=1) reply batching with a BATCH_FLUSH_MS=10 partial-batch flush timer, folded into the engine's own poll-timeout clock (io.py:124-130 semantics: one bare DetokenizeMsg, or BatchTokenizerMsg for 2+)"
  - "CLI validation exits 2 before any socket opens or handshake line is written: unpaired --misbehave-uids/--behavior counts, a uid in two lists (named in the error), malformed/reversed ranges, and --batch-size 0"
  - "tests/mock_scheduler_behaviors.rs: 12 tests covering all of the above plus several-behaviors-in-one-process and uid-range application"
affects: [03-04, 03-05, 03-06, phase-05, phase-06, phase-07]

actuals:
  tokens: 10300
  tasks: 2
  commits: 3

commits: 3
plan_head_before: 8d8118af1d3a408ec89293996c8923ade2fc1d7b
plan_head_after: 8cf54679e0706ebc947a0657e278d6df0439d237

tech-stack:
  added: []
  patterns:
    - "Behavior selection is a BehaviorTable over UidList interval ranges (never expanded into a set, T-03-11), looked up by behavior_of(uid) at both UserMsg-processing time (drop-overlong) and AbortBackendMsg-processing time (late-abort-token)"
    - "Running gained a RunningState enum (Normal | Draining{remaining}) instead of a second parallel map, so the existing BTreeMap<i64, Running> step model and ascending-uid emission order keep working unmodified for draining requests"
    - "Reply batching is a PendingReplies buffer (entries + oldest-added timestamp) flushed either when it reaches --batch-size or when the engine's own poll-timeout clock reaches the oldest entry's BATCH_FLUSH_MS deadline — the engine's recv_backend timeout is now min(earliest due token, pending flush deadline), so no separate timer thread was needed"
    - "process_msg's growing parameter list was bundled into an EngineConfig{prefill_delay, decode_delay, max_seq_len} value type to stay under clippy::too_many_arguments, rather than disabling the lint"
    - "clap ValueEnum's default kebab-case rendering (confirmed by cargo build, not assumed) gave late-abort-token/drop-overlong for free from LateAbortToken/DropOverlong variant names"

key-files:
  created:
    - crates/rsg-server/tests/mock_scheduler_behaviors.rs
  modified:
    - crates/rsg-server/src/bin/mock-scheduler.rs

key-decisions:
  - "CLI validation (equal-length pairing, no uid in two lists) runs via Cli::command().error(ArgumentConflict, ...).exit() immediately after Cli::parse(), before tracing_subscriber::init() or any socket opens — matches the plan's 'never looks ready' requirement and reuses clap's own exit-2 path rather than a hand-rolled one"
  - "Uid-range overlap detection is pairwise interval intersection (first_overlap), not set expansion, so a huge --misbehave-uids range costs O(ranges) to validate, not O(range size) (T-03-11)"
  - "drop-overlong and the upstream max_seq_len rule share one code path in process_msg's UserMsg arm (a single `dropped_overlong` boolean checked once), since D-09 explicitly frames drop-overlong as a way to trigger the same silent drop upstream's overlong rule produces"
  - "late-abort-token's draining due time uses config.decode_delay (not prefill_delay or 0), so late tokens keep the same cadence as normal decode tokens — the plan's 'due = now + decode' instruction taken literally rather than approximated"

requirements-completed: [MOCK-01]

coverage:
  - id: D1
    description: "A flagged uid (late-abort-token) keeps sending exactly three more echo tokens (finished=false) after its abort while an unflagged uid stops within one already-in-flight token; the behavior is selected per uid from the CLI at spawn"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#late_abort_token_sends_three_tokens_after_abort"
        status: pass
    human_judgment: false
  - id: D2
    description: "drop-overlong silently drops a flagged uid's request (recorded as submit, never answered, logged); independently, any UserMsg with input_len >= max_seq_len is dropped the same way, and a shorter prompt's max_tokens is clamped to max_seq_len - input_len"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#drop_overlong_flagged_uid_never_replies"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#prompt_at_max_seq_len_is_dropped_and_shorter_prompt_is_clamped"
        status: pass
    human_judgment: false
  - id: D3
    description: "--batch-size N sends up to N pending replies as one BatchTokenizerMsg (bare DetokenizeMsg when exactly one is pending); a partial batch flushes BATCH_FLUSH_MS=10ms after its oldest reply; the default of 1 keeps every reply bare"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#batch_size_four_sends_one_batch_tokenizer_msg"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#partial_batch_flushes_on_timer"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#default_batch_size_sends_every_reply_bare"
        status: pass
    human_judgment: false
  - id: D4
    description: "One mock process applies several behaviors at once to different uids, and a uid range A-B applies a behavior to every uid from A to B inclusive"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#several_behaviors_in_one_process"
        status: pass
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#uid_range_applies_behavior_to_each_uid"
        status: pass
    human_judgment: false
  - id: D5
    description: "Invalid behavior configuration (unpaired flags, a uid in two lists, a malformed/reversed range, or --batch-size 0) exits 2 before any handshake line is written"
    requirement: "MOCK-01"
    verification:
      - kind: integration
        ref: "tests/mock_scheduler_behaviors.rs#{unpaired_flags_exit_2,overlapping_uid_lists_exit_2,malformed_uid_list_exits_2,zero_batch_size_exits_2}"
        status: pass
    human_judgment: false
  - id: D6
    description: "mock-scheduler --help lists exactly the eleven documented long options besides --help, with no option for random/seeded timing, per-uid delays, or scenario files (D-11 deferred)"
    requirement: "MOCK-01"
    verification:
      - kind: other
        ref: "target/debug/mock-scheduler --help (manually diffed against the plan's acceptance-criteria list)"
        status: pass
    human_judgment: false
  - id: D7
    description: "cargo test -p rsg-server (all 53 tests across lib/cli/mock_scheduler_behaviors/mock_scheduler_process/transport_e2e) and cargo clippy -p rsg-server --all-targets -- -D warnings both exit 0 — no regression to Plan 03-01's mock-scheduler contract or Plan 03-02's writer/dispatcher"
    requirement: "MOCK-01"
    verification:
      - kind: other
        ref: "cargo test -p rsg-server"
        status: pass
      - kind: other
        ref: "cargo clippy -p rsg-server --all-targets -- -D warnings"
        status: pass
    human_judgment: false

duration: 55min
completed: 2026-10-06
status: complete
---

# Phase 3 Plan 03: Mock-Scheduler Backend Misbehaviors Summary

**Extended `mock-scheduler` with the three MOCK-01 backend misbehaviors Phase 5/6/7 need to provoke on purpose — late tokens after an abort, silently dropped overlong prompts (plus the upstream length-clamp rule), and batched replies with a flush timer — each independently selectable per uid or globally at spawn, validated strictly, and pinned by 12 new integration tests.**

## Performance
- **Duration:** ~55min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 2 completed
- **Files modified:** 2 (1 created, 1 modified)

## Accomplishments
- `--misbehave-uids <LIST> --behavior <late-abort-token|drop-overlong>` (repeatable, paired by position) plus `--batch-size <N>` give one mock process a full D-09 misbehavior vocabulary, with uid ranges stored and matched as intervals (never expanded, T-03-11) so a huge range is free to validate
- `late-abort-token` adds a `RunningState::Draining` variant to the existing `Running`/`BTreeMap` step model rather than a parallel data structure, so a flagged uid keeps echoing exactly `LATE_TOKENS_AFTER_ABORT=3` more tokens (always `finished=false`) after its abort while everything else about the engine's emission order is untouched
- `drop-overlong` and the upstream overlong rule (scheduler.py:177-188) share one `dropped_overlong` check in `process_msg`'s `UserMsg` arm; the surviving path clamps `max_tokens` to `max_seq_len - input_len`
- `--batch-size` reply batching (io.py:124-130) is a `PendingReplies` buffer folded directly into the engine's existing poll-timeout clock (`min(earliest due token, pending flush deadline)`) — no second timer thread, and the default of 1 keeps Plan 03-01's bare-reply behavior byte-for-byte unchanged
- All CLI validation (unequal `--misbehave-uids`/`--behavior` counts, a uid in two lists, malformed/reversed ranges, `--batch-size 0`) exits 2 via clap's own `ArgumentConflict` error path before any socket opens or handshake line is written
- 12 integration tests in a new `tests/mock_scheduler_behaviors.rs`, written test-first for Task 2 (RED commit confirmed failing for the right reason — missing `--batch-size` flag and missing drop/clamp logic — before the GREEN implementation commit)

## Task Commits
1. **Task 1: Tracer — late-abort-token, CLI flag to extra tokens on the wire after an abort** - `4edda0e` (feat)
2. **Task 2: drop-overlong, upstream clamp, batch-size, several behaviors, CLI validation** - `3908942` (test, RED) + `8cf5467` (feat, GREEN)

_No REFACTOR commit: the GREEN implementation needed no follow-up cleanup (clippy was clean after one argument-count fix folded into the GREEN commit itself)._

## Files Created/Modified
- `crates/rsg-server/src/bin/mock-scheduler.rs` — `UidList`/`Behavior`/`BehaviorTable`, CLI validation, `RunningState`, `PendingReplies`/`flush_pending`, the overlong drop/clamp rule, and the `EngineConfig` bundling refactor
- `crates/rsg-server/tests/mock_scheduler_behaviors.rs` — 12 tests: `late_abort_token_sends_three_tokens_after_abort`, `drop_overlong_flagged_uid_never_replies`, `prompt_at_max_seq_len_is_dropped_and_shorter_prompt_is_clamped`, `batch_size_four_sends_one_batch_tokenizer_msg`, `partial_batch_flushes_on_timer`, `default_batch_size_sends_every_reply_bare`, `uid_range_applies_behavior_to_each_uid`, `several_behaviors_in_one_process`, `unpaired_flags_exit_2`, `overlapping_uid_lists_exit_2`, `malformed_uid_list_exits_2`, `zero_batch_size_exits_2`

## Decisions Made
See `key-decisions` in frontmatter. Most consequential for later plans: `process_msg`'s `EngineConfig` bundling pattern (anything else that needs to grow its fixed-config footprint without tripping `clippy::too_many_arguments` should follow the same shape), and the `PendingReplies` flush-deadline-folded-into-poll-timeout pattern (no new thread for batch timing).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking issue] `process_msg` exceeded clippy's `too_many_arguments` threshold**
- **Found during:** Task 2, first `cargo clippy --all-targets -- -D warnings` run after adding `max_seq_len` as an 8th parameter
- **Issue:** `process_msg(msg, now, prefill_delay, decode_delay, max_seq_len, behavior_table, running, observe)` hit clippy's default 7-argument limit, failing the plan's own mandatory `-D warnings` gate.
- **Fix:** Bundled `prefill_delay`/`decode_delay`/`max_seq_len` into a new `Copy` `EngineConfig` struct, passed as a single `&EngineConfig` reference, bringing `process_msg` to 6 parameters. `run_engine` and `emit_due_tokens` were left as direct parameters (both stayed within the limit).
- **Files modified:** `crates/rsg-server/src/bin/mock-scheduler.rs`
- **Verification:** `cargo clippy -p rsg-server --all-targets -- -D warnings` exits 0; full `cargo test -p rsg-server` (53 tests) still passes.
- **Commit:** `8cf5467` (folded into the Task 2 GREEN commit, since this was a mechanical refactor required to make that commit's own code pass the plan's clippy gate, not a separate behavior change)

**Total deviations:** 1 auto-fixed (Rule 3 — a blocking lint failure in the plan's own required code, not scope creep).
**Impact:** None on behavior or test coverage; purely a parameter-passing refactor to satisfy a mandatory gate the plan itself specifies (`cargo clippy -p rsg-server --all-targets -- -D warnings` in both tasks' `<verify>` blocks).

## Issues Encountered
None beyond the deviation above, root-caused and fixed within this plan's own scope.

## Open Questions / Flagged Assumptions

Carried forward verbatim from the plan's own "Flagged assumptions (unresolved edge probe)" section — unresolved, never auto-resolved, for the phase verifier to confirm or reject:

- **MOCK-01, edge category "unclassified — review manually" (status: unresolved; never auto-resolved).** The deterministic edge probe could not classify MOCK-01. Below is the planner's own reading of the edges this mock leaves open. The probe has not verified it, and the phase verifier should confirm or reject it:
  1. A uid flagged for a behavior that is never submitted (or never aborted, for late-abort-token) never triggers that behavior. The mock does not report unused flags. Tests always exercise the uids they flag.
  2. A second UserMsg for a uid that is still running replaces that request and logs a warning (03-01). The real scheduler would admit a duplicate request. Phase 5 is responsible for unique uids.
  3. late-abort-token emits its late tokens only for a uid still running when the abort arrives. An abort that arrives after the uid has finished produces nothing, matching the real scheduler's no-op on unknown uids.

## User Setup Required
None — no external service configuration required. No new dependencies were added (clap, tracing, rsg-wire were all already pinned from Plan 03-01).

## Next Phase Readiness
Wave 2 complete (03-02 and 03-03 both done) — Wave 3 (03-04, 03-05) and Wave 4 (03-06) remain. Phase 5/6/7 test harnesses can now spawn `mock-scheduler` with the full D-09 misbehavior vocabulary (`--misbehave-uids`/`--behavior`/`--batch-size`) exactly as documented in this plan's `<interfaces>` contract. No blockers identified.

---
*Phase: 03-zmq-transport-mock-scheduler*
*Completed: 2026-10-06*

## Self-Check: PASSED

All created/modified files verified present on disk; all three plan commits (`4edda0e`, `3908942`, `8cf5467`) verified present in `git log --oneline --all`.
