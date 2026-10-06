---
phase: 02-python-frontend-baseline-profile
plan: 06
subsystem: profiling
tags: [aiohttp, asyncio, bench_simple, hyperfine, cold-start, process-teardown]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 03
    provides: "rsglang.profiling.procs: launch_server/wait_ready/teardown/tree_memory/server_argv, race-free launch and readiness"
provides:
  - "rsglang.profiling.scenarios: RequestRecord; scenario 1 aiohttp agents (run_s1, stream_chat, plan_agent_request, make_session, get_model_id); scenario 2 bench_simple adaptation (run_s2) and run_first_request; hyperfine/coldstart helpers (hyperfine_argv, parse_hyperfine_json, coldstart_once, coldstart_stop, read_coldstart_records)"
affects: [02-08]
actuals:
  tokens: 8050
  tasks: 3
  commits: 6
  plan_head_before: 2e40dddcfc8117d331cc5e79eeb9440054d6fdda
  plan_head_after: 07145ac3d0a2b237afca02594cc6cc16bb56f4ca
tech-stack:
  added: []
  patterns:
    - "Unlimited aiohttp connector (TCPConnector(limit=0, limit_per_host=0)): the default 100-connection cap would silently queue agents 101-128 client-side and invalidate the 128-agent scenario's TTFT measurement"
    - "Per-agent deterministic RNG: random.Random(seed * 1000 + i) gives each of the 128 agents its own reproducible think-time/cancel/prompt sequence regardless of asyncio scheduling order"
    - "t_send captured immediately before the network call, never from the library's own post-header timestamp (tics[0]): both stream_chat and run_s2's wrapper measure from t_send to avoid hiding frontend request-accept latency, which is exactly what BENCH-01 profiles"
    - "Lazy imports for GPU-box-only deps: run_s2 imports openai/transformers/minisgl.benchmark.client inside the function body, so the module imports cleanly on the Mac where those packages are absent; tested by injecting fakes into sys.modules"
    - "Cold start timed to readiness, not teardown: coldstart_once leaves the server running after wait_ready so hyperfine's timer stops at the readiness signal; a separate --conclude (coldstart_stop) samples whole-tree RSS/PSS and tears down outside the timed window"
    - "Self-healing pgid file: coldstart_once reads and kills any stale recorded group before launching, so a skipped --conclude (e.g. after a warmup) never leaves two servers fighting over one port"
key-files:
  created:
    - python/rsglang/profiling/scenarios.py
    - python/tests/test_profile_scenarios.py
  modified: []
key-decisions:
  - "_kill_group and coldstart_stop's killpg calls now catch PermissionError alongside ProcessLookupError. Observed empirically on macOS: once a process group's leader has already exited (e.g. after a graceful SIGINT), a follow-up SIGKILL to the recycled pgid number can raise EPERM rather than ESRCH. Both outcomes mean 'nothing left to signal' and are treated identically. This is a Rule 1 bug fix discovered by test_coldstart_once_then_stop's self-heal path (launching, stopping implicitly via self-heal, and re-launching twice in quick succession reliably reproduced the race)."
  - "run_s2's exception handling around benchmark_one uses a broad `except Exception` (not a narrower aiohttp/openai-specific type), because the plan's own fakes-based test exercises arbitrary exception types (RuntimeError) and a malformed-but-successful return (tics of length 1) as two independent 'failed' triggers; benchmark_one's real exception surface (httpx/openai errors) is not available to narrow against on the Mac."
  - "test_profile_scenarios.py defines its own local _pid_alive/_wait_until_dead helpers rather than importing them from test_baseline_profile.py (which has an equivalent pair), to keep this plan's test file self-contained and avoid a cross-test-file import dependency between sibling Wave 3 plans' test files."
patterns-established:
  - "Scenario drivers (run_s1/run_s2/run_first_request) never construct their own base_url from a port -- they always receive base_url from the caller (02-08), and the module's only host-selecting invariant (http://127.0.0.1:<port>, T-02-11) is documented at the top of scenarios.py rather than hardcoded as a literal construction inside this module."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "Scenario 1: 128 (configurable) concurrent aiohttp agents stream /v1/chat/completions over one unlimited-connector session, with per-agent seeded cancellation decisions including headers-only prefill aborts, recording TTFT on the shared perf_counter clock"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_plan_agent_request_deterministic"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_make_session_unlimited"
        status: pass
      - kind: integration
        ref: "python/tests/test_profile_scenarios.py::test_s1_cancellation_semantics"
        status: pass
      - kind: integration
        ref: "python/tests/test_profile_scenarios.py::test_run_s1_concurrency"
        status: pass
    human_judgment: false
  - id: D2
    description: "Scenario 2: adapts bench_simple.py's own random.seed/generate_prompt/get_model_name/benchmark_one sequence via lazy imports, capturing t_send ahead of benchmark_one's tics[0] so request-accept latency is not hidden; failure paths (raised exception, malformed single-tic result) are recorded as 'failed' with the error string"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_run_s2_with_injected_helpers"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_run_s2_failure_recorded"
        status: pass
      - kind: integration
        ref: "python/tests/test_profile_scenarios.py::test_run_first_request"
        status: pass
    human_judgment: false
  - id: D3
    description: "Scenario 3: hyperfine argv/JSON helpers and coldstart_once/coldstart_stop/read_coldstart_records time cold start to readiness (not teardown), sample whole-tree RSS/PSS outside the timed window, and self-heal a stale recorded group before relaunching"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_hyperfine_argv"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_parse_hyperfine_json"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_scenarios.py::test_read_coldstart_records"
        status: pass
      - kind: integration
        ref: "python/tests/test_profile_scenarios.py::test_coldstart_once_then_stop"
        status: pass
    human_judgment: false
