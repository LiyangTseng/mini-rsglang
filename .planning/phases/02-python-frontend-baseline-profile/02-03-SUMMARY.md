---
phase: 02-python-frontend-baseline-profile
plan: 03
subsystem: profiling
tags: [py-spy, psutil, process-discovery, role-identification, json-sidecar, process-teardown]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 02
    provides: "rsglang.profiling.hook: env-gated gc.callbacks + tracemalloc shim, hook_env()/write_shim()/load_hook_records() contract"
provides:
  - "rsglang.profiling.procs: race-free server launch/wait_ready/discover_children, py-spy role identification (classify_dump/identify_roles), process-group teardown, tree_memory"
  - "rsglang.profiling.sidecar: schema_version 1 constants, build_meta provenance, validate_sidecar, an atomic allow_nan=False writer"
  - "rsglang.testing.fake_profile_env: Mac stand-ins for the server (exec hop, scheduler/tokenizer spawn topology, /v1/models + streaming chat) and for py-spy (--version/dump/record)"
  - "scripts/baseline_profile.py discover: the end-to-end driver wiring all of the above together, proven on the Mac against the stand-in tree"
affects: [02-04, 02-05, 02-06, 02-07, 02-08, docs/benchmarks/baseline-profile]
actuals:
  tokens: 12400
  tasks: 2
  commits: 1
  plan_head_before: fea3e8aa482b0f6241585d40a287a1b087dbee2e
  plan_head_after: 9edb9592c7e96050f0294ed9a8b612506f9e5ba1
tech-stack:
  added: []
  patterns:
    - "Race-free PID discovery: launch the server from inside the driver, poll GET /v1/models until 200, then enumerate psutil.Process(top_pid).children() -- by the time /v1/models answers, start_backend() has every child's ack, so the tree is stable"
    - "Role identification via one-shot py-spy dump, never PID order: grep each child's dump for _run_scheduler vs tokenize_worker; a dump containing both markers is an error, not a guess"
    - "SIGINT-then-SIGKILL process-group teardown in a finally block on every discover exit path, verified by polling psutil.pid_exists/zombie status rather than trusting the signal alone"
    - "Validated-before-written, atomically-replaced JSON sidecar: validate_sidecar() must return [] before write_sidecar() ever calls os.replace()"
key-files:
  created:
    - python/rsglang/profiling/procs.py
    - python/rsglang/profiling/sidecar.py
    - python/rsglang/testing/fake_profile_env.py
    - scripts/baseline_profile.py
    - python/tests/test_baseline_profile.py
  modified: []
key-decisions:
  - "Built tree_memory() and the complete discover exit-code/finally-teardown mapping together with Task 1's tracer, in the same commit, rather than deferring them to a separate Task 2 RED/GREEN pass. The tracer's single end-to-end slice needed the full procs.py/scripts/baseline_profile.py contract (every exit path, every teardown case) to prove the pipeline honestly rather than against a partial stub. Task 2's seven behaviors (test_role_identification, test_server_argv, test_tree_memory_current_process, and the three failure-exit-code tests) were therefore already written and already passing by the time Task 2 began; Task 2 became a verification-only pass with no new diff to commit. Documented as a deviation below rather than silently treated as 'done'."
  - "py_spy_dump's permission-error remediation message uses uppercase CAP_SYS_PTRACE (matching the real capability name and the test's exact string match), even though some setcap examples in the wild use lowercase -- this is the string test_discover_permission_denied_exits_2 checks for."
patterns-established:
  - "fake_profile_env.py's spawn targets (_run_scheduler, tokenize_worker) are module-level functions in the file that is itself run via `python -m`, with all side effects gated under `if __name__ == \"__main__\":` -- the same os.execv/__mp_main__ convention already established in rsglang/launch.py, now proven to also work for a two-child default-topology spawn tree."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "discover launches the stand-in server tree, race-free-discovers its two children, py-spy role-identifies them, confirms the 02-02 hook activated in every role, writes a validated discover JSON, and leaves no process behind"
    requirement: "BENCH-01"
    verification:
      - kind: e2e
        ref: "python/tests/test_baseline_profile.py::test_discover_end_to_end"
        status: pass
    human_judgment: false
  - id: D2
    description: "Role identification requires exactly one scheduler and one tokenizer dump; zero, two-plus, or a dump containing both markers is an error naming the default topology, never a guess"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_baseline_profile.py::test_role_identification"
        status: pass
    human_judgment: false
  - id: D3
    description: "server_argv builds the default launch command or shlex.splits a {python}/{model}/{port} template"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_baseline_profile.py::test_server_argv"
        status: pass
    human_judgment: false
  - id: D4
    description: "tree_memory sums integer RSS (and Linux-only PSS) across the top process and its descendants, counting a vanished process in vanished instead of raising"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_baseline_profile.py::test_tree_memory_current_process"
        status: pass
    human_judgment: false
  - id: D5
    description: "discover exits 2 before launching anything when py-spy is missing from PATH"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_discover_missing_py_spy_exits_2"
        status: pass
    human_judgment: false
  - id: D6
    description: "discover exits 2 with CAP_SYS_PTRACE / --py-spy-sudo remediation on a py-spy permission error, and no stand-in process survives"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_discover_permission_denied_exits_2"
        status: pass
    human_judgment: false
  - id: D7
    description: "discover exits 1 with the server's log tail when the server process exits before becoming ready"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_discover_server_exits_early"
        status: pass
    human_judgment: false
