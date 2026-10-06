---
phase: 261004-vqo
plan: 01
subsystem: infra
tags: [launcher, multiprocessing, process-group, signal-handling, pytest]

# Dependency graph
requires:
  - phase: 01
    provides: the rust-mode launcher (_run_rust_mode), the e2e test harness (LauncherRun/make_launcher), the D-12 SIGKILL-escalation pattern in shutdown()
provides:
  - "except BaseException guard in _run_rust_mode that SIGKILLs the launcher's process group on any unanticipated error"
  - "test_unexpected_error_leaves_no_orphans, a two-case regression test for CR-01 (spawn-loop and handshake-encode triggers)"
  - "CR-01 recorded as fixed in 01-REVIEW-DISPOSITION.md, open count 14 -> 13"
affects: [phase-01-gpu-validation, phase-06-abort-timing]

# Actuals (#2632)
actuals:
  tokens: 5283
  tasks: 2
  commits: 3
  plan_head_before: 8439eddf90c205356728e28513fe5e2cc5892a03
  plan_head_after: f3122be

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "except BaseException + inner try/finally os.killpg(..., SIGKILL) mirrors shutdown()'s existing D-12 escalation tail, so the launcher now has exactly one way to die on an unanticipated error instead of inventing a second idiom"
    - "test-only sitecustomize.py injected via PYTHONPATH, gated on 'rsglang.launch' in sys.orig_argv so the patch never reaches a spawned --multiprocessing-fork child, the resource tracker, or pytest itself"

key-files:
  created: []
  modified:
    - python/rsglang/launch.py
    - python/tests/test_launch_rust_e2e.py
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md

key-decisions:
  - "Precondition (.venv presence) was unmet at start; ran the plan-sanctioned idempotent scripts/bootstrap_mac_env.sh to satisfy it before any task work, per the precondition's own remedy text"
  - "Followed the plan's fix shape exactly: wrap everything after the rsg-server Popen in try/except BaseException, mirroring shutdown()'s SIGKILL escalation tail rather than inventing a new cleanup idiom"

patterns-established:
  - "Pattern: an unanticipated exception inside a process-spawning function must still killpg the function's own process group before re-raising, with diagnostics (log + traceback + socket unlink) run inside an inner try whose finally performs the kill, so a failure in a diagnostic step can never skip the kill"

requirements-completed: [BASE-02]

coverage:
  - id: D1
    description: "An unanticipated exception after rsg-server is spawned (spawn-loop OSError or handshake-encode ValueError) makes the launcher print the cause, SIGKILL its process group, exit non-zero, and leave no rsg-server or scheduler process behind"
    requirement: "BASE-02"
    verification:
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py::test_unexpected_error_leaves_no_orphans[spawn_loop]"
        status: pass
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py::test_unexpected_error_leaves_no_orphans[handshake_encode]"
        status: pass
    human_judgment: false
  - id: D2
    description: "No anticipated path changed behavior: every pre-existing launcher test and the whole Python suite still pass, and launch.py's whitespace-insensitive diff is additions only"
    requirement: "BASE-02"
    verification:
      - kind: integration
        ref: "python -m pytest python/tests/test_launch_rust_e2e.py python/tests/test_launch_args.py -q (24 passed)"
        status: pass
      - kind: integration
        ref: "python -m pytest python/tests -q (90 passed, 37 skipped)"
        status: pass
      - kind: other
        ref: "git diff -w --numstat -- python/rsglang/launch.py (23 added, 0 deleted)"
        status: pass
    human_judgment: false
  - id: D3
    description: "CR-01 recorded as fixed in 01-REVIEW-DISPOSITION.md, open count drops from 14 to 13, no other row changed"
    verification:
      - kind: other
        ref: "git diff --numstat -- .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md (3 added, 3 deleted)"
        status: pass
    human_judgment: false

duration: 22min
completed: 2026-10-05
status: complete
---

# Quick Task 261004-vqo: Fix CR-01 Critical Finding Summary

**An `except BaseException` guard in `_run_rust_mode` now SIGKILLs the launcher's whole process group on any unanticipated error, closing the critical finding from the 2026-10-05 incremental code review.**

## Performance

- **Duration:** ~22 min (plus env bootstrap)
- **Started:** 2026-10-04T23:18Z (approx, worktree bootstrap)
- **Completed:** 2026-10-05T06:41Z
- **Tasks:** 2 completed
- **Files modified:** 3

## Accomplishments
- Closed CR-01: `_run_rust_mode` in `python/rsglang/launch.py` now wraps everything after the rsg-server `Popen` in `try`/`except BaseException`, which logs "unexpected error in the launcher", writes the traceback, unlinks the run sockets, flushes stdout/stderr, and `os.killpg`s the launcher's own process group with `SIGKILL` before re-raising — mirroring `shutdown()`'s existing D-12 escalation tail instead of inventing a new idiom.
- Added `test_unexpected_error_leaves_no_orphans`, a two-case regression test (`spawn_loop`, `handshake_encode`) that proved RED on the unmodified launcher and GREEN after the fix.
- Verified every anticipated path is unchanged: all 24 tests in `test_launch_rust_e2e.py` + `test_launch_args.py` pass, and the whole `python/tests` suite (90 passed, 37 skipped) passes.
- Recorded CR-01 as `fixed` in `01-REVIEW-DISPOSITION.md`, dropping the open count from 14 to 13 with no other row changed.

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): CR-01 regression test** - `de6ba69` (test)
2. **Task 1 (GREEN): except BaseException guard** - `c39851f` (fix)
3. **Task 2: record CR-01 as fixed** - `f3122be` (docs)