duration: 55min
completed: 2026-10-06
status: complete
---

# Phase 2 Plan 6: Scenario Drivers (128-agent cancellations, bench_simple saturation, hyperfine cold start) Summary

**128-agent aiohttp load generator with deterministic per-agent seeded cancellations (including headers-only prefill aborts), a lazily-imported bench_simple.py adaptation for 32-token saturation, and hyperfine-timed cold start with self-healing process-group cleanup and whole-tree PSS -- all three proven end to end on the Mac against the 02-03 stand-in server.**

## Performance
- **Duration:** ~55min | **Started:** 2026-10-05T23:55:00Z (session start, environment bootstrap) | **Completed:** 2026-10-06T00:49:29Z | **Tasks:** 3/3 | **Files modified:** 2 (both newly created)

## Accomplishments
- Built `rsglang.profiling.scenarios.run_s1`: 128 (configurable) truly concurrent aiohttp agents against `/v1/chat/completions`, each with its own `random.Random(seed * 1000 + i)` for reproducible think-time/cancellation/prompt decisions, over a session whose `TCPConnector(limit=0, limit_per_host=0)` guarantees no client-side queuing at 128 agents. `stream_chat` handles three outcomes -- completed, mid-stream cancelled, and headers-only (prefill) cancelled -- all verified against the real stand-in server's streaming chunk shape.
- Built `run_s2`, a tested adaptation of `bench_simple.py`'s own helper sequence (`random.seed`, `generate_prompt`, `get_model_name`, `benchmark_one`), with `openai`/`transformers`/`minisgl.benchmark.client` imported lazily inside the function so the module imports cleanly on the Mac (where `openai` is absent) and its control flow is proven with `sys.modules`-injected fakes, including both failure-recording paths (raised exception, malformed single-tic result).
- Built `run_first_request`, scenario 3's warm-up helper, proven against the real stand-in server.
- Built the scenario 3 helper set: `hyperfine_argv` (shlex-joined `--conclude`/once commands, never string concatenation), `parse_hyperfine_json`, `coldstart_once`/`coldstart_stop` (self-healing pgid file, readiness-not-teardown timing, whole-tree RSS/PSS via `procs.tree_memory`), and `read_coldstart_records` (warmup-dropped ready list, last-`runs` mem windows).
- All 11 tests in `python/tests/test_profile_scenarios.py` pass, including the 4 that run standalone under `-m "not slow"`. The project's full `-m "not slow"` suite shows no regression (75 passed, 36 skipped). No orphaned `fake_profile_env` process observed in `ps aux` after any test run. `git status --porcelain -- vendor/mini-sglang` stayed empty throughout.

## Task Commits
1. **Task 1: Scenario 1 -- 128 aiohttp agents with deterministic cancellations (D-06)**
   - `3b0b99e` test(02-06): RED -- scenario 1 (128 aiohttp agents, deterministic cancellations)
   - `8258421` feat(02-06): GREEN -- scenario 1 (128 aiohttp agents, deterministic cancellations)
2. **Task 2: Scenario 2 (adapted bench_simple helpers, D-07) and the first-request workload**
   - `4d16759` test(02-06): RED -- scenario 2 (bench_simple adaptation) and first-request workload
   - `fda02ce` feat(02-06): GREEN -- scenario 2 (bench_simple adaptation) and first-request workload
3. **Task 3: Scenario 3 helpers -- hyperfine command and JSON, coldstart_once and coldstart_stop (D-08)**
   - `42c63ed` test(02-06): RED -- scenario 3 helpers: hyperfine argv/JSON, coldstart_once/stop
   - `07145ac` feat(02-06): GREEN -- scenario 3 helpers: hyperfine argv/JSON, coldstart_once/stop

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md, written by the worktree executor per its orchestrator contract)

## Files Created/Modified
- `python/rsglang/profiling/scenarios.py` - the three scenario drivers, RequestRecord, and all hyperfine/coldstart helpers
- `python/tests/test_profile_scenarios.py` - all 11 tests for this plan

