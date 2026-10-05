---
phase: 01-vendored-base-wire-codec
plan: 13
subsystem: launcher/watchdog
tags: [cli-args, argparse, prctl, ctypes, pytest]

requires:
  - phase: 01-12
    provides: GPU check script's PDEATHSIG-degrade detection (pdeathsig_degraded), which this plan's fix stays compatible with
provides:
  - rust mode parses upstream args once and rejects every spelling of --shell-mode before resolving the rsg-server binary
  - parent watchdog degrades to the polling thread on a missing prctl symbol (AttributeError), not just OSError
affects: [launcher CLI, parent-watchdog robustness, phase 01 verification]

actuals:
  tokens: 9000
  tasks: 2
  commits: 5

tech-stack:
  added: []
  patterns:
    - "Parse upstream args exactly once, before any side-effecting precondition check, so the authoritative run_shell flag wins over a literal pre-check's blind spot"
    - "Broaden a defence-in-depth except clause to the full class of 'symbol/call unavailable' exceptions (OSError + AttributeError) rather than the one exception type first observed"

key-files:
  created: []
  modified:
    - python/rsglang/launch.py
    - python/rsglang/backend.py
    - python/tests/test_launch_args.py
    - python/tests/test_parent_watchdog.py
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md

key-decisions:
  - "Decision 1: kept the literal \"--shell-mode\" in rest pre-check as run_rust_mode's first statement (01-11's original fast-path), and added the authoritative parse_args/run_shell check immediately after it, before resolve_rust_bin — planner/executor discretion, matches 01-REVIEW.md's intent without dropping the existing fast path"
  - "Decision 2: did not thread run_shell down into _run_rust_mode — by the time it runs, run_shell is always false, so the parameter would be dead; _run_rust_mode's signature changed to take the parsed ServerArgs instead of the raw argv list"

requirements-completed: [BASE-02]

coverage:
  - id: D1
    description: "rust mode reports the --shell-mode rejection before a missing rsg-server binary, for every spelling (--shell, --shell-m, --shell-mode), closing gap G-01-8 / review finding WR-01"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_launch_args.py#test_rust_mode_reports_shell_mode_before_missing_binary"
        status: pass
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py (full suite)"
        status: pass
    human_judgment: false
  - id: D2
    description: "a missing prctl symbol (AttributeError from the ctypes lookup) degrades the parent watchdog to the polling thread instead of aborting the scheduler, closing gap G-01-9 / review finding IN-01"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parent_watchdog.py#test_prctl_failure_degrades_to_polling[prctl-missing]"
        status: pass
      - kind: integration
        ref: "python/tests/test_gpu_check_script.py (full suite) + scripts/check_all.sh --offline"
        status: pass
    human_judgment: false

duration: 15min (active work; wall-clock span was longer due to two mid-execution interruptions, see Issues Encountered)
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 13: Rust-mode shell-mode ordering and prctl degrade Summary

**Rust mode now parses upstream args once and checks the authoritative `run_shell` flag before resolving the `rsg-server` binary, and the parent watchdog's `prctl` setup now catches a missing libc symbol (`AttributeError`) the same way it catches an `OSError`, so both reported gaps (G-01-8/WR-01 and G-01-9/IN-01) from the 2026-10-04 incremental review are closed.**

## Performance