duration: 35min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 3: Tracer -- Launch, Role-ID, Hook Confirmation, Validated Sidecar Summary

**`scripts/baseline_profile.py discover` proves the whole discovery and instrumentation pipeline end to end on the Mac against a stand-in server tree that mimics minisgl's exact spawn topology -- race-free launch, py-spy role identification, 02-02 hook activation confirmation, a validated atomic JSON sidecar, and clean process-group teardown -- before any scenario driver, analysis or reporting is built.**

## Performance
- **Duration:** ~35min
- **Started:** 2026-10-05 (session start, environment bootstrap)
- **Completed:** 2026-10-05T17:25:35-07:00 (Task 1 commit; Task 2 required no new commit)
- **Tasks:** 2/2 completed (Task 2 verification-only; see Deviations)
- **Files modified:** 5 (all newly created)

## Accomplishments
- Built the complete `rsglang.profiling.procs` module: race-free `launch_server`/`wait_ready`/`discover_children` (Pattern 1), py-spy role identification via `classify_dump`/`identify_roles` (Pattern 2), `py_spy_base`/`py_spy_version`/`py_spy_dump` with actionable permission-error remediation, `teardown` (SIGINT then SIGKILL on the process group, verified alive-check), and `tree_memory` (RSS always, PSS Linux-only).
- Built `rsglang.profiling.sidecar`: `build_meta` provenance (git commit/dirty, upstream SHA, GPU name, py-spy version, clock implementation), `validate_sidecar` (schema, discover-mode discovery block, NaN/Infinity rejection anywhere in the document), and `write_sidecar` (validate-then-atomic-`os.replace`).
- Built `rsglang.testing.fake_profile_env`: a Mac stand-in server that mimics minisgl's exec hop, two-child spawn topology (`_run_scheduler`/`tokenize_worker`, both doing real `gc.collect()` work so the hook has something to observe), and the exact `/v1/models`/streaming-chat-completions wire shapes; plus a stand-in `py-spy` CLI (`--version`, `dump`, `record`) whose dumps and speedscope output are keyed off a role-map file so later plans' radix-bucketing tests have a real fixture to target.
- Wired `scripts/baseline_profile.py discover` end to end: py-spy preflight before anything launches, hook shim injection, launch, race-free readiness wait, role identification, a settle period, hook-record loading, hook-activation/gc-count extraction, validated sidecar write, and `teardown` in a `finally` block on every path with the documented exit-code contract (0 OK, 1 measurement failure, 2 environment error).
- All 7 tests in `python/tests/test_baseline_profile.py` pass, including the 3 that run standalone under `-m "not slow"`; the project's full `-m "not slow"` suite shows no regression (68 passed, 36 skipped). No orphaned `fake_profile_env` process observed in `ps aux` after any test run, including repeated runs of the end-to-end test.
- `git status --porcelain -- vendor/mini-sglang` stayed empty throughout.

## Task Commits
1. **Task 1: Tracer -- discover launches, role-identifies, confirms the hook and writes a validated JSON**
   - `9edb959` feat(02-03): tracer -- discover launches, role-identifies, confirms the hook and writes a validated JSON
2. **Task 2: Role-ID edge cases, failure exits, tree_memory, and no leftovers on every path**
   - No new commit -- see Deviations. All seven of Task 2's behaviors (`test_role_identification`, `test_server_argv`, `test_tree_memory_current_process`, and the three failure-exit-code tests) were already implemented and passing as part of Task 1's commit above.

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md, written by the worktree executor per its orchestrator contract)

## Files Created/Modified
- `python/rsglang/profiling/procs.py` - server launch/readiness/discovery, py-spy role identification, process-group teardown, tree_memory
- `python/rsglang/profiling/sidecar.py` - schema constants, provenance metadata, validation, atomic writer
- `python/rsglang/testing/fake_profile_env.py` - Mac-only server and py-spy stand-ins
- `scripts/baseline_profile.py` - the `discover` subcommand driver
- `python/tests/test_baseline_profile.py` - all 7 tests for this plan