## Decisions Made
- **`_kill_group`/`coldstart_stop` now catch `PermissionError` alongside `ProcessLookupError` on `killpg`.** Discovered via `test_coldstart_once_then_stop`'s self-heal path: once a process group's leader has already exited (e.g. after a graceful SIGINT), a follow-up `SIGKILL` to the recycled pgid number can raise `EPERM` rather than `ESRCH` on macOS. Both mean "nothing left to signal" and are now treated identically -- see Deviations.
- **`run_s2`'s per-request exception handling uses a broad `except Exception`**, not a narrower openai/httpx-specific type, because `benchmark_one`'s real exception surface isn't available on the Mac to narrow against, and the plan's own test exercises an arbitrary `RuntimeError` plus a malformed-but-successful return (single-element `tics`) as two independent "failed" triggers.
- **Scenario drivers never construct their own base_url from a port.** `run_s1`/`run_s2`/`run_first_request` all receive `base_url` from the caller (02-08 builds it from `--port`); the module's only host-selecting invariant (`http://127.0.0.1:<port>`, T-02-11) is documented in `scenarios.py`'s module docstring rather than hardcoded as a string literal inside a function, since no function in this module builds that URL itself.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `killpg` EPERM on a recycled process-group ID after the leader has already exited**
- **Found during:** Task 3, `test_coldstart_once_then_stop` (the self-heal sub-scenario: calling `coldstart_once` a second time without an intervening `coldstart_stop`)
- **Issue:** `_kill_group`'s and `coldstart_stop`'s `os.killpg(pgid, signal.SIGKILL)` calls only caught `ProcessLookupError`. On this macOS dev machine, once a group's leader process has already exited gracefully (in response to the preceding `SIGINT`), the OS can recycle the pgid number; a subsequent signal to that number then raises `PermissionError` (EPERM) rather than `ProcessLookupError` (ESRCH), even though functionally there is nothing left in that group to signal.
- **Fix:** Both `_kill_group` and `coldstart_stop` now catch `(ProcessLookupError, PermissionError)` around every `os.killpg` call, treating both as "nothing left to signal" and continuing.
- **Files modified:** `python/rsglang/profiling/scenarios.py`
- **Commit:** `07145ac` (included in Task 3's GREEN commit, since it was discovered and fixed while making Task 3's own tests pass, before any separate commit existed)

**Total deviations:** 1 auto-fixed bug (Rule 1). **Impact:** none on the plan's scope or test coverage -- the fix makes the already-specified self-heal and teardown behavior reliable under a real macOS timing race the plan's test exercises; no behavior outside this plan's own files was touched.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh`, which created `.venv` from the already-pinned lock (02-01's prior work, including `aiohttp` and `psutil`) and installed `rsglang`/`minisgl` in editable mode. The environment check passed on the first run.
- `openai` is correctly absent from the Mac venv (confirmed via `python -c "import openai"` failing with `ModuleNotFoundError`) -- this is exactly why `run_s2`'s imports are lazy and why its control-flow test injects `sys.modules` fakes rather than using the real package. `transformers` happens to already be present on this Mac (a transitive dependency pinned for other reasons), but `run_s2` never relies on that being true, since its own test replaces it with a fake too.

## User Setup Required
None - no external service configuration required. The real `openai`/`transformers`/`minisgl.benchmark.client` import path (needed for `run_s2`'s real behavior) and the real `hyperfine` binary are GPU-box-only and are smoke-tested there in a later plan (02-09), not here.

## Next Phase Readiness
- Plan 02-08 can now import `rsglang.profiling.scenarios` using exactly the names and shapes fixed in this plan's interfaces contract: `RequestRecord`, `OUTCOMES`, `plan_agent_request`, `make_session`, `get_model_id`, `stream_chat`, `run_s1`, `run_s2`, `run_first_request`, `hyperfine_argv`, `parse_hyperfine_json`, `coldstart_once`, `coldstart_stop`, `read_coldstart_records`.
- 02-08 is responsible for building the `http://127.0.0.1:<port>` base_url and wiring `coldstart_once`/`coldstart_stop` into CLI subcommands that hyperfine's `--conclude`/timed-command arguments call, per this plan's interfaces note.
- No blockers identified for downstream plans in this phase. Scenario 2's real import path (`openai`/`transformers`/`minisgl.benchmark.client` actually resolving, not faked) remains unverified until the GPU-box run in plan 02-09, as scoped.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-06*

## Self-Check: PASSED
All 3 created/output files found on disk (`python/rsglang/profiling/scenarios.py`, `python/tests/test_profile_scenarios.py`, this SUMMARY.md). All 6 task commits (`3b0b99e`, `8258421`, `4d16759`, `fda02ce`, `42c63ed`, `07145ac`) and the plan-metadata commit (`45f321d`) found in `git log`.
