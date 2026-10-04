---
phase: 01-vendored-base-wire-codec
plan: 07
subsystem: infra
tags: [launcher, signals, sigint, process-group, pytest-e2e, gap-closure]

requires:
  - phase: 01-vendored-base-wire-codec
    provides: "01-05 rust-mode launcher with D-12 failure contract and e2e harness"
provides:
  - "Rust-mode launcher exits 0 with no failure report on a group SIGINT, before and after the backend is ready"
  - "fake scheduler hang_before_ready status marker (hang_entered)"
  - "Three group-SIGINT e2e tests sharing one clean-stop assertion helper"
  - "CR-01 recorded as fixed in 01-REVIEW-DISPOSITION.md"
affects: [phase-01-verification, gpu_phase1_check, WR-05]

actuals:
  tokens: 4000
  tasks: 2
  commits: 4

plan_head_before: e69235e60d5094c571eb04c68f3781c6d53497f5
plan_head_after: cc3d2c8afeee87384b08c7d55160e842b68ba8dc

tech-stack:
  added: []
  patterns:
    - "Re-check the stop flag right after every blocking get, because PEP 475 retries an interrupted get to its full timeout and children have already reacted to the group signal by then"

key-files:
  created: []
  modified:
    - python/rsglang/launch.py
    - python/rsglang/testing/fake_scheduler.py
    - python/tests/test_launch_rust_e2e.py
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md

key-decisions:
  - "shutdown() left byte-for-byte unchanged (its group SIGINT stays), so the pid-only SIGTERM test still works and WR-05 stays open and unregressed"
  - "Stop re-check routes only to shutdown(0); with no stop requested every failure branch still reports and exits 1 (T-01-21)"

patterns-established:
  - "Pre-ready e2e determinism: the fake scheduler touches hang_entered inside __init__ so a test signals at a known point rather than by timing"

requirements-completed: [BASE-02]

coverage:
  - id: D1
    description: "Group SIGINT after 'backend ready; handshake sent' exits 0 with no failure report, no surviving child, no leftover socket"
    requirement: BASE-02
    verification:
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py#test_group_sigint_after_ready_exits_0"
        status: pass
    human_judgment: false
  - id: D2
    description: "Group SIGINT while the scheduler boots (children-scan branch) exits 0 the same way"
    requirement: BASE-02
    verification:
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py#test_group_sigint_while_scheduler_boots_exits_0"
        status: pass
    human_judgment: false
  - id: D3
    description: "Group SIGINT while the scheduler hangs before ready (error-envelope branch) exits 0 the same way"
    requirement: BASE-02
    verification:
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py#test_group_sigint_while_scheduler_hangs_exits_0"
        status: pass
    human_judgment: false
  - id: D4
    description: "Real failures still exit non-zero with their printed cause (D-12 tests) and the pid-only SIGTERM tracer test still exits 0"
    requirement: BASE-02
    verification:
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py (all 9 tests)"
        status: pass
    human_judgment: false
  - id: D5
    description: "The BrokenPipeError window between a ready message and the handshake write honors a pending stop"
    requirement: BASE-02
    verification: []
    human_judgment: true
    rationale: "Found by code reading in the diagnosis and not reproducible on demand, so no test asserts it; reviewer should confirm the guard by inspection"

duration: 20min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 07: Group SIGINT exits 0 in rust mode (CR-01 / G-01-2) Summary

**Rust-mode launcher now re-checks `stop_requested` after every blocking `ready_queue.get` and before each child-state-driven `shutdown(1)`, so a terminal-style group SIGINT exits 0 with no failure report in all three windows (booting, hung before ready, running).**

## Performance

- **Duration:** about 20 min (cargo build of rsg-server and the first env bootstrap included)
- **Started:** 2026-10-04T05:49Z (approx)
- **Completed:** 2026-10-04T06:09Z
- **Tasks:** 2 (tracer + auto, both TDD)
- **Files modified:** 4

## Accomplishments

- Root cause fixed as diagnosed: the handler only set a flag and PEP 475 retried the interrupted get to its full timeout, so the loop body saw children that had already exited from the same group signal and called `shutdown(1)`. Both rust-mode loops now check the flag right after the get, again at the top of each children-scan failure branch, and in the handshake `except BrokenPipeError:` branch.
- `shutdown()` is untouched (one `os.killpg(os.getpgrp(), signal.SIGINT)` remains), so WR-05 is neither fixed nor regressed.
- Three e2e tests, one shared `_group_sigint_clean_stop` helper that asserts exit 0, `rsglang.launch: exit code 0`, no launcher line containing "exited with code" / "failed" / "lines of rsg-server stderr" / "escalating to SIGKILL", children gone, and all five run sockets removed.
- CR-01 recorded `fixed` in 01-REVIEW-DISPOSITION.md (frontmatter, table row, `open:` 15 to 14); WR-05 row still `open`.

