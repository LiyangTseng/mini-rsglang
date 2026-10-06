---
phase: 02-python-frontend-baseline-profile
plan: 08
subsystem: profiling
tags: [py-spy, run-orchestration, hyperfine, gc-correlation, radix-share, bench-01]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 02
    provides: "rsglang.profiling.hook: write_shim/hook_env/request_snapshot/load_hook_records, the gc/mem/alloc_top JSONL record contract"
  - phase: 02-python-frontend-baseline-profile
    plan: 03
    provides: "rsglang.profiling.procs: launch_server/wait_ready/discover_children/py_spy_dump/identify_roles/teardown/tree_memory; scripts/baseline_profile.py's discover flow and argparse scaffold"
  - phase: 02-python-frontend-baseline-profile
    plan: 04
    provides: "rsglang.profiling.sidecar: the full scenario-entry schema validator/writer, REQUIRED_BUCKETS, require_gpu gate"
  - phase: 02-python-frontend-baseline-profile
    plan: 05
    provides: "rsglang.profiling.analysis: load_speedscope, radix_share, cpu_metrics, gc_stats, gc_ttft_correlation, summarize_requests, memory_role_summary"
  - phase: 02-python-frontend-baseline-profile
    plan: 06
    provides: "rsglang.profiling.scenarios: run_s1/run_s2/run_first_request, hyperfine_argv/parse_hyperfine_json/coldstart_once/coldstart_stop/read_coldstart_records"
provides:
  - "rsglang.profiling.session: MeasurementError, PySpyRecorder, RssSampler, build_scenario_entry, run_session, run_coldstart -- the full per-scenario measurement orchestration"
  - "scripts/baseline_profile.py run: profiles s1/s2/s3 end to end and writes a validated BENCH-01 sidecar directly (D-14), with a GPU-only guard on the canonical docs/benchmarks/baseline-profile.json path (T-02-15)"
  - "scripts/baseline_profile.py coldstart-once/coldstart-stop: the hyperfine timed-command and --conclude subcommands scenario 3's cold start wraps"
  - "scripts/baseline_profile.py validate: schema-validates any sidecar file, optionally with the require_gpu gate"
  - "rsglang.testing.fake_profile_env hyperfine: a Mac stand-in for the real hyperfine binary"
affects: [02-09]
actuals:
  tokens: 11459
  tasks: 2
  commits: 4
  plan_head_before: 9e7fd71784476be22dd5b23f5c611f4c35c318e2
  plan_head_after: eddbf067496bd17532fb48f55e803a3851daaee3
tech-stack:
  added: []
  patterns:
    - "PySpyRecorder.stop_all() always signals and waits on every started recorder before it ever raises (collecting all per-recorder errors into one MeasurementError), so a missing/invalid speedscope file from one role never leaves a sibling recorder running (T-02-16)."
    - "run_session's authoritative hook_records load happens AFTER teardown (not reused from the mid-session allocation-snapshot poll), because hook.py's atexit-registered _final_flush only guarantees every GC/mem record is on disk once the process has actually received SIGINT and exited."
    - "Nested try/finally in run_session: an inner finally stops the RSS sampler and every py-spy recorder immediately if the workload itself raises, before the outer finally runs teardown -- so a failing workload never leaves orphaned py-spy subprocesses for teardown to miss (they aren't server-tree children, so teardown's killpg can't reach them)."
    - "run_coldstart's finally block calls scenarios.coldstart_stop only if the pgid file still exists after hyperfine exits, self-healing a server left running by a hyperfine crash without double-tearing-down a cleanly concluded one."
key-files:
  created:
    - python/rsglang/profiling/session.py
  modified:
    - scripts/baseline_profile.py
    - python/rsglang/testing/fake_profile_env.py
    - python/tests/test_baseline_profile.py
key-decisions:
  - "sidecar._gpu_name() (a leading-underscore helper) is called directly from scripts/baseline_profile.py's canonical-path guard rather than duplicated or re-exported, since build_meta() already performs the identical nvidia-smi probe internally and the guard needs the exact same GPU-presence signal before committing to a run."
  - "coldstart-once and coldstart-stop subcommands import only procs and scenarios at the call sites that matter (session/analysis/hook are only imported at module level alongside the other subcommands, never exercised by these two commands' own logic), keeping the work hyperfine's own timer measures as small as the plan's interfaces block intends."
  - "run_session's final hook-records load is a fresh load_hook_records() call after procs.teardown() returns, not a reuse of the record set read during the mid-workload allocation-snapshot poll -- the two reads can legitimately differ (teardown's SIGINT triggers hook.py's atexit _final_flush, writing any remaining buffered GC events) and only the post-teardown read is used to build the scenario entry."
