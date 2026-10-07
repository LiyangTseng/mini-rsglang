---
phase: 06-gpu-end-to-end-parity
plan: 01
subsystem: testing
tags: [parity, pytest, aiohttp, sitecustomize, meta_path, scheduler-instrumentation]

requires:
  - phase: 02-python-frontend-baseline-profile
    provides: "The docs/benchmarks/{name}.{md,json} sidecar convention and the generated-sitecustomize-shim pattern (rsglang.profiling.hook), both reused here"
provides:
  - "rsglang.parity package: tap schema + env-gated backend instrumentation, corpus loader, ids-first compare, validated sidecar, one-request-at-a-time sweep"
  - "scripts/parity_check.py run/validate CLI"
  - "rsglang.testing.fake_parity_server: Mac-only deterministic stand-in server for the tracer tests"
affects: [06-03-canonical-corpus-llama, 06-04-concurrent-load, 06-05-abort-stress, 06-06-gpu-script, 06-07-report, 06-08-ship]

actuals:
  tokens: 19440
  tasks: 2
  commits: 3
  plan_head_before: 38d1dca9facc1a69ef8fa2bf41cba896dd8bd5b6
  plan_head_after: b3740571a840f4fd5fecc42943dbd624aba7dbc8

tech-stack:
  added: []
  patterns:
    - "Generated sitecustomize shim installed via a meta_path finder that wraps the target module's loader.exec_module, applied before (Phase 2's hook) and now inside (this plan's tap) a vendored/CUDA-only module import"
    - "Wrapper rule: record (guarded by its own try/except), then always delegate -- return the original's value, propagate the original's exception unchanged"

key-files:
  created:
    - python/rsglang/parity/__init__.py
    - python/rsglang/parity/tap.py
    - python/rsglang/parity/corpus.py
    - python/rsglang/parity/compare.py
    - python/rsglang/parity/sidecar.py
    - python/rsglang/parity/sweep.py
    - scripts/parity_check.py
    - python/rsglang/testing/fake_parity_server.py
    - python/tests/test_parity_check.py
    - python/tests/test_parity_tap.py
  modified: []

key-decisions:
  - "Followed the plan's two deviations from RESEARCH.md: one tap (a generated sitecustomize shim outside vendor/) instead of two (vendored log line + Rust dump), and aiohttp instead of the openai SDK for the sweep client"
  - "tdd-red-evidence check not run for Task 2's RED phase: it parses only Node's `--test` TAP summary format, which pytest's default reporter does not emit; workflow.tdd_mode is disabled in this project's config, so the automated gate is not enforced. RED evidence (5 genuine failures, 1 pre-existing pass) was verified manually instead and is recorded below"

requirements-completed: [PAR-01]

coverage:
  - id: D1
    description: "A fresh Python-frontend session and a fresh Rust-frontend session each serve the corpus one request at a time, their tap ids and HTTP text joined, compared ids-first, and written to a validated sidecar; a single injected divergence exits 1 with the first differing index recorded"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_tracer_sequential_identical"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_tracer_divergence_exits_1"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_validate_cli"
        status: pass
    human_judgment: false
  - id: D2
    description: "With RSGLANG_PARITY_TAP_DIR set and the generated shim on PYTHONPATH, a fresh interpreter importing the real vendored minisgl.scheduler.scheduler gets all four tap targets wrapped; without the env var, none are. Every wrapper records then always delegates, and a recording failure never reaches the scheduler"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_tap.py#test_patch_real_scheduler_methods"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_tap.py#test_recording_failure_never_raises"
        status: pass
      - kind: integration
        ref: "python/tests/test_parity_tap.py#test_shim_patches_in_fresh_interpreter"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_tap.py#test_install_noop_without_env"
        status: pass
    human_judgment: false
  - id: D3
    description: "No file under vendor/ is modified; scripts/check_upstream.py --offline still passes"
    requirement: "PAR-01"
    verification:
      - kind: other
        ref: ".venv/bin/python scripts/check_upstream.py --offline"
        status: pass
    human_judgment: false

