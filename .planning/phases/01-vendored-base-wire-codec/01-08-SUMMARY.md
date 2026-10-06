---
phase: 01-vendored-base-wire-codec
plan: 08
subsystem: infra
tags: [multiprocessing, watchdog, prctl, pdeathsig, orphan-process, gap-closure]

requires:
  - phase: 01-vendored-base-wire-codec
    provides: "01-07 group-SIGINT clean stop and the launcher's supervise loop"
provides:
  - "Launcher pid passed explicitly to every scheduler rank"
  - "start_parent_watchdog(launcher_pid): Linux PR_SET_PDEATHSIG(SIGKILL), immediate getppid re-check, polling backstop"
  - "Early-kill (during scheduler boot) no-orphan e2e test and watchdog unit tests"
  - "GPU step 4b early-kill orphan check"
  - "WR-02 recorded as fixed"
affects: [phase-1 GPU verification, 01-VERIFICATION, 01-UAT]

actuals:
  tokens: 9000
  tasks: 3
  commits: 5  # measured before the SUMMARY commit: git rev-list --count d96c92e..HEAD

tech-stack:
  added: []
  patterns:
    - "Parent identity is captured by the parent at spawn time and passed to the child, never read by the child after boot"
    - "Test-only sitecustomize in tmp_path, keyed on '--multiprocessing-fork' in sys.orig_argv, to widen a spawn child's boot window deterministically"

key-files:
  created:
    - python/tests/test_parent_watchdog.py
  modified:
    - python/rsglang/launch.py
    - python/rsglang/backend.py
    - python/tests/test_launch_rust_e2e.py
    - scripts/gpu_phase1_check.sh
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md

key-decisions:
  - "Pass the launcher pid as an explicit run_scheduler argument rather than using multiprocessing.parent_process() or a sentinel thread: an argument is trivially injectable in unit tests"
  - "A failing prctl(PR_SET_PDEATHSIG) raises OSError so a broken Linux setup fails before the handshake instead of silently losing the guard"
  - "Keep os._exit(1) in the watchdog: no parent is left to coordinate a clean exit"

patterns-established:
  - "Early-window process-death tests: slow-boot sitecustomize via extra_env on LauncherRun"

requirements-completed: [BASE-02]

duration: 25min
completed: 2026-10-03
status: complete
plan_head_before: d96c92e2f6523e8193bf657a1e409acdbbf31520
plan_head_after: 0c402d4bfbf41d470e780c22f24898d020ae7bb9

coverage:
  - id: D1
    description: "kill -9 of the launcher while the scheduler child is still booting leaves no scheduler or rsg-server (Mac, fake scheduler)"
    requirement: BASE-02
    verification:
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py#test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans"
        status: pass
      - kind: e2e
        ref: "python/tests/test_launch_rust_e2e.py#test_launcher_sigkill_leaves_no_orphans"
        status: pass
    human_judgment: false
  - id: D2
    description: "Watchdog contract: wrong parent exits 1 promptly; right parent stays alive"
    requirement: BASE-02
    verification:
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py#test_exits_at_once_when_parent_is_not_the_launcher"
        status: pass
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py#test_stays_alive_while_parent_is_the_launcher"
        status: pass
    human_judgment: false
  - id: D3
    description: "Linux PR_SET_PDEATHSIG(SIGKILL) is armed by the watchdog; GPU step 4b kills the launcher during scheduler boot and finds no orphan"
    requirement: BASE-02
    verification: []
    human_judgment: true
    rationale: "The Linux prctl branch and step 4b run only on the Linux GPU box; this Mac cannot execute them (test_linux_arms_pdeathsig_sigkill is skipped on macOS)"
---

# Phase 1 Plan 08: Watchdog launcher-pid gap closure Summary

**Scheduler parent watchdog now compares against the launcher pid passed at spawn time (with an immediate re-check and Linux PR_SET_PDEATHSIG), so kill -9 of the launcher during scheduler boot no longer leaves an orphan.**

## Performance

- **Duration:** about 25 min (includes the first cargo build with bundled libzmq)
- **Tasks:** 3
- **Files modified:** 6 (1 created)

## Accomplishments

- Closed G-01-3 / WR-02: the watchdog no longer records its reference parent after boot, when a dead launcher has already reparented the child to init.
- `launch.py` passes `os.getpid()` as the fourth `run_scheduler` argument; `start_parent_watchdog(launcher_pid, poll_interval)` exits with code 1 immediately if `os.getppid() != launcher_pid`, then polls against the same pid.
- On Linux the watchdog first arms `prctl(PR_SET_PDEATHSIG, SIGKILL)` and raises `OSError` if it fails.
- New early-kill e2e test, watchdog unit tests, GPU step 4b, and WR-02 marked fixed in the disposition.