## Decisions Made
- **Tree_memory and the full exit-code/finally-teardown mapping were built together with Task 1, not deferred to Task 2.** The plan's interfaces block lists `tree_memory` as part of `procs.py`'s public contract, and Task 1's own `<action>` text already fully specifies `teardown()` and the discover command's complete exit-code contract (0/1/2, including the py-spy-missing/permission/role-error/hook-inactive/still-alive cases). Building the complete module in one pass was necessary for the tracer to prove the pipeline honestly end to end, including its failure paths, rather than against a partial stub. See Deviations for the TDD-discipline consequence on Task 2.
- **`CAP_SYS_PTRACE` uses the real, uppercase capability name** in `py_spy_dump`'s permission-error remediation message, matching what `test_discover_permission_denied_exits_2` checks for and what Linux capability documentation conventionally uses.

## Deviations from Plan

### Auto-fixed Issues

None - no bugs found requiring Rule 1/2/3 auto-fixes during implementation.

### Process Deviation (TDD discipline, flagged per tdd.md fail-fast rule 3)

**[Process] Task 2's RED/GREEN split collapsed into a single Task 1 commit**
- **Found during:** Start of Task 2
- **Issue:** Task 2 carries `tdd="true"` and specifies a RED-then-GREEN flow: write `test_role_identification`, `test_server_argv`, `test_tree_memory_current_process`, and three failure-exit-code tests first (expecting them to fail against a procs.py/scripts/baseline_profile.py that doesn't yet have `tree_memory` or the full exit-code mapping), then implement to GREEN. Because Task 1's own `<action>` text already fully specified `teardown()` and the discover command's complete exit-code contract, and because `tree_memory` was listed in the plan's interfaces block as part of `procs.py`'s contract, the Task 1 implementation (needed for the tracer to prove its own failure paths honestly) already included every function and every exit-code branch Task 2's tests check. All 7 tests in `test_baseline_profile.py` were written and already passing in Task 1's single commit.
- **Resolution:** No separate RED commit exists for Task 2's behaviors; there is no missing-RED violation in the sense of "implementation predates a failing test that was supposed to drive it," because the tests and implementation landed together, not implementation-then-tests-added-later. Re-ran Task 2's exact `<verify>` (`pytest python/tests/test_baseline_profile.py -q`, 7 passed) and all four `<acceptance_criteria>` commands after the fact; all pass. No production-code change was needed for Task 2 — it is a verification-only pass with no new diff to commit.
- **Files involved:** `python/rsglang/profiling/procs.py`, `scripts/baseline_profile.py`, `python/tests/test_baseline_profile.py` (all committed in `9edb959`)
- **Verification:** `.venv/bin/python -m pytest python/tests/test_baseline_profile.py -q` → `7 passed`; `-m "not slow"` → `3 passed, 4 deselected`; `.venv/bin/python -m pytest python/tests -q -m "not slow"` → `68 passed, 36 skipped`; `grep` confirms `def tree_memory(`, `children(recursive=True)`, `memory_full_info()`, `NoSuchProcess` all present in `procs.py`.
- **Commit:** `9edb959` (no new commit for Task 2)

**Total deviations:** 1 process deviation (TDD RED/GREEN split collapsed by necessity), 0 auto-fixed bugs. **Impact:** none on correctness or test coverage -- every behavior Task 2 specifies is implemented and independently verified to pass; the only difference from the plan is commit granularity.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh` per Task 1's own instructions; it created `.venv` from the already-psutil/aiohttp-pinned lock (02-01's prior work) and installed `rsglang`/`minisgl` in editable mode. The environment check passed on the first run.
- `py-spy` and `hyperfine` are correctly absent from the Mac's PATH (GPU-box-only binaries); this is expected and is exactly why `fake_profile_env.py`'s `py-spy` stand-in exists.

## User Setup Required
None - no external service configuration required. The real `py-spy` binary (needed by the GPU-box run) is out of scope for this Mac-side plan.

## Next Phase Readiness
- Plans 02-04/02-05/02-06/02-07/02-08 can now import `rsglang.profiling.procs` (`server_argv`, `launch_server`, `wait_ready`, `discover_children`, `py_spy_base`/`py_spy_version`/`py_spy_dump`, `classify_dump`, `identify_roles`, `teardown`, `tree_memory`) and `rsglang.profiling.sidecar` (`SCHEMA_VERSION`, `GENERATED_BY`, `ROLES`, `SCENARIOS`, `CANONICAL_OUT`, `SidecarError`, `build_meta`, `validate_sidecar`, `write_sidecar`) using exactly the names and shapes fixed in this plan's interfaces contract.
- `rsglang.testing.fake_profile_env`'s `py-spy record` stand-in already emits role-conditioned speedscope output (`match_prefix` for the scheduler, `put`/`serialize_type`/`tokenize` for api_server/tokenizer) per the plan's exact sample counts, so 02-05's radix-share bucketing tests have a real, deterministic fixture to target without waiting for the GPU box.
- No blockers identified for downstream plans in this phase.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED
All 5 created files found on disk; Task 1 commit `9edb959` found in git log.
