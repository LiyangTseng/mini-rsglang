---
phase: 06-gpu-end-to-end-parity
plan: 02
subsystem: testing
tags: [bash, gpu-verification, process-health, d-12, mac-ci]

requires:
  - phase: 01-foundation
    provides: "scripts/gpu_phase1_check.sh's alive/gpu_pids/on_gpu helper semantics, usage()/argument-loop/source-guard conventions, and the test_gpu_check_script.py stub-on-PATH technique, all reused here"
provides:
  - "scripts/gpu_phase6_watch.sh: D-12 thin process-health watcher (crash/zombie/restart/nvidia-smi-failure observation) with sourceable helpers"
  - "python/tests/test_gpu_phase6_watch.py: Mac-runnable tests covering the tracer path plus zombie, restart, nvidia-smi failure, unlisted-pid, usage and sourced-helper paths"
affects: [06-05-abort-stress]

actuals:
  tokens: 4524
  tasks: 2
  commits: 2
  plan_head_before: 5194b8dc7ca0bb405954e41388103abce2106c13
  plan_head_after: 98854bb0b5f925d21d3583f9747bb06d6b7e07c4

tech-stack:
  added: []
  patterns:
    - "File order: argument loop -> helpers -> source guard -> required-option check -> main loop, so sourcing the script for tests runs nothing past the guard while an executed invocation still gets every exit-2 usage path (mirrors gpu_phase1_check.sh's layout)"
    - "0.1s sleep slicing with a STOP flag set by a TERM/INT trap, so a signal is noticed within about one slice regardless of the configured --interval"

key-files:
  created:
    - scripts/gpu_phase6_watch.sh
    - python/tests/test_gpu_phase6_watch.py
  modified: []

key-decisions:
  - "New standalone script rather than extending gpu_phase1_check.sh (per CONTEXT.md's Claude's-discretion note): gpu_phase1_check.sh is Phase 1's signed-off GPU artifact, and sourcing it would run its own argument parsing and mktemp. The three shared helpers (alive, gpu_pids, on_gpu) are copied with identical semantics instead, following gpu_phase2_profile.sh's precedent of defining its own small helpers rather than sourcing a sibling script"
  - "Task 2's tests pass against Task 1's implementation unmodified: Task 1's own action block already specified the complete counter set (crashed, zombie, restarts, gpu_unlisted, nvsmi_errors) and helper semantics, so Task 2's zombie/restart/nvsmi-failure/usage/sourced-helper tests found no bug to fix. Documented honestly below rather than invented a cosmetic refactor to manufacture a GREEN diff"

requirements-completed: [PAR-02]

coverage:
  - id: D1
    description: "scripts/gpu_phase6_watch.sh --pid PID polls the watched scheduler pid; a dead pid is reported crashed=1/verdict=unhealthy and exits 1 immediately"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_tracer_crash_detected"
        status: pass
    human_judgment: false
  - id: D2
    description: "A live, healthy pid keeps sampling until SIGTERM/SIGINT, then reports crashed=0 zombie=0 restarts=0 verdict=healthy and exits 0"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_tracer_healthy_until_sigterm"
        status: pass
    human_judgment: false
  - id: D3
    description: "A zombie pid is reported zombie=1/unhealthy; more than one scheduler rank=0 spawn line in --launcher-log is reported as restarts and is unhealthy"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_zombie_detected"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_restart_detected_from_launcher_log"
        status: pass
    human_judgment: false
  - id: D4
    description: "An nvidia-smi failure is counted in nvsmi_errors and never treated as listed; an unlisted pid is counted in gpu_unlisted. Neither decides the verdict"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_nvidia_smi_failure_counted_not_fatal"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_unlisted_pid_counted"
        status: pass
    human_judgment: false
  - id: D5
    description: "Usage errors (missing --pid, bad --pid, unknown flag) exit 2; --help exits 0; sourcing the script for tests runs nothing past the guard"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_usage_errors"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_sourced_helpers"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_watch.py#test_help_prints_usage"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-10-06