patterns-established:
  - "scripts/baseline_profile.py's `run` subcommand is the single entry point phase 02-09's real GPU run invokes; its --scenarios/--server-cmd/--out contract is now fixed and the Mac stand-in-driven tests are the parity baseline for that GPU run."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "run profiles scenario 1 end to end against the stand-in: launches a fresh server tree, role-identifies children, runs 8 concurrent agents for 2s, samples active+gil py-spy passes and RSS for every role, snapshots allocations, and writes a schema-valid sidecar whose radix share, CPU/GIL fractions, IPC/tokenize bucket counts, GC counts and memory curves all trace back to the stand-in's known, fixed sample counts"
    requirement: "BENCH-01"
    verification:
      - kind: e2e
        ref: "python/tests/test_baseline_profile.py::test_run_s1_end_to_end"
        status: pass
    human_judgment: false
  - id: D2
    description: "run refuses to write the canonical docs/benchmarks/baseline-profile.json path (exit 2, before launching anything) unless it is on Linux with an nvidia-smi-visible GPU, leaving the canonical file byte-for-byte unchanged"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_run_refuses_canonical_out_off_gpu"
        status: pass
    human_judgment: false
  - id: D3
    description: "validate FILE exits 0 for a schema-valid sidecar and 1 (naming the offending field) for an invalid one; validate FILE --require-gpu additionally rejects a non-Linux/non-GPU document"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_validate_cli"
        status: pass
    human_judgment: false
  - id: D4
    description: "run profiles scenario 3 end to end: hyperfine times 2 runs (after 1 warmup) of coldstart-once to readiness, coldstart-stop samples whole-tree RSS/PSS and tears down after every iteration, then one instrumented first-request session with include_boot=True captures boot-window GC events and scheduler radix sampling in the same scenario entry's coldstart block"
    requirement: "BENCH-01"
    verification:
      - kind: e2e
        ref: "python/tests/test_baseline_profile.py::test_run_s3_end_to_end"
        status: pass
    human_judgment: false
  - id: D5
    description: "run --scenarios s3 exits 2 before launching any server when hyperfine is missing from PATH, with 'hyperfine' named in stderr"
    requirement: "BENCH-01"
    verification:
      - kind: integration
        ref: "python/tests/test_baseline_profile.py::test_run_s3_hyperfine_missing_exits_2"
        status: pass
    human_judgment: false
duration: 95min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 8: Run Orchestration (session.py, run/coldstart/validate subcommands) Summary

**`scripts/baseline_profile.py run` profiles all three BENCH-01 scenarios end to end on the Mac against the stand-ins -- fresh server tree per scenario, dual active+gil py-spy passes and RSS sampling for every role, hyperfine-timed cold start with whole-tree PSS, and boot-window GC capture for scenario 3 -- and writes a schema-valid sidecar directly, refusing the canonical GPU-only output path off a real GPU box.**

## Performance
- **Duration:** ~95min | **Started:** 2026-10-05 (session start, environment bootstrap) | **Completed:** 2026-10-05 | **Tasks:** 2/2 | **Files modified:** 4 (1 new, 3 modified)

## Accomplishments
- Built `rsglang.profiling.session` end to end: `MeasurementError`, `PySpyRecorder` (dual active+gil `--nonblocking` speedscope recording per role, every recorder stopped before ever raising), `RssSampler` (daemon-thread per-role RSS sampling), `build_scenario_entry` (assembles the complete 02-04 sidecar scenario-entry schema from raw py-spy/hook/request measurements), `run_session` (one fresh server tree per scenario: launch, role-ID, start recorders+sampler, run the workload, snapshot allocations, stop everything, teardown in a `finally` block, clock-mismatch detection), and `run_coldstart` (hyperfine-timed cold start via the coldstart-once/coldstart-stop subcommands, then one instrumented `include_boot=True` session).
- Extended `scripts/baseline_profile.py` with the `run` subcommand (every flag in the plan's interfaces block, s1/s2/s3 wired), the canonical-path GPU-only guard (refuses exit 2 before launching anything unless Linux + nvidia-smi), `coldstart-once`/`coldstart-stop` (the hyperfine timed-command and `--conclude` subcommands), and `validate` (schema-checks any sidecar file, with an optional `--require-gpu` gate).
- Added a `hyperfine` stand-in subcommand to `rsglang.testing.fake_profile_env`: runs the timed command and `--conclude` command through the shell for `warmup+runs` iterations and writes the real hyperfine JSON export shape (`mean`/`stddev`/`median`/`min`/`max`/`times`/`exit_codes`).
- All 12 tests in `python/tests/test_baseline_profile.py` pass, including the 5 new end-to-end/integration tests this plan adds. The full project suite (`pytest python/tests -q`, slow included) passes: 163 passed, 37 skipped, with no regression. No orphaned `fake_profile_env` process observed after any test run, including the s3 cold-start test's two full launch/teardown cycles. `git status --porcelain -- docs/benchmarks vendor/mini-sglang` stayed empty throughout.

## Task Commits
1. **Task 1: Session orchestration and `run` for scenarios 1 and 2, the canonical-path guard, and validate**
   - `1227840` test(02-08): RED -- run (s1/s2), canonical-path GPU guard, validate CLI
   - `c35508b` feat(02-08): session orchestration, run (s1/s2), canonical-path GPU guard, validate
2. **Task 2: Scenario 3 -- hyperfine cold start plus instrumented first-request session; coldstart subcommands; hyperfine stand-in**
   - `c0f0b53` test(02-08): RED -- scenario 3 (hyperfine cold start + instrumented session)
   - `eddbf06` feat(02-08): scenario 3 -- hyperfine cold start + instrumented session

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md, committed next)