duration: 55min
completed: 2026-10-06
status: complete
---

# Phase 6 Plan 1: Parity Measurement Path and Real Backend Tap Summary

**Built the end-to-end parity measurement path (fresh session per frontend, one-at-a-time HTTP sweep, ids-first comparison, validated sidecar) against a Mac-only fake server, then made the backend tap real by env-gated wrapping of four methods on the actual vendored `Scheduler`/`SchedulerIOMixin` classes.**

## Performance

- **Duration:** 55 min
- **Tasks:** 2 completed
- **Files modified:** 10 (all new)

## Accomplishments

- `rsglang.parity` package: tap record schema (`TAP_DIR_ENV`, six `KIND_*` constants, `TapRecords`, `tap_env`, `load_tap_records`), a corpus loader with full-pass validation, `compare_prompt`/`summarize` (ids first, D-04 zero tolerance, D-06 secondary text check), a sidecar schema/validator/atomic writer mirroring `rsglang.profiling.sidecar`, and `sweep.run_session`/`send_sequential`/`join_sequential` (one request at a time, backend-tap-to-HTTP-result join)
- `scripts/parity_check.py` with `run`/`validate` subcommands, mirroring `baseline_profile.py`'s shape and exit-code convention (0 OK, 1 measurement failure/mismatch/invalid sidecar, 2 environment error)
- `rsglang.testing.fake_parity_server`: a Mac-only deterministic stand-in server that renders prompts, computes deterministic output ids/text, and emits tap records in the real schema -- enough to prove the whole path end to end without GPU access
- The real backend tap: `tap.install()`/`tap.patch()` wrap `Scheduler._process_one_msg`, `Scheduler._free_req_resources`, `Scheduler._prepare_batch`, and `SchedulerIOMixin._reply_tokenizer_rank0` at class level, idempotently, via a generated `sitecustomize.py` shim installed on `PYTHONPATH` by `tap_env`. Every wrapper records (guarded by its own try/except) then always delegates, returning the original's value and letting the original's exception propagate unchanged
- Proved against the real vendored classes (not mocks): `_process_one_msg` for `UserMsg` and `AbortBackendMsg`, `_free_req_resources`'s double-free producing `dup_free_slots: false` then `true` against a real `TableManager`, `_reply_tokenizer_rank0` still emitting one `BatchTokenizerMsg`, and `_prepare_batch`'s collision detection plus unmodified sentinel-exception propagation from a stand-in `pad_batch`

## Task Commits

1. **Task 1: Tracer -- one prompt through fake Python and Rust sessions, compared ids first, lands in a validated sidecar** - `ed3dfb5` (feat)
2. **Task 2 RED: failing tests for the real backend tap wrappers** - `1ee28d8` (test)
3. **Task 2 GREEN: real backend tap via env-gated wrappers and a generated shim** - `b374057` (feat)

No REFACTOR commit: the GREEN implementation needed no behavior-neutral cleanup.

**Plan metadata:** recorded separately in the `docs(06-01)` commit that adds this SUMMARY, STATE.md, ROADMAP.md and REQUIREMENTS.md.

## Files Created/Modified

- `python/rsglang/parity/__init__.py` - package docstring naming PAR-01/PAR-02
- `python/rsglang/parity/tap.py` - tap schema, `tap_env`, `load_tap_records` (Task 1); `install`, `patch`, `write_shim`, `_PatchOnImport`, the four wrapper factories, the process-local record writer, and `_reset_for_tests` (Task 2)
- `python/rsglang/parity/corpus.py` - `CorpusItem`, `CorpusError`, `load_corpus` (full-pass validation), `corpus_sha256`
- `python/rsglang/parity/compare.py` - `LAYERS`, `compare_prompt` (request_error/backend/detokenization_or_api), `summarize`
- `python/rsglang/parity/sidecar.py` - schema/meta/validated atomic writer for `docs/benchmarks/parity-report.json`
- `python/rsglang/parity/sweep.py` - `run_session`, `send_sequential` (aiohttp, one request at a time), `join_sequential` (tap-to-HTTP join)
- `scripts/parity_check.py` - `run`/`validate` CLI
- `python/rsglang/testing/fake_parity_server.py` - Mac-only deterministic stand-in server
- `python/tests/test_parity_check.py` - tracer tests (fake sessions, divergence, validate CLI)
- `python/tests/test_parity_tap.py` - tap behavior tests against the real vendored classes, plus the AST drift guard

