---
phase: 01-vendored-base-wire-codec
plan: 09
subsystem: gpu-verification
tags: [bash, pytest, gpu-orphan-check, tdd, gap-closure]

requires:
  - phase: 01-vendored-base-wire-codec
    provides: scripts/gpu_phase1_check.sh steps 4/4b (D-12 no-orphan backstop) from 01-07/01-08
provides:
  - "on_gpu <pid> helper: captures nvidia-smi's output once and returns 0 listed / 1 not listed / 2 nvidia-smi failed"
  - "wait_no_orphans <timeout_s> <pid>... helper used by steps 4 (30s) and 4b (120s), replacing the piped per-pid judgment"
  - "A polling start_session that waits up to 5s for the setsid exec and kills+reaps the pid it started on failure"
  - "A source guard in scripts/gpu_phase1_check.sh so Mac tests can source the script without running the preflight/build/GPU steps"
  - "python/tests/test_gpu_check_script.py: 11 slow tests with stubbed nvidia-smi and setsid executables"
  - "WR-07 and WR-08 recorded fixed in 01-REVIEW-DISPOSITION.md"
affects: [phase-01-gpu-verification, phase-07-benchmark-harness]

actuals:
  tokens: 4048
  tasks: 2
  commits: 5
  plan_head_before: 3d7eab8be3a61d8a0159865819726ecb93a186a7
  plan_head_after: 054e9a4f183df190e92bc33350487df658f8b4d6

tech-stack:
  added: []
  patterns:
    - "Bash helper functions tested on the Mac by sourcing the script behind a BASH_SOURCE[0]-vs-$0 guard, with stub executables (nvidia-smi, setsid) placed first on PATH for a single bash subprocess"
    - "Capture-then-grep instead of pipe-then-grep for nvidia-smi output, to avoid SIGPIPE-driven PIPESTATUS under `set -o pipefail` when the listing is long"
    - "Bounded poll (N rounds x sleep 0.1s) instead of a fixed sleep, for a startup race that depends on interpreter/exec speed"

key-files:
  created:
    - python/tests/test_gpu_check_script.py
  modified:
    - scripts/gpu_phase1_check.sh
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md

key-decisions:
  - "on_gpu returns a 3-way status (0 listed / 1 not listed / 2 nvidia-smi failed) rather than a 2-way boolean, so wait_no_orphans can distinguish 'confirmed gone' from 'cannot confirm' and fail loudly on the latter (D-12 requires the backstop to fail loudly, not pass silently)"
  - "wait_no_orphans owns both the polling loop and the final per-pid judgment that step4/step4_early used to inline, so the fix lives in one helper instead of being duplicated across both call sites"
  - "start_session's failure-path cleanup signals only the started pid, never its process group, because at that moment the pid still shares the script's own process group"

patterns-established:
  - "Mac-testable bash helpers: a source guard plus stub executables on PATH is the established way to unit-test scripts/gpu_phase1_check.sh helpers without a GPU box"

requirements-completed: [BASE-02, BASE-03]

coverage:
  - id: D1
    description: "on_gpu/wait_no_orphans fail the GPU-orphan check on an nvidia-smi error and detect a listed pid regardless of listing length (WR-07)"
    requirement: "BASE-02"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_check_script.py::test_on_gpu_reports_nvidia_smi_failure, test_on_gpu_detects_pid_in_long_listing, test_on_gpu_reports_absent_pid, test_wait_no_orphans_fails_when_nvidia_smi_fails, test_wait_no_orphans_fails_when_exited_pid_is_still_listed, test_wait_no_orphans_passes_when_pids_are_gone_and_unlisted, test_wait_no_orphans_fails_while_pid_runs"
        status: pass
    human_judgment: false
  - id: D2
    description: "start_session polls up to 5s for the setsid exec and kills+reaps the pid it started on failure, so step4_early never leaves a session running after that failure (WR-08)"
    requirement: "BASE-03"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_check_script.py::test_start_session_waits_for_slow_setsid, test_start_session_kills_what_it_started_when_setsid_never_detaches, test_start_session_fast_path, test_start_session_leaves_an_exited_process_to_the_caller"
        status: pass
    human_judgment: false
  - id: D3
    description: "01-REVIEW-DISPOSITION.md records WR-07 and WR-08 as fixed"
    verification:
      - kind: automated_ui
        ref: "grep '| WR-07 | warning | fixed |' and '| WR-08 | warning | fixed |' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md"
        status: pass
    human_judgment: false
  - id: D4
    description: "The real GPU box confirms steps 4/4b still PASS end-to-end with the new helpers (UAT test 1, non-blocking human-check)"
    verification: []
    human_judgment: true
    rationale: "Requires real nvidia-smi and a GPU-loaded rsg-server/scheduler process, unavailable on this Mac dev machine; the collaborator's Linux GPU run is the authoritative check per the plan's human-check instructions"

