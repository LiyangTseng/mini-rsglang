---
phase: 02-python-frontend-baseline-profile
plan: 07
subsystem: profiling
tags: [bash, gpu-runbook, py-spy, hyperfine, preflight, least-privilege]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 03
    provides: "scripts/baseline_profile.py discover subcommand (exit codes 0/1/2, --model/--port/--timeout/--out/--py-spy-sudo flags)"
provides:
  - "scripts/gpu_phase2_profile.sh: the one command a human runs on the GPU box for the Phase 2 profiling run -- preflight, discover, run, validate --require-gpu, check_upstream.py, PASS/FAIL summary"
  - "version_ge/hyperfine_ok/pyspy_can_attach bash helpers, independently testable via the source guard"
affects: ["02-09"]
actuals: {tokens: 4100, tasks: 2, commits: 3}
tech-stack:
  added: []
  patterns:
    - "Preflight-then-numbered-steps-then-summary (gpu_phase1_check.sh shape): PATH/import/version/privilege checks fail loud before anything launches; each of the 4 steps records PASS/FAIL; a failed early step skips later steps as 'skipped: X failed' instead of wasting GPU time"
    - "Disposable-sleeper privilege probe: pyspy_can_attach starts a throwaway python sleep(30) process, dumps it with py-spy, and always kills+waits it in both the success and failure path -- proving the privilege check itself leaves nothing running"
key-files:
  created:
    - scripts/gpu_phase2_profile.sh
    - python/tests/test_gpu_profile_script.py
  modified: []
key-decisions:
  - "All 7 tests (both Task 1's 5 and Task 2's 2) were written in a single RED commit before any implementation, rather than splitting Task 2's tests into a separate later RED pass. test_steps_reference_driver (a pure text-match test) was therefore already satisfied by Task 1's usage() heredoc -- which legitimately documents all four driver subcommands for --help output -- before Task 2's real step-invocation code existed. This is a text-coincidence, not a hidden implementation gap: test_preflight_missing_tool_fails_before_launch (Task 2's functional test) correctly stayed RED until Task 2's preflight code was written. Documented as a deviation below per tdd.md's fail-fast rule 1 (unexpected GREEN)."
  - "pyspy_can_attach always kills and waits its probe sleeper in both the success and failure branch (not just on failure), so a crash or signal mid-probe can never leave an orphaned python process on the GPU box."
patterns-established:
  - "version_ge compares dotted version strings field-by-field as bash integers (missing fields default to 0), avoiding a dependency on sort -V or python for a single comparison the wrapper needs before any venv exists."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "Wrapper CLI and preflight helpers (--help, unknown-arg exit 2, version_ge, hyperfine_ok, pyspy_can_attach) behave correctly on the Mac via stand-ins"
    requirement: "BENCH-01"
    verification:
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_help_anywhere", status: pass}
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_unknown_arg_exits_2", status: pass}
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_version_ge", status: pass}
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_hyperfine_ok", status: pass}
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_pyspy_can_attach_ok_and_denied", status: pass}
    human_judgment: false
  - id: D2
    description: "Preflight fails before any launch when a required tool is missing, and the script's text correctly references the driver subcommands it will invoke on the GPU box"
    requirement: "BENCH-01"
    verification:
      - {kind: integration, ref: "python/tests/test_gpu_profile_script.py::test_preflight_missing_tool_fails_before_launch", status: pass}
      - {kind: unit, ref: "python/tests/test_gpu_profile_script.py::test_steps_reference_driver", status: pass}
    human_judgment: false
  - id: D3
    description: "Privilege-cleanup reminder text and the real GPU-box preflight/step behavior (nvidia-smi, real py-spy attach, real hyperfine, the actual run) -- the plan's <human-check> item on Task 2"
    verification: []
    human_judgment: true
    rationale: "Can only be confirmed on the actual GPU box, reviewed alongside plan 02-09's real profiling run; everything else about Task 2 has passing automated tests on the Mac against stand-ins"
duration: 25min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 7: GPU Runbook Wrapper (gpu_phase2_profile.sh) Summary