status: complete
---

# Phase 6 Plan 2: D-12 Process-Health Watcher Summary

**`scripts/gpu_phase6_watch.sh`: a thin bash watcher that follows `gpu_phase1_check.sh`'s helper and layout conventions to report a watched scheduler pid as crashed, zombied, or restarted, with nvidia-smi failures and unlisted pids counted but never deciding the verdict.**

## Performance

- **Duration:** 20 min
- **Tasks:** 2 completed
- **Files modified:** 2 (both new)

## Accomplishments

- `scripts/gpu_phase6_watch.sh`: `--pid PID [--interval SECONDS] [--out FILE] [--launcher-log FILE] [--help]`, with a numeric-`--pid` validation, `usage()` heredoc, and an argument loop in `gpu_phase1_check.sh`'s style
- Helpers `alive`, `gpu_pids`, `on_gpu` copied verbatim from `gpu_phase1_check.sh` lines 106-114 (0 listed, 1 unlisted, 2 nvidia-smi failed); new `proc_state` (first char of `ps -o stat=`, or `"gone"`, using the `|| true` capture pattern `start_session` uses) and `scheduler_spawns` (count of `rsglang.launch: spawned scheduler rank=0 pid=` lines, 0 when the log is missing)
- Main loop samples state and GPU listing on `--interval` (default 1s, sliced into 0.1s steps so a `TERM`/`INT` trap is noticed within about one slice), appends `t=... pid=... state=... gpu=...` sample lines to `--out`, and on exit prints/appends `WATCH summary pid=... samples=... crashed=... zombie=... restarts=... gpu_unlisted=... nvsmi_errors=... verdict=...`
- Exit codes: 0 healthy, 1 unhealthy, 2 usage error. File order (argument loop, helpers, source guard, required-`--pid` check, main loop) means sourcing the file for tests runs nothing past the guard, while an executed invocation still reaches every exit-2 usage path
- `python/tests/test_gpu_phase6_watch.py`: 9 Mac-runnable tests with a stub `nvidia-smi` on PATH and real `ps`/`sleep`/fork, covering the tracer (crash + healthy-until-SIGTERM), zombie (an unreaped forked child), restart (two launcher-log spawn lines), nvidia-smi failure (counted, non-fatal), unlisted pid (counted, non-fatal), usage errors, and sourced-helper behavior

## Task Commits

1. **Task 1: Tracer — the watcher sees a watched pid die and reports a crash; a healthy pid reports healthy on SIGTERM** - `e1a14f6` (feat)
2. **Task 2: Zombie, restart, nvidia-smi failure and usage paths, with sourced-helper tests** - `98854bb` (test)

No separate GREEN/feat commit for Task 2: see "TDD Gate Compliance" below — all 6 new tests passed against Task 1's implementation unmodified, so there was no script change to commit.

**Plan metadata:** recorded separately in the `docs(06-02)` commit that adds this SUMMARY, STATE.md, ROADMAP.md and REQUIREMENTS.md.

## Files Created/Modified

- `scripts/gpu_phase6_watch.sh` - D-12 process-health watcher (new, executable)
- `python/tests/test_gpu_phase6_watch.py` - tracer + comprehensive path tests (new)

## Decisions Made