## Decisions Made

- Implemented exactly the plan's two stated deviations from RESEARCH.md: one tap (generated sitecustomize shim, outside `vendor/`) instead of RESEARCH's proposed two (a vendored log line plus a separate Rust dump), and `aiohttp` instead of the `openai` SDK for the sweep's HTTP client, since the sweep only needs non-streaming JSON requests.
- `gsd_run check tdd-red-evidence` was **not** run for Task 2's RED phase. Inspecting `.claude/gsd-core/bin/lib/tdd-red-evidence.cjs` showed it parses only Node's `--test` TAP summary format (`# tests N`/`# pass N`/`# fail N` lines, `ok N - <name>` lines) or Surefire/Failsafe XML -- neither of which pytest's default text reporter emits. Running it against raw pytest output would misclassify a genuine RED as `INVALID_RED` (`zero_tests_discovered`), which is worse than not running it. `workflow.tdd_mode` is `false` in this project's `.planning/config.json` (confirmed via `config-get`), so the automated gate is not enforced for this plan. RED evidence was instead verified manually and is recorded in "TDD Gate Compliance" below. This mirrors the documented Rust gap already called out in `gsd-core/references/tdd.md` ("Known gap — Rust (#4379)") for a different reason (path-based detection vs. format mismatch), and should be flagged for anyone adding TDD gate tooling to a non-Node project.
- The abort-record stand-in test could not use `types.SimpleNamespace` for objects placed in `DecodeManager.running_reqs` (a `set`): `SimpleNamespace` defines `__eq__`, which makes it unhashable. Added a minimal `_StandInReq` test helper class instead.
- The real (unwrapped) `_process_one_msg` logs through `logger.debug_rank0`, which reads process-global TP info (`minisgl.distributed.info`). Tests call `set_tp_info(0, 1)` once at module import time, guarded by `try_get_tp_info() is None` (the real setter raises if called twice in-process).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] No .venv existed in this fresh worktree**
- **Found during:** Setup, before Task 1
- **Issue:** `.venv/bin/python` did not exist; the worktree was created without running the project's bootstrap step.
- **Fix:** Ran `scripts/bootstrap_mac_env.sh` (idempotent, documented, safe to rerun in any checkout per its own docstring) to create the hash-pinned Mac venv and install `minisgl`/`rsglang` in editable mode.
- **Files modified:** none (only `.venv/`, which is gitignored)
- **Verification:** `import minisgl.scheduler.scheduler, minisgl.scheduler.io, minisgl.scheduler.table, aiohttp` succeeded afterward.
- **Commit:** n/a (no tracked files changed)