**`scripts/gpu_phase2_profile.sh` is the single preflighted command for the Phase 2 GPU profiling run: it fails loud before launching anything if nvidia-smi/curl/py-spy/hyperfine are missing, if hyperfine is older than 1.19.0, if psutil/aiohttp/openai/transformers don't import, or if py-spy can't attach to a sibling process (printing the three least-privilege remediation options) -- then runs discover, run, validate --require-gpu and check_upstream.py in sequence, skipping later steps on an early failure, and ends with a privilege-cleanup reminder plus a live getcap check.**

## Performance
- **Duration:** ~25min | **Started:** 2026-10-05 | **Completed:** 2026-10-05 | **Tasks:** 2/2 | **Files modified:** 2 (both newly created)

## Accomplishments
- Built the complete `scripts/gpu_phase2_profile.sh` wrapper in the `gpu_phase1_check.sh` shape: `usage()`/flag-parsing (`--model`, `--port`, `--timeout`, `--out`, `--py-spy-sudo`, `--help`), `ROOT`/`PYTHON`/`PYTHONPATH`/`LOG_DIR`/`record()` plumbing, and a bash-sourcing guard so Mac tests can exercise its pure-function helpers without any GPU, py-spy or hyperfine binary.
- Implemented `version_ge` (numeric dotted-version comparison, missing fields count as 0), `hyperfine_ok` (PATH + `>=1.19.0` check with an install hint), and `pyspy_can_attach` (probes a disposable sleeper process, always tears it down, prints the three remediation options — `setcap`, `--py-spy-sudo` + NOPASSWD, `ptrace_scope` — on denial).
- Implemented the preflight (tool PATH check with install hints, the Python import check, `hyperfine_ok`, `pyspy_can_attach`, each failing loud with `FAIL preflight: ...` before any launch) and the four numbered steps: 1 `discover` smoke, 2 the full `run` (writes the baseline profile JSON per D-14), 3 `validate --require-gpu`, 4 `check_upstream.py`. A failed `discover` skips steps 2 and 3 as `"skipped: discover failed"` rather than burning GPU time on a broken foundation.
- The summary block mirrors `gpu_phase1_check.sh`'s `ALL PASS` / `SOME STEPS FAILED` shape and, before exiting on either path, prints the privilege-cleanup reminder (`setcap -r`, `ptrace_scope` restore) and runs `getcap` against the venv's `py-spy` binary so the operator sees directly whether a grant is still active.
- All 7 tests in `python/tests/test_gpu_profile_script.py` pass; the project's `-m "not slow"` suite shows no regression (68 passed, 36 skipped).
- `grep -n 'sudo' scripts/gpu_phase2_profile.sh` confirms the only non-text `sudo` usage is the opt-in `sudo -n py-spy` attach; every other `sudo`/`setcap` mention is inside the `usage()` heredoc or an `echo` reminder line. `git status --porcelain -- vendor/mini-sglang` stayed empty throughout.

## Task Commits
1. **Task 1: Wrapper helpers and their Mac tests** — `969aad3` test(02-07): RED, `d21055e` feat(02-07): GREEN
2. **Task 2: Preflight and the four numbered steps with PASS/FAIL summary** — `028f638` feat(02-07)

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md)

## Files Created/Modified
- `scripts/gpu_phase2_profile.sh` - the GPU-box runbook wrapper (preflight, 4 steps, PASS/FAIL summary, privilege-cleanup reminder)
- `python/tests/test_gpu_profile_script.py` - all 7 Mac-runnable tests for the wrapper's CLI, helpers and preflight/driver-reference behavior

## Decisions Made
- **All 7 tests were written up front in one RED commit** covering both Task 1's helper/CLI behaviors and Task 2's preflight/driver-reference behaviors, rather than splitting Task 2's RED pass into a later commit. See Deviations for the one consequence (an early, benign GREEN on a text-match test).
- **`pyspy_can_attach` tears down its probe sleeper unconditionally** (both success and failure branches use the same `kill -9`/`wait` lines before the pass/fail decision), so a crash mid-probe cannot leave an orphan on the shared GPU box.
- **Preflight checks tool PATH before anything else**, including before the Python import check, so a missing `nvidia-smi` is reported without ever touching the venv — matching `test_preflight_missing_tool_fails_before_launch`'s expectation that no `"step 1"` line appears.