duration: 18min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 09: GPU-Orphan Check Hardening (WR-07, WR-08) Summary

**`on_gpu`/`wait_no_orphans` make the D-12 orphan backstop fail loudly on an nvidia-smi error and detect any-length GPU listings instead of silently passing (WR-07); `start_session` polls for up to 5s for a slow `setsid` exec and kills+reaps the pid it started on timeout instead of leaving it running (WR-08) — both proven on the Mac with 11 stubbed-executable tests.**

## Performance
- **Duration:** ~18min
- **Started:** 2026-10-04T07:18:00Z (approx.)
- **Completed:** 2026-10-04T07:35:19Z
- **Tasks:** 2 completed
- **Files modified:** 3 (1 created, 2 modified)

## Accomplishments
- Closed G-01-7-WR07: `gpu_pids` no longer discards nvidia-smi's stderr; `on_gpu <pid>` captures its output once and greps the captured text (0 listed / 1 not listed / 2 nvidia-smi failed), so neither a failing nvidia-smi nor a long listing (which SIGPIPE'd the old `gpu_pids | grep -qx` form under `pipefail`, PIPESTATUS 141) can report a listed pid absent.
- Closed G-01-7-WR08: `start_session` replaced its fixed half-second wait with a bounded poll (50 x 0.1s = 5s) for the `setsid` exec, and on timeout kills and reaps the pid it started (never a process group) before returning 1, so `step4_early`'s existing `start_session ... || return 1` leaves nothing running.
- `step4` and `step4_early` now judge orphans only through `wait_no_orphans` (30s and 120s respectively); the piped per-pid checks and their duplicated judgment logic are gone from both call sites.
- Added a source guard (`BASH_SOURCE[0]` vs `$0`) so `python/tests/test_gpu_check_script.py` can source the script to reach the helpers without running the preflight, build or any GPU step.
- `python/tests/test_gpu_check_script.py`: 11 slow tests with stub `nvidia-smi` and `setsid` executables generated per-test in `tmp_path`, all passing.
- `01-REVIEW-DISPOSITION.md`: WR-07 and WR-08 recorded `fixed`; `open` count dropped from 19 to 17.
- `bash scripts/check_all.sh --offline` (the full Phase 1 gate: cargo test --workspace, pytest python/tests, fixture freshness, WIRE-02 decode, check_upstream.py) prints `check_all: OK`.

## Task Commits
1. **Task 1: Tracer — source guard, on_gpu/wait_no_orphans, stubbed-nvidia-smi tests**
   - `efb181e` (test): add failing tests for on_gpu/wait_no_orphans + source guard (RED: 7 tests fail, "command not found", exit 127)
   - `c349b33` (feat): add on_gpu/wait_no_orphans, fail loudly on nvidia-smi error; rewire step4/step4_early (GREEN: 7 tests pass)
2. **Task 2: start_session polls for the setsid exec; record WR-07/WR-08 fixed**
   - `aa8f3bd` (test): add failing tests for start_session's setsid poll (RED: 2 of 4 new tests fail on the fixed half-second wait)
   - `1497983` (feat): poll start_session for the setsid exec, clean up on failure (GREEN: all 11 tests pass)
   - `054e9a4` (docs): record WR-07 and WR-08 as fixed in 01-REVIEW-DISPOSITION.md