## TDD Gate Compliance

Both tasks followed RED then GREEN (`test(01-07)` precedes `feat(01-07)` for each). No refactor commits.

RED evidence (target tests failed on the planned behavior, on the unmodified launcher logic):

- Task 1, `test_group_sigint_after_ready_exits_0`: `assert 1 == 0` from `finish(30)`; launcher printed `rsglang.launch: rsg-server exited with code 0`, the rsg-server stderr tail, then `rsglang.launch: exit code 1`.
- Task 2, `test_group_sigint_while_scheduler_boots_exits_0`: exit 1 with `rsglang.launch: rsg-server exited with code -2 before ready` (the plan allowed "code 0" or "code -2" for this window).
- Task 2, `test_group_sigint_while_scheduler_hangs_exits_0`: exit 1 with `rsglang.launch: scheduler rank 0 failed:` and a `KeyboardInterrupt` traceback from `fake_scheduler.py` `time.sleep(0.5)`.

Tracer gate: Task 1 `<verify>` is automated-only; it was re-run after the GREEN commit (2 passed) and again as part of the full file before expanding. Tracer verified end-to-end, expansion proceeded.

## Task Commits

1. **Task 1 RED: failing group-SIGINT-after-ready test** - `c69a4f3` (test)
2. **Task 1 GREEN: supervise loop stop re-check** - `e60f07e` (feat)
3. **Task 2 RED: failing booting and hanging tests plus `hang_entered` marker** - `6b61c80` (test)
4. **Task 2 GREEN: ready-wait loop and BrokenPipe stop re-check, CR-01 disposition** - `cc3d2c8` (feat)

**Plan metadata:** committed separately after this file (docs: complete plan).

## Files Created/Modified

- `python/rsglang/launch.py` - post-get `if stop_requested: return shutdown(0)` in both loops; same check first in each children-scan failure branch and in the handshake BrokenPipeError branch; ready-wait `except queue.Empty` now sets `msg = None` and the scan runs under `if msg is None:`
- `python/rsglang/testing/fake_scheduler.py` - `hang_before_ready` touches `<status dir>/hang_entered` before its sleep loop
- `python/tests/test_launch_rust_e2e.py` - `_group_sigint_clean_stop` helper and three group-SIGINT tests; `_gone` moved above its first use
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` - CR-01 fixed

## Decisions Made

None beyond the plan. One placement note: `_gone` had to be defined before the new helper, so it moved up with the new stop-contract block; the D-12 failure-contract header and tests are otherwise unchanged.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- A first combined Bash command using `git rev-parse --git-dir` inside `$(...)` was refused by the worktree sandbox as too complex; split into plain commands. The plan-head ledger was written from the main repo's `.git/worktrees/<id>/` directory.

## Verification

- `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -q`: 9 passed (six pre-existing, three new).
- Group-SIGINT tests re-run 3 more times: 3 passed each time (no flakiness observed).
- `bash scripts/check_all.sh --offline`: `check_all: OK` (cargo tests, pytest 37 passed in the fast suite, fixture freshness, WIRE-02 decode, vendored tree matches pristine 9a91cfa).
- Acceptance greps: `if stop_requested:` count 7; `os.killpg(os.getpgrp(), signal.SIGINT)` count 1; `hang_entered` count 1; CR-01 `fixed` row 1; WR-05 `open` row 1; `git status --porcelain vendor/` empty.

## Known Stubs

None.

## Threat Flags

None. No new network, auth, or file-access surface; the only new file write is a test-only marker in the fake scheduler under an already test-controlled status directory.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- G-01-2 closed; `scripts/gpu_phase1_check.sh` style group-SIGINT stops no longer see a spurious failure in rust mode.
- WR-05 (shutdown re-sending SIGINT to the group after an external group SIGINT) remains open by design.
- STATE.md and ROADMAP.md intentionally not updated (orchestrator owns them in worktree mode).

## Self-Check: PASSED

- Files exist: launch.py, fake_scheduler.py, test_launch_rust_e2e.py, 01-REVIEW-DISPOSITION.md all FOUND.
- Commits c69a4f3, e60f07e, 6b61c80, cc3d2c8 all present in `git log`.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*