**2. [Rule 1 - Bug] Real `_process_one_msg` needs process-global TP info set**
- **Found during:** Task 2 GREEN, first run of `test_patch_real_scheduler_methods`
- **Issue:** Calling the real (wrapped) `_process_one_msg` raised `RuntimeError: TP info has not been set` from `logger.debug_rank0`, since TP info is a module-global set once per process and never set in a bare pytest run.
- **Fix:** Added a module-level `set_tp_info(0, 1)` guarded by `try_get_tp_info() is None` at the top of `test_parity_tap.py`.
- **Files modified:** `python/tests/test_parity_tap.py`
- **Verification:** Both affected tests pass.
- **Commit:** `b374057` (part of Task 2's GREEN commit)

**3. [Rule 1 - Bug] `types.SimpleNamespace` is unhashable, but `DecodeManager.running_reqs` is a set**
- **Found during:** Task 2 GREEN, first run of `test_patch_real_scheduler_methods`
- **Issue:** `TypeError: unhashable type: 'types.SimpleNamespace'` when building the abort-scenario's `running_reqs` set.
- **Fix:** Added a minimal `_StandInReq` helper class (identity-hashable) for objects that must live in a set.
- **Files modified:** `python/tests/test_parity_tap.py`
- **Verification:** `test_patch_real_scheduler_methods` passes.
- **Commit:** `b374057` (part of Task 2's GREEN commit)

---

**Total deviations:** 3 auto-fixed (1 Rule 3 blocker, 2 Rule 1 bugs, all in test/setup code, none in the committed implementation's design).
**Impact:** None of these changed scope or design; all were necessary to exercise the real vendored classes correctly in a plain pytest process.

## TDD Gate Compliance

Task 2 (`tdd="true"`) followed RED -> GREEN; no REFACTOR commit was needed.

| Gate | Commit | Status |
|------|--------|--------|
| RED | `1ee28d8` `test(06-01): add failing tests for the real backend tap wrappers` | 5 of 6 tests failed intentionally (`AttributeError` on the not-yet-existing `tap.install`/`tap.patch`/`tap.write_shim`/`tap._reset_for_tests`, or a genuine behavior mismatch for the fresh-interpreter case); `test_vendored_tap_targets_exist` passed because it only asserts on pre-existing vendored source shape |
| GREEN | `b374057` `feat(06-01): real backend tap via env-gated wrappers and a generated shim` | All 6 tests in `test_parity_tap.py` pass (9 total across both test files in this plan) |
| REFACTOR | none | No behavior-neutral cleanup needed |

**`gsd_run check tdd-red-evidence` not run** (see "Decisions Made" above for the full explanation): the tool's TAP-summary parser is Node-`--test`-specific and does not understand pytest's default output format, and `workflow.tdd_mode` is disabled project-wide, so the automated gate was not applicable. Manual RED evidence:

```
command: .venv/bin/python -m pytest python/tests/test_parity_tap.py -q
exit_code: 1 (before GREEN)
result: 5 failed, 1 passed
  - test_install_noop_without_env: AttributeError (tap._reset_for_tests missing)
  - test_patch_real_scheduler_methods: AttributeError (tap._reset_for_tests missing)
  - test_recording_failure_never_raises: AttributeError (tap._reset_for_tests missing)
  - test_shim_patches_in_fresh_interpreter: AssertionError (WRAPPED=False,False,False,False -- expected True,True,True,True)
  - test_tap_records_load_cleanly: AttributeError (tap._reset_for_tests missing)
  - test_vendored_tap_targets_exist: PASSED (pre-existing vendored-source assertion, unrelated to new code)
expected: all six target behaviors described in the plan's <behavior> block
actual (after GREEN): .venv/bin/python -m pytest python/tests/test_parity_tap.py -q -> 6 passed
```

## Issues Encountered

None beyond the three auto-fixed deviations above, all resolved inline.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

The `rsglang.parity` package's contract (tap schema, `compare_prompt`/`summarize`, sidecar schema, sweep functions, CLI flags) is locked per the plan's interfaces block for plans 06-03 through 06-06 to extend without renaming. The real backend tap is proven against the actual vendored `Scheduler`/`SchedulerIOMixin` classes, not just the fake server, so plan 06-04 (canonical corpus + GPU run) and 06-05 (abort stress) can rely on it for real token-id ground truth. `vendor/` remains untouched; `scripts/check_upstream.py --offline` still passes.

Ready for 06-02 (parallel; no dependency on this plan) and 06-03 (depends on this plan's corpus/compare/sidecar contract).

## Self-Check: PASSED

- All 10 created files found on disk, plus this SUMMARY.
- All 3 task commits (`ed3dfb5`, `1ee28d8`, `b374057`) found in `git log`.
- Re-ran plan-level `<verification>`: `pytest python/tests/test_parity_check.py python/tests/test_parity_tap.py -q` -> 9 passed; `scripts/check_upstream.py --offline` -> `check_upstream: OK`.
- Re-ran every task's `<acceptance_criteria>` command: all passed (see Task Commits / Decisions Made for detail).