## Deviations from Plan

### Process Deviation (TDD discipline, flagged per tdd.md fail-fast rule 1)

**[Process] `test_steps_reference_driver` passed earlier than its implementing task**
- **Found during:** Start of Task 2, when confirming RED for Task 2's two added tests.
- **Issue:** `test_steps_reference_driver` just greps the script's text for `"baseline_profile.py discover"`, `"baseline_profile.py run"`, `"baseline_profile.py validate"`, `"--require-gpu"` and `"scripts/check_upstream.py"`. Task 1's `usage()` heredoc — written to satisfy `test_help_anywhere` and to document the four steps for a human reading `--help` — already contains every one of those exact tokens as part of its step-list prose. So this test was already GREEN immediately after Task 1's commit, before Task 2 wrote any step-invocation code.
- **Resolution:** Confirmed this is a text coincidence, not a masked implementation gap, by checking the plan's actual acceptance criterion for Task 2 is `test_preflight_missing_tool_fails_before_launch` (which did stay RED until Task 2's preflight code existed — verified above) plus the acceptance-criteria grep checks (also only satisfiable once Task 2's real step commands existed with those literal invocation strings, not just the help text — `grep -n 'baseline_profile.py discover'` matches both the heredoc *and* the real step-1 command line, but the step-1 command line is what Task 2 actually added). No production-code or test change was needed; `test_steps_reference_driver` continues to pass for the right structural reason (the script's text, now including the real step invocations, contains every required token) after Task 2's commit.
- **Files involved:** `python/tests/test_gpu_profile_script.py` (test added in the RED commit `969aad3`), `scripts/gpu_phase2_profile.sh` (usage() text in `d21055e`, real step invocations in `028f638`)
- **Verification:** `.venv/bin/python -m pytest python/tests/test_gpu_profile_script.py -q` → `7 passed` after both Task 1 and Task 2 commits; `test_preflight_missing_tool_fails_before_launch` independently confirmed RED-then-GREEN (RED: `0 == 1` exit-code mismatch before Task 2; GREEN: `7 passed` after).
- **Commit:** no corrective commit needed — behavior is correct, documented for transparency only.

**Total deviations:** 1 process deviation (an early, benign GREEN on a text-match test), 0 auto-fixed bugs. **Impact:** none on correctness or coverage.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored, per the parallel_execution briefing). Ran `bash scripts/bootstrap_mac_env.sh`; it completed cleanly and installed `rsglang`/`minisgl` in editable mode with all previously-approved pins (psutil 7.2.2, pyzmq 27.2.0, etc.).
- `py-spy` and `hyperfine` are correctly absent from the Mac's PATH — expected, since they are GPU-box-only binaries; `rsglang.testing.fake_profile_env`'s py-spy stand-in (built in plan 02-03) and simple bash stubs cover all Mac-side test needs for this plan.

## User Setup Required
None — no external service configuration required. The real `py-spy`/`hyperfine` binaries and GPU access needed for the actual profiling run are out of scope for this Mac-side plan; they are plan 02-09's job.

## Next Phase Readiness
- `scripts/gpu_phase2_profile.sh` is ready to run on the GPU box once plans 02-04 through 02-08 land `scripts/baseline_profile.py run`/`validate` (currently forward-referenced; this plan's own tests never invoke those subcommands for real, matching the parallel_execution briefing).
- Plan 02-09's human-check item (reviewing the Privileges paragraph and the end-of-run cleanup reminder together with a real GPU run) is flagged `human_judgment: true` above — everything else about this plan is machine-verified.
- No blockers identified for downstream plans in this phase.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED
Both created files found on disk; commits `969aad3`, `d21055e` and `028f638` found in git log.