**Plan metadata:** not committed by this executor — the orchestrator handles the SUMMARY/STATE docs commit in a later step, per this quick task's constraints.

_Note: Task 1 was a `tdd="true"` tracer task, so it carries two commits (RED test, then GREEN fix). Task 2 is a single docs commit._

## Files Created/Modified
- `python/rsglang/launch.py` - Added `import traceback`; wrapped everything in `_run_rust_mode` after the rsg-server `Popen` (spawn loop, nested `children`/`report_errors`/`shutdown` defs, ready-wait loop, handshake write, supervise loop) inside `try: ... except BaseException:`, which prints the cause, unlinks run sockets, flushes streams, and `killpg`s the group with `SIGKILL` before re-raising
- `python/tests/test_launch_rust_e2e.py` - Added an `extra_args` option to `LauncherRun`; two module-level sitecustomize-source constants (`_SPAWN_LOOP_HOOK`, `_HANDSHAKE_HOOK`) gated on `"rsglang.launch" in sys.orig_argv`; `test_unexpected_error_leaves_no_orphans`, parametrized over `spawn_loop` (`--tp-size 2`, rank 1's `start()` raises `OSError` after rank 0 is up) and `handshake_encode` (`encode_handshake_line` raises `ValueError`)
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` - CR-01 frontmatter `disposition: open` -> `fixed`, `open: 14` -> `open: 13`, and the CR-01 table row's Source cell now cites this quick-task plan and the fix

## Decisions Made
- The `.venv` precondition was unmet in this fresh worktree. The plan's own precondition text names the remedy (`scripts/bootstrap_mac_env.sh`, explicitly idempotent and worktree-safe), so it was run once before any task work rather than halting — this is the plan author's sanctioned setup step, not an ambiguous blocker requiring human sign-off.
- Followed the plan's fix shape exactly (mirror `shutdown()`'s SIGKILL escalation tail) rather than any alternative cleanup idiom, per the plan's explicit instruction and the review's recommendation.

## Deviations from Plan

None - plan executed exactly as written. Both regression cases reproduced the exact RED failure mode described in the plan ("the launcher hangs in multiprocessing's exit-time join of the non-daemon rank 0, with both children alive") before the fix, and both passed after it.

## Issues Encountered
- The worktree had no `.venv` yet (fresh checkout). Resolved per the task's `<precondition>` by running `bash scripts/bootstrap_mac_env.sh`, which created a project-local uv venv and installed `minisgl` + `rsglang` editable, no other side effects.

## RED Evidence (recorded per plan's success_criteria)

Both cases failed on the unmodified `launch.py` with the exact "still running" failure predicted during planning, before any fix commit:

**`spawn_loop`** (`--tp-size 2`, rank 1's `start()` raises after rank 0 is up):
```
Failed: launcher still running 30 s after the injected error; its children were never killed (CR-01)
rsglang.launch: spawned rsg-server pid=70587
rsglang.launch: spawned scheduler rank=0 pid=70589
...
Traceback (most recent call last):
  ...
  File ".../python/rsglang/launch.py", line 175, in _run_rust_mode
    p.start()
  File ".../hook/sitecustomize.py", line 21, in _start
    raise OSError("injected: TP rank 1 failed to start")
OSError: injected: TP rank 1 failed to start
```

**`handshake_encode`** (`encode_handshake_line` raises `ValueError`):
```
Failed: launcher still running 30 s after the injected error; its children were never killed (CR-01)
rsglang.launch: spawned rsg-server pid=70080
rsglang.launch: spawned scheduler rank=0 pid=70082
...
Traceback (most recent call last):
  ...
  File ".../python/rsglang/launch.py", line 269, in _run_rust_mode
    rust.stdin.write(handshake.encode_handshake_line(payload))
  File ".../hook/sitecustomize.py", line 7, in _raise_handshake_error
    raise ValueError("injected: handshake keys drifted")
ValueError: injected: handshake keys drifted
```

Both cases took ~30-36 s each to fail (the fixture's `cleanup()` then SIGKILLed the hung group), confirming the hang is real and not a flaky timeout.

## GREEN Evidence

- `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py python/tests/test_launch_args.py -q` -> `24 passed in 55.17s` (includes both new parametrized cases)
- `.venv/bin/python -m pytest python/tests -q` -> `90 passed, 37 skipped in 80.74s`
- `git diff -w --numstat -- python/rsglang/launch.py` -> `23  0` (whitespace-insensitive: additions only)
- `grep -c "except BaseException:" python/rsglang/launch.py` -> `1`
- `grep -c "os.killpg(os.getpgrp(), signal.SIGKILL)" python/rsglang/launch.py` -> `2`
- `grep -c "unexpected error in the launcher" python/rsglang/launch.py` -> `1`
- `grep -cx "import traceback" python/rsglang/launch.py` -> `1`
- awk ordering check (setpgid < Popen < try < spawn log < except) -> exit 0
- `git status --porcelain vendor/` -> empty, both before and after the fix commit

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- CR-01 is closed; WR-01, IN-01, and IN-02 remain `open` as the plan scoped them out intentionally.
- Phase 01's GPU-machine validation pass can proceed without this known orphan-leak risk on unanticipated launcher errors.

## Self-Check: PASSED

- FOUND: python/rsglang/launch.py
- FOUND: python/tests/test_launch_rust_e2e.py
- FOUND: .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
- FOUND commit: de6ba69
- FOUND commit: c39851f
- FOUND commit: f3122be

---
*Phase: 261004-vqo*
*Completed: 2026-10-05*