## TDD record (Task 1)

- **RED** (commit `7de9f47`): on the unmodified source, `test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans` failed with `AssertionError: scheduler pid=15160 orphaned` after the 30 s deadline (the 3 s sitecustomize sleep makes the kill land inside the boot window). Failure is on the targeted assertion, not a setup error.
- **GREEN** (commit `8acade2`): same test passes in the full `-k launcher_sigkill` run (2 passed, 17.8 s).

## Task Commits

1. **Task 1 (tracer): early-kill test + explicit launcher pid** - `7de9f47` (test, RED), `8acade2` (feat, GREEN)
2. **Task 2: Linux PR_SET_PDEATHSIG + watchdog unit tests** - `7206089` (test), `7ef11a1` (feat)
3. **Task 3: GPU step 4b + WR-02 fixed** - `0c402d4` (chore)

**Plan metadata:** docs(01-08) commit following this file.

## Files Created/Modified

- `python/rsglang/launch.py` - rank args now `(rank_args, ready_queue, upstream_sha, os.getpid())`
- `python/rsglang/backend.py` - `start_parent_watchdog(launcher_pid, poll_interval)`, `PR_SET_PDEATHSIG = 1`, `run_scheduler(..., launcher_pid)`
- `python/tests/test_launch_rust_e2e.py` - `LauncherRun(extra_env=...)`, early-kill e2e test with a tmp_path-only sitecustomize (never committed)
- `python/tests/test_parent_watchdog.py` - three watchdog unit tests (`slow` marker)
- `scripts/gpu_phase1_check.sh` - `step4_early`, record `4b`, usage line, log `rust-mode-early-kill.log`
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` - WR-02 fixed, open count 13

## Decisions Made

See key-decisions in the frontmatter. No vendor/ file was touched (`git status --porcelain vendor/` is empty).

## Deviations from Plan

### TDD gate note

**Task 2's RED phase is not observable on this Mac.** The two watchdog tests that run on macOS (wrong parent exits; right parent stays alive) pin behavior that Task 1 had already made true, exactly as the plan states ("on the Mac the first two tests already pass after Task 1"). The third test (`test_linux_arms_pdeathsig_sigkill`) is skipped on macOS, so no RED run for the prctl was possible here. The test commit (`7206089`) still precedes the feat commit (`7ef11a1`).

Total deviations: 0 auto-fixed. Impact: none on scope.

## Issues Encountered

- A first attempt to record the plan-head ledger and a combined edit command were rejected by the worktree command guard as too complex; they were split into plain commands. No effect on the result. The base `d96c92e` was recorded from the spawn-time check.

## Verification

- `python/tests/test_launch_rust_e2e.py` + `python/tests/test_parent_watchdog.py`: 12 passed, 1 skipped (Linux-only PDEATHSIG test), including 01-07's group-SIGINT tests and the post-handshake kill test.
- `bash scripts/check_all.sh --offline`: `check_all: OK`.
- `bash -n scripts/gpu_phase1_check.sh` ok, `--help` prints Usage and the 4b line; `git ls-files '*sitecustomize.py'` empty; `vendor/` clean.
- **Not verified on this machine (Linux/GPU only):** the `prctl(PR_SET_PDEATHSIG)` branch in `backend.py`, `test_linux_arms_pdeathsig_sigkill`, and GPU step 4b. These are covered by the end-of-phase human check on the GPU box (`bash scripts/gpu_phase1_check.sh`, expect PASS for steps 1, 2, 3, 4, 4b, 5). The CUDA-torch boot window used for step 4b's 120 s allowance is a projection, not measured.

## User Setup Required

None - no external service configuration required.

## Known Stubs

None.

## Threat Flags

None. The only new surface is a constant-argument `prctl` call on the scheduler's own process (T-01-23, accepted in the plan).

## Next Phase Readiness

- G-01-3 closed on the Mac side; the GPU box run of `scripts/gpu_phase1_check.sh` (including step 4b) remains the human sign-off.

## Self-Check: PASSED

- Files exist: `python/tests/test_parent_watchdog.py`, this SUMMARY.
- Commits found: `7de9f47`, `8acade2`, `7206089`, `7ef11a1`, `0c402d4`.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-03*
