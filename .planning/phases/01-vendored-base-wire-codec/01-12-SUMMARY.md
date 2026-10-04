---
phase: 01-vendored-base-wire-codec
plan: 12
subsystem: gpu-verification
tags: [bash, pytest, tdd, gpu-check, pdeathsig, watchdog]
requires:
  - phase: 01-vendored-base-wire-codec (01-09, 01-10)
    provides: "scripts/gpu_phase1_check.sh's source guard and helper block (01-09); python/rsglang/backend.py's start_parent_watchdog degrade-to-polling behavior and its fixed 'PDEATHSIG unavailable' stderr message (01-10)"
provides:
  - "pdeathsig_degraded <log> helper in scripts/gpu_phase1_check.sh, reachable by sourcing (above the source guard), returning 0 when a log contains the fixed string 'PDEATHSIG unavailable' and 1 otherwise (including for a missing/unreadable log)"
  - "step3 now fails with a clear message when the scheduler degraded to the polling watchdog during the run it just verified"
  - "--help's step 3 line documents the PDEATHSIG requirement"
  - "three new Mac-runnable pytest cases pinning pdeathsig_degraded's three behaviors"
affects: [gpu-verification, phase-01-gap-closure, future-phase-gpu-gates]
actuals:
  tokens: 930
  tasks: 1
  commits: 2
  plan_head_before: 462b100ebae8862ed3ca98c6d3b9a2c64cdf6dff
  plan_head_after: 11df2d4
tech-stack:
  added: []
  patterns:
    - "Fixed-string grep on redirected stderr as the GPU-check's signal for a Python-side degrade, same pattern as the existing handshake-line and child-pid greps in step3"
key-files:
  created: []
  modified:
    - scripts/gpu_phase1_check.sh
    - python/tests/test_gpu_check_script.py
key-decisions:
  - "Normalized pdeathsig_degraded's exit code to strictly 0/1 (grep alone returns 2 on a missing/unreadable log under `set -euo pipefail`'s error semantics); the plan's behavior spec required a missing log to return 1, not grep's raw 2. Found and fixed during GREEN verification, before any commit (not a post-hoc patch)."
patterns-established: []
requirements-completed: [BASE-02, BASE-03]
coverage:
  - id: D1
    description: "pdeathsig_degraded helper, step3 wiring, and --help update so a GPU step-3 PASS once again proves PR_SET_PDEATHSIG armed"
    requirement: "BASE-03"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_check_script.py::test_pdeathsig_degraded_detects_the_fallback_line, ::test_pdeathsig_degraded_passes_a_clean_log, ::test_pdeathsig_degraded_on_missing_log"
        status: pass
      - kind: integration
        ref: ".venv/bin/python -m pytest python/tests/test_gpu_check_script.py -q (14 passed, including 11 pre-existing WR-07/WR-08 tests)"
        status: pass
      - kind: integration
        ref: "bash scripts/check_all.sh --offline (full Phase 1 Mac gate: cargo test --workspace, pytest python/tests, fixture freshness, WIRE-02 decode, check_upstream.py --offline) -> check_all: OK"
        status: pass
    human_judgment: false
  - id: D2
    description: "On a real GPU box, step 3 actually PASSes (proving PR_SET_PDEATHSIG armed on that machine), not just that the helper's string match is correct on Mac fixtures"
    verification: []
    human_judgment: true
    rationale: "Plan's own <verification> marks this 'Non-blocking: the collaborator's GPU-box run (UAT test 1) shows step 3 PASS' -- it requires Linux + nvidia-smi + a real backend run, unavailable on this Mac dev machine. The Mac-side proof (D1) is everything this plan can verify locally; the GPU-box confirmation is deferred to the collaborator's run per the plan text itself."
duration: ~20min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 12: GPU check fails on PDEATHSIG degrade Summary

**gpu_phase1_check.sh step 3 now fails if rust-mode.log shows the scheduler fell back to the polling watchdog ("PDEATHSIG unavailable"), restoring the proof that a passing GPU check means PR_SET_PDEATHSIG armed on that machine -- a property 01-10's degrade-to-polling fix had given up.**

## Performance
- **Duration:** ~20min
- **Completed:** 2026-10-04
- **Tasks:** 1 (TDD: RED, GREEN; no REFACTOR needed)
- **Files modified:** 2

## Accomplishments
- Added `pdeathsig_degraded <log>` above `scripts/gpu_phase1_check.sh`'s source guard, next to the other Mac-testable helpers (`alive`, `gpu_pids`, `on_gpu`, `wait_no_orphans`).
- Wired it into `step3`, right after the handshake line is found and the child pids (`RSG_PID`, `SCHED_PID`) are read, before the field checks: a degrade now fails step 3 with `"scheduler fell back to the polling watchdog: PDEATHSIG unavailable (see $log)"`.
- Extended the `--help` step 3 line to state the scheduler must have armed PDEATHSIG.
- Added three Mac-runnable pytest cases (`test_pdeathsig_degraded_detects_the_fallback_line`, `test_pdeathsig_degraded_passes_a_clean_log`, `test_pdeathsig_degraded_on_missing_log`), following the existing `_bash`/`_rc` runner convention in `python/tests/test_gpu_check_script.py`.
- `python/rsglang/backend.py` and every other step (1, 2, 4, 4b, 5) were untouched, per the plan.

## Task Commits
1. **Task 1 (RED): add failing tests for pdeathsig_degraded** - `cd8c57b` (test) -- confirmed the three new tests fail with exit 127 ("command not found") because the helper does not exist yet in the sourced script.
2. **Task 1 (GREEN): implement pdeathsig_degraded and wire it into step3** - `11df2d4` (feat) -- all 14 tests in `python/tests/test_gpu_check_script.py` pass (11 pre-existing WR-07/WR-08 tests + 3 new).