- **Duration:** ~15 min of active executor work, split across three dispatches (see Issues Encountered)
- **Started:** 2026-10-04T11:16:30-07:00 (first RED commit)
- **Completed:** 2026-10-04T20:35:51-07:00 (disposition-fix commit); this SUMMARY committed immediately after
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- `run_rust_mode` in `python/rsglang/launch.py` now runs `server_args, run_shell = parse_args(rest)` and checks `run_shell` before `resolve_rust_bin`, so `--shell`, `--shell-m` and `--shell-mode` all report "--shell-mode is not supported with --frontend rust" even when no `rsg-server` binary has been built yet. `_run_rust_mode` now takes the already-parsed `ServerArgs` instead of the raw argv list, parsing upstream args exactly once.
- `start_parent_watchdog` in `python/rsglang/backend.py` now catches `except (OSError, AttributeError) as exc:` around the `prctl` setup, so a libc that does not export the `prctl` symbol (observed on macOS as `AttributeError: dlsym(RTLD_DEFAULT, prctl): symbol not found`) logs the same "PDEATHSIG unavailable" line and degrades to the polling watchdog instead of crashing the scheduler process.
- `01-REVIEW-DISPOSITION.md`: WR-01 and IN-01 both recorded as `fixed`; frontmatter `open:` count dropped from 12 to 10.

## Task Commits

1. **Task 1: rust mode reports shell-mode rejection before missing binary (G-01-8/WR-01)** - RED `3513e27` (test), GREEN `23b6ac0` (feat)
2. **Task 2: parent watchdog degrades on a missing prctl symbol (G-01-9/IN-01)** - RED `06c0a54` (test), GREEN `f581841` (feat), disposition update `2241e53` (docs)

**Plan metadata:** committed immediately after this SUMMARY (docs: complete plan)

_Note: no REFACTOR commits — both GREEN implementations were already minimal._

## Files Created/Modified
- `python/rsglang/launch.py` - `run_rust_mode` parses upstream args and checks `run_shell` before resolving the binary; `_run_rust_mode` signature takes `ServerArgs` with a `TYPE_CHECKING` import
- `python/rsglang/backend.py` - `except (OSError, AttributeError) as exc:` around the `prctl` setup; docstring updated to mention the missing-symbol case
- `python/tests/test_launch_args.py` - `test_rust_mode_reports_shell_mode_before_missing_binary`, parametrized over `--shell`/`--shell-m`/`--shell-mode`
- `python/tests/test_parent_watchdog.py` - `test_prctl_failure_degrades_to_polling` extended with a `prctl-missing` parametrize case
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` - WR-01 and IN-01 rows and frontmatter entries marked `fixed`, `open:` now 10

## Decisions Made
See `key-decisions` in frontmatter above: (1) kept the literal `"--shell-mode" in rest` fast-path pre-check and added the authoritative `parse_args`/`run_shell` check right after it, rather than replacing the pre-check; (2) did not thread `run_shell` into `_run_rust_mode` since it is always false by the time that function runs.

## Deviations from Plan

None in substance — both fixes match the plan's GREEN instructions exactly. Process note, not a plan deviation: this plan's execution spanned three separate executor dispatches into the same git worktree. The first was stopped by the user partway through Task 1's GREEN implementation (an uncommitted edit was left in `launch.py`). The second dispatch verified that edit matched the plan exactly, committed Task 1's GREEN, completed Task 2's RED and GREEN, but then stalled (stream-watchdog timeout) during final verification. The third dispatch re-verified git state (nothing lost — each prior stopping point left a clean commit or a correct-but-uncommitted diff), ran the full test and acceptance-criteria verification, and wrote this SUMMARY.

## Issues Encountered

Two mid-execution interruptions (one user-initiated stop, one agent stall during a long-running shell verification) required spawning fresh continuation agents into the same worktree rather than resuming the original agent sessions. Each continuation re-verified the actual git log and diff before acting, rather than trusting prior narration — no work was lost or duplicated as a result. No code-level issues were encountered; all tests passed on the first attempt in each case.

## Next Phase Readiness

This closes the last known open gap in Phase 01 (01-UAT.md tests 8 and 9; review findings WR-01 and IN-01). `01-REVIEW-DISPOSITION.md` frontmatter `open:` is now 10. `scripts/check_all.sh --offline` passes in full (cargo tests, pytest, fixture freshness, WIRE-02 decode, check_upstream.py). Ready for phase 01 re-verification.

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*