**Plan metadata:** committed together with this SUMMARY (see git_commit_metadata step)

_No REFACTOR commits: both GREEN implementations were already minimal; no cleanup was needed._

## Files Created/Modified
- `scripts/gpu_phase1_check.sh` — source guard; `gpu_pids` without the stderr redirect; new `on_gpu`/`wait_no_orphans` helpers; `step4`/`step4_early` rewired to call `wait_no_orphans`; `start_session` rewritten to poll for the setsid exec and clean up on failure.
- `python/tests/test_gpu_check_script.py` (new) — 11 tests: 7 for `on_gpu`/`wait_no_orphans`, 4 for `start_session`, using stub `nvidia-smi`/`setsid` executables on PATH.
- `.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` — WR-07 and WR-08 rows and frontmatter entries changed to `fixed`; `open: 19` → `open: 17`.

## RED Evidence (recorded per the plan's acceptance criteria)

**Scratch evidence against the unmodified `gpu_pids | grep -qx` form (before any helper existed), 5 runs each:**

Long-listing stub (prints the pid, then 200000 filler lines), reproducing the WR-07 SIGPIPE false-absent:
```
RUN1: absent (PIPESTATUS=141 0)
RUN2: absent (PIPESTATUS=141 0)
RUN3: absent (PIPESTATUS=141 0)
RUN4: absent (PIPESTATUS=141 0)
RUN5: absent (PIPESTATUS=141 0)
```

Failing nvidia-smi stub (exits 9, writes to stderr), reproducing the WR-07 false-absent-on-error:
```
RUN1: absent (PIPESTATUS=9 1)
RUN2: absent (PIPESTATUS=9 1)
RUN3: absent (PIPESTATUS=9 1)
RUN4: absent (PIPESTATUS=9 1)
RUN5: absent (PIPESTATUS=9 1)
```

**Task 1 test RED** (before `on_gpu`/`wait_no_orphans` existed): all 7 new tests failed with `bash: line N: on_gpu: command not found` / `wait_no_orphans: command not found` (exit 127), the intentional failure for the exact missing helper — not an INVALID_RED (no syntax error, no zero-test discovery, no unrelated failures).

**Task 2 test RED** (before `start_session`'s poll existed), 2 of 4 new tests failed on the current fixed half-second wait:
- `test_start_session_waits_for_slow_setsid` (1.5s setsid delay): `rc=1`, stderr `setsid did not exec in place (pid 76562, pgid 76556)` — the WR-08 false FAIL.
- `test_start_session_kills_what_it_started_when_setsid_never_detaches`: `rc=1` (correct), but stdout printed `alive` instead of `dead` — the started pid was left running, which `step4_early`'s early return would otherwise leave until the EXIT trap.

## Decisions Made
See `key-decisions` in frontmatter.

## Deviations from Plan
None - plan executed exactly as written. The precondition (`.venv/bin/python` importing `rsglang`) was unmet at task start because this worktree had no `.venv`; the precondition text itself names the remedy (`bash scripts/bootstrap_mac_env.sh`, "idempotent, worktree-safe"), which was run once before Task 1 and is not a deviation from the plan.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required. The non-blocking human-check in Task 2's `<verify>` (running `bash scripts/gpu_phase1_check.sh` on the real GPU box and confirming steps 4/4b still PASS) is deferred to the collaborator's later Linux GPU run, per the plan's own instructions (UAT test 1); it does not block this plan's completion.

## Next Phase Readiness
G-01-7-WR07 and G-01-7-WR08 are closed. Steps 4 and 4b of `scripts/gpu_phase1_check.sh` can no longer report a false PASS (nvidia-smi error) or a false FAIL (slow setsid exec), and a failed `start_session` never leaves a half-started GPU-loading run behind. Ready for 01-10 (next gap closure plan in the chain).

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED
All created/modified files found on disk (scripts/gpu_phase1_check.sh, python/tests/test_gpu_check_script.py, 01-REVIEW-DISPOSITION.md, 01-09-SUMMARY.md). All 6 commits confirmed in `git log` (efb181e, c349b33, aa8f3bd, 1497983, 054e9a4, 7050fc9).