No REFACTOR commit: the GREEN implementation needed one in-flight exit-code fix (see Deviations) before any commit was made, so there was nothing left to clean up afterward.

**Plan metadata:** committed in a follow-up `docs(01-12)` commit alongside this file (see commit list in the final handback).

## Files Created/Modified
- `scripts/gpu_phase1_check.sh` - added `pdeathsig_degraded()` helper above the source guard; `step3` now calls it and fails on a degrade; `usage()`'s step 3 line mentions PDEATHSIG.
- `python/tests/test_gpu_check_script.py` - three new tests in a `--- pdeathsig_degraded (G-01-7-WR06-CHECK) ---` section.

## Decisions Made
- **Normalize `pdeathsig_degraded`'s exit code to 0/1, not grep's raw exit code.** `grep -qF ... "$1" 2>/dev/null` alone returns 2 (not 1) when `$1` is missing or unreadable, because that is grep's own "file error" exit status, distinct from "pattern not found" (1). The plan's `<behavior>` block requires a missing log to return 1 ("step3 already fails earlier when the log has no handshake"), so the helper now does `grep ... && return 0; return 1` to collapse both grep failure modes (no match, and file error) into a single `1`. Found via `test_pdeathsig_degraded_on_missing_log` failing with `2 != 1` during the first GREEN run, fixed immediately, re-verified with the full suite before committing GREEN (Rule 1 - Bug, inline per the shared deviation process; no separate commit needed since the fix landed before the GREEN commit was made).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `pdeathsig_degraded` returned grep's raw exit code (2 on a missing log) instead of the spec's 1**
- **Found during:** Task 1, first GREEN test run (before any GREEN commit)
- **Issue:** `pdeathsig_degraded() { grep -qF 'PDEATHSIG unavailable' "$1" 2>/dev/null; }` returns whatever `grep` returns: 0 (match), 1 (no match), or 2 (file missing/unreadable, e.g. "No such file or directory"). The plan's `<behavior>` spec requires exactly 1 for a missing log, matching the existing test for `pdeathsig_degraded_on_missing_log`.
- **Fix:** Changed the helper to `grep -qF 'PDEATHSIG unavailable' "$1" 2>/dev/null && return 0; return 1`, collapsing every non-match outcome (grep 1 or grep 2) into a single returned `1`.
- **Files modified:** `scripts/gpu_phase1_check.sh`
- **Verification:** `.venv/bin/python -m pytest python/tests/test_gpu_check_script.py -q` -> 14 passed (previously 13 passed, 1 failed with `2 != 1`).
- **Commit:** folded into `11df2d4` (the fix predates the GREEN commit; no separate bug-fix commit was needed).

**Total deviations:** 1 auto-fixed (1 bug). **Impact:** none on scope or behavior contract -- the fix makes the helper match the plan's own stated spec exactly; no additional files or logic beyond what the plan's `<action>` block already described.

### Environment note (not a plan deviation)

This worktree had no local `.venv` (gitignored, not shared across worktrees, same situation 01-11 documented). Per the same precondition pattern 01-11 used, ran `bash scripts/bootstrap_mac_env.sh` (idempotent, Claude-automated) before the plan's exact verify commands (`.venv/bin/python -m pytest ...`, `bash scripts/check_all.sh --offline`) so they could run as specified rather than substituting a different interpreter. This is infrastructure setup, not a code or plan deviation.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required. The one setup action taken (running `scripts/bootstrap_mac_env.sh` to create this worktree's local `.venv`, same as 01-11) was Claude-automated, not a manual step.

## Next Phase Readiness

Phase 01 gap closure complete; ready for phase re-verification. This was the LAST gap-closure plan in the `--gaps-only` chain for Phase 01 (wave 9, `depends_on: ["01-11"]`). G-01-7-WR06-CHECK is closed:

- `bash -n scripts/gpu_phase1_check.sh` exits 0; `--help` mentions PDEATHSIG.
- `.venv/bin/python -m pytest python/tests/test_gpu_check_script.py -q` -> 14 passed.
- `bash scripts/check_all.sh --offline` -> `check_all: OK` (cargo test --workspace, pytest python/tests, fixture freshness, WIRE-02 decode, check_upstream.py --offline all pass).
- `git status --porcelain vendor/` prints nothing.
- `git diff --stat` since this plan's base touches only `scripts/gpu_phase1_check.sh` and `python/tests/test_gpu_check_script.py` (plus this SUMMARY and REQUIREMENTS.md in the metadata commit).
- Non-blocking: a real GPU-box run of step 3 (UAT test 1) is still the collaborator's job -- this plan proves the check logic is correct on Mac fixtures, not that any specific GPU box arms PDEATHSIG.

`requirements: [BASE-02, BASE-03]` were already marked `[x]` / `Complete` in `.planning/REQUIREMENTS.md` by earlier plans in this chain; no change needed there.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED

- FOUND: scripts/gpu_phase1_check.sh
- FOUND: python/tests/test_gpu_check_script.py
- FOUND: .planning/phases/01-vendored-base-wire-codec/01-12-SUMMARY.md
- FOUND commit: cd8c57b (test(01-12): add failing tests for pdeathsig_degraded)
- FOUND commit: 11df2d4 (feat(01-12): fail step 3 when the scheduler degraded to the polling watchdog)
- FOUND commit: 760ab7d (docs(01-12): complete gap closure plan)