## Files Created/Modified
- `python/rsglang/profiling/session.py` - the full per-scenario measurement orchestration: `MeasurementError`, `PySpyRecorder`, `RssSampler`, `build_scenario_entry`, `run_session`, `run_coldstart`
- `scripts/baseline_profile.py` - `run`, `coldstart-once`, `coldstart-stop` and `validate` subcommands added to the existing `discover` scaffold
- `python/rsglang/testing/fake_profile_env.py` - added the `hyperfine` stand-in subcommand
- `python/tests/test_baseline_profile.py` - 5 new tests: `test_run_s1_end_to_end`, `test_run_refuses_canonical_out_off_gpu`, `test_validate_cli`, `test_run_s3_end_to_end`, `test_run_s3_hyperfine_missing_exits_2`, plus a `_write_hyperfine_stub` helper and a `_run_cli` helper mirroring the existing `_run_discover` pattern

## Decisions Made
- **`sidecar._gpu_name()` is called directly from the canonical-path guard** rather than duplicated or re-exported as a public helper -- `build_meta()` already performs the identical `nvidia-smi` probe internally, and the guard needs the exact same GPU-presence signal before the run commits to launching anything.
- **`run_session`'s authoritative hook-records load happens strictly after `procs.teardown()` returns**, as a fresh `load_hook_records()` call, not a reuse of the record set read during the mid-workload allocation-snapshot poll. `hook.py`'s `atexit`-registered `_final_flush` only guarantees every buffered GC/mem record is flushed to disk once the process has actually received `SIGINT` and exited, so reading before teardown could silently miss final records.
- **A nested `try/finally` inside `run_session`** stops the RSS sampler and every py-spy recorder immediately if the workload itself raises, in an inner `finally` block that runs before the outer `finally`'s `teardown()` call. Py-spy recorder subprocesses are not children of the server's process group (they attach from outside), so `teardown()`'s `killpg` on the server tree cannot reach them -- without this nested structure, a failing workload would leak orphaned py-spy processes exactly as T-02-16 warns against.
- **`run_coldstart`'s `finally` block only calls `scenarios.coldstart_stop` if the pgid file still exists** after hyperfine's own process exits, so a cleanly concluded cold-start cycle (whose last `--conclude` already removed the pgid file) is never torn down a second time, while a hyperfine crash mid-run still gets its server cleaned up.

## Deviations from Plan

None - plan executed exactly as written. Both tasks' RED tests failed for the expected reason (unknown argparse subcommand / unknown `hyperfine` command) before implementation, and both tasks' acceptance criteria passed on the first full implementation pass with no auto-fixes required.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh` per the plan's environment note; it created `.venv` from the already-pinned lock (including psutil/aiohttp from 02-01's prior work) and installed `rsglang`/`minisgl` in editable mode. The environment check passed on the first run.
- The full slow+fast suite (`pytest python/tests -q`) takes noticeably longer than any earlier plan in this phase (~2.5 minutes), as expected given this plan spawns multiple py-spy-recorder subprocesses and sampler threads across two full end-to-end scenario sessions (s1's 8 concurrent agents, s3's hyperfine cold-start cycles). No timeouts or flakiness observed across the runs performed during this plan's execution.

## User Setup Required
None - no external service configuration required. The real `py-spy` and `hyperfine` binaries (needed by the GPU-box run) are out of scope for this Mac-side plan; `rsglang.testing.fake_profile_env` stands in for both.

## Next Phase Readiness
- Plan 02-09 (the real GPU run) can now invoke `scripts/baseline_profile.py run` exactly as this plan's Mac tests do, substituting the real `py-spy`/`hyperfine` binaries and the real `rsglang.launch --frontend python` server for the stand-ins, with the real `--out docs/benchmarks/baseline-profile.json` canonical path now reachable once that run is on Linux with a GPU visible to `nvidia-smi`.
- The `run`/`coldstart-once`/`coldstart-stop`/`validate` CLI surface (every flag named in this plan's interfaces block) is fixed and stable for 02-09 to depend on; no further changes to this contract are expected.
- No blockers identified for downstream plans in this phase. The real py-spy/hyperfine invocation paths, the real server's actual readiness timing, and the real scheduler's actual radix-cache time share all remain unverified until the GPU-box run in plan 02-09, as scoped from the start of this phase.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED
- FOUND: python/rsglang/profiling/session.py
- FOUND: scripts/baseline_profile.py
- FOUND: python/rsglang/testing/fake_profile_env.py
- FOUND: python/tests/test_baseline_profile.py
- FOUND commits: 1227840, c35508b, c0f0b53, eddbf06 (all present in `git log --oneline -6`)