- New standalone script, not an extension of `gpu_phase1_check.sh`: that script is Phase 1's signed-off GPU artifact, and sourcing it in production would re-run its own argument parsing and `mktemp`. The three shared helpers (`alive`, `gpu_pids`, `on_gpu`) are copied with identical semantics, following `gpu_phase2_profile.sh`'s precedent of defining its own small helpers (e.g. `record`) rather than sourcing a sibling script.
- Task 2 is `tdd="true"`, but `workflow.tdd_mode` is `false` project-wide (confirmed via `config-get`, matching 06-01's Plan precedent), so the automated RED/GREEN gate tool was not invoked. The RED phase was still run in spirit — the 6 new tests were written and executed before any further script edits — but all 6 passed immediately: Task 1's own action block had already specified the complete counter set (`crashed`, `zombie`, `restarts`, `gpu_unlisted`, `nvsmi_errors`) and every helper's exact semantics, so there was no missing behavior for Task 2 to add. This is documented honestly rather than manufacturing a cosmetic script edit to produce an artificial GREEN commit.

## Deviations from Plan

None - plan executed exactly as written. Task 1's tracer already implemented the full main loop (including zombie/restart/gpu counters) as specified in its own `<action>` block; Task 2 added the comprehensive test coverage for those same paths plus usage/sourced-helper tests, exactly as its `<action>` and `<behavior>` blocks described.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

`scripts/gpu_phase6_watch.sh` is ready for plan 06-05 to start (`--launcher-log`, `--out`) and stop (SIGTERM) around each real-backend 128-request cancellation stress run. Its CLI, helper functions, sample-line and summary-line formats, and exit codes (0/1/2) match the plan's `<interfaces>` contract exactly, so 06-05 can consume it without any shape negotiation. No change to Phase 5's stress tool or to `gpu_phase1_check.sh` was made (D-11, D-12 preserved).

## TDD Gate Compliance

Task 2 (`tdd="true"`) added tests only; `workflow.tdd_mode` is `false` project-wide so the automated `gsd_run check tdd-red-evidence` gate was not invoked (same situation 06-01 documented for its own Task 2). Manual record:

```
command: .venv/bin/python -m pytest python/tests/test_gpu_phase6_watch.py -q -k "zombie or restart or nvidia_smi_failure or unlisted or usage_errors or sourced_helpers"
exit_code: 0
result: 6 passed, 3 deselected
```

This is an "unexpected GREEN" in the strict TDD sense (fail-fast rule 1: "the feature may already exist"). Investigation: it does — Task 1's `<action>` block (not Task 2's) specified the complete behavior, including the zombie/restart/nvsmi/unlisted counters and the exact helper semantics these 6 tests verify, and Task 1's implementation was written in full on the first pass. No bug was exposed by running the harder edge cases (macOS `Z`/`Z+` zombie-stat truncation via `${stat:0:1}`, `ps` exiting non-zero for a missing pid via the `|| true` capture pattern, restart detection re-reading the log every tick, and the sourced-helper file ordering) — all were already handled correctly. Commit `98854bb` is a `test(06-02)` commit with no accompanying `feat(06-02)` commit for this task, which is the honest record of what happened rather than a violation to paper over.

| Gate | Commit | Status |
|------|--------|--------|
| RED (Task 2 tests run against existing code) | `98854bb` `test(06-02): cover zombie, restart, nvsmi failure, usage and sourced helpers` | All 6 new tests passed on first run — no failing RED achieved, investigated and found the behavior already existed from Task 1 |
| GREEN | n/a | No implementation change needed; see investigation above |
| REFACTOR | none | No behavior-neutral cleanup needed |

## Self-Check: PASSED

- Both created files found on disk: `scripts/gpu_phase6_watch.sh` (executable), `python/tests/test_gpu_phase6_watch.py`.
- Both task commits (`e1a14f6`, `98854bb`) found in `git log`.
- Re-ran plan-level `<verification>`: `.venv/bin/python -m pytest python/tests/test_gpu_phase6_watch.py -q` -> 9 passed; `bash -n scripts/gpu_phase6_watch.sh` -> exit 0.
- Re-ran every task's `<acceptance_criteria>` command: Task 1 (`pytest -k tracer` -> 2 passed, `--help` exits 0 with `--pid PID` in output, `test -x` passes) and Task 2 (`pytest -q` -> 9 passed, `--pid abc` -> `rc=2`, `grep -c 'BASH_SOURCE\[0\]'` -> 1) all passed.
