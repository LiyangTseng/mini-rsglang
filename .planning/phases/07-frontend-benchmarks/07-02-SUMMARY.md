---
phase: 07-frontend-benchmarks
plan: 02
subsystem: benchmarking
tags: [python, gc, tracemalloc, multiprocessing, instrumentation, tdd]

requires:
  - phase: 02-baseline-profiling
    provides: "python/rsglang/profiling/hook.py's env-gated sitecustomize shim, write_shim/hook_env/chain_load mechanism, load_hook_records reader"
provides:
  - "RSGLANG_PROFILE_MODE=gc_only: gc.callbacks pause timing stays on while tracemalloc.start, mem sampling and snapshot serving are skipped"
  - "_mode_from_env(value) -> 'full'|'gc_only' pure parser, unknown values fall back to 'full' with one stderr warning"
  - "hook_env(..., mode=None) optional kwarg: None leaves RSGLANG_PROFILE_MODE untouched (Phase 2 backward compatible)"
  - "Per-process multiprocessing role records: {\"kind\": \"proc\", \"pid\", \"name\", \"t\", \"wall\"}, written only in gc_only mode whenever the process's mp name changes (including the first flush tick and once more at process exit)"
affects: [07-05-mock-scheduler-gc-reader]

actuals:
  tokens: 3048
  tasks: 2
  commits: 3
plan_head_before: 10211100e40b32c7338dc72992d1ddbd05654b32
plan_head_after: bfec0402b0371094c7eefdce848f03bbf0a5bbee

tech-stack:
  added: []
  patterns:
    - "Env-var mode toggle on an existing instrumentation module instead of a second shim: _mode_from_env gates tracemalloc.start and the flush-loop body, write_shim/hook_env's PYTHONPATH injection and the shim source text stay byte-for-byte unchanged"
    - "Reading sys.modules.get('multiprocessing') instead of importing it from a background thread, to avoid import-lock contention with the main thread during interpreter startup"
    - "Change-detection record emission: a module-level _last_proc_name global suppresses duplicate proc records across flush ticks, emitting only on the first tick and on actual name transitions"

key-files:
  created:
    - python/tests/test_hook_gc_only.py
  modified:
    - python/rsglang/profiling/hook.py

key-decisions:
  - "Task 1 and Task 2 touch the same functions (install(), _flush_loop, _final_flush) in hook.py, so true TDD required building an intermediate Task-1-only state (mode gating, no proc records) before writing Task 2's RED test against it, rather than writing both tasks' code in one pass and reverse-engineering commit boundaries"
  - "_final_flush gained a mode parameter in Task 2 (not Task 1), matching the plan's reading that Task 1 only required threading mode through install() and _flush_loop; the one extra proc record at process exit is Task 2's concern"

patterns-established:
  - "gc_only mode as the default timed-run profile: full mode (RSGLANG_PROFILE_MODE unset) remains byte-for-byte Phase 2 behavior, verified by test_profile_hook.py passing completely unedited"

requirements-completed: []  # BENCH-08 is shared with 07-03/07-05/07-06/07-07/07-09/07-10 (shared-ID gate); 0/1 ready per requirements.ready-ids, so it stays open in REQUIREMENTS.md.

coverage:
  - id: D1
    description: "RSGLANG_PROFILE_MODE=gc_only keeps gc.callbacks timing on while tracemalloc.start, mem records and snapshot serving are skipped; full mode (unset/empty/\"full\") is Phase 2's exact unchanged behavior"
    requirement: BENCH-08
    verification:
      - kind: unit
        ref: "python/tests/test_hook_gc_only.py#test_gc_only_tracer"
        status: pass
      - kind: unit
        ref: "python/tests/test_hook_gc_only.py#test_mode_from_env"
        status: pass
      - kind: integration
        ref: "python/tests/test_profile_hook.py (unedited, full suite passes)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Per-process multiprocessing role records (proc records) let the harness attribute GC pauses to api_server/tokenizer/scheduler/launcher roles without py-spy: a spawn child named rsgbench-TP0-scheduler ends with that name as its last proc record, its parent ends with MainProcess, and full mode writes zero proc records"
    requirement: BENCH-08
    verification:
      - kind: unit
        ref: "python/tests/test_hook_gc_only.py#test_proc_records_name_spawn_child"
        status: pass
      - kind: unit
        ref: "python/tests/test_hook_gc_only.py#test_full_mode_writes_no_proc_records"
        status: pass
    human_judgment: false

duration: 20min
completed: 2026-10-07
status: complete
---

# Phase 07 Plan 02: GC-Only Profiling Mode and Process-Name Records Summary

**Extended `hook.py` with an `RSGLANG_PROFILE_MODE=gc_only` toggle that keeps `gc.callbacks` timing on while dropping `tracemalloc`/mem-sampling/snapshot-serving, plus per-process multiprocessing-role records for GC-pause attribution, with Phase 2's full-mode path proven byte-for-byte unchanged.**

## Performance
- **Duration:** ~20min
- **Started:** 2026-10-07 (session start)
- **Completed:** 2026-10-07
- **Tasks:** 2
- **Files modified:** 2 (1 created, 1 modified)

## Accomplishments
- `RSGLANG_PROFILE_MODE` env var with a pure `_mode_from_env` parser: unset/empty/`"full"` → `full`, `"gc_only"` → `gc_only`, anything else → `full` with one stderr warning (never raises)
- `install()` now gates `tracemalloc.start(1)` on mode; `gc.callbacks` registration and the start record are unconditional in both modes
- `_flush_loop`/`_final_flush` thread the mode through: gc_only mode always drains gc records and appends proc records, full mode keeps draining gc records plus mem sampling and snapshot serving exactly as Phase 2 did
- New `_append_proc_record_if_changed` helper reads `multiprocessing` from `sys.modules` (never imports it, to avoid import-lock contention from the hook thread) and emits a `{"kind": "proc", "pid", "name", "t", "wall"}` record only when the process's mp name changes — proven against a real spawn child named `rsgbench-TP0-scheduler`
- `hook_env(..., mode=None)` is fully backward compatible: every existing Phase 2 caller that doesn't pass `mode` behaves identically
- `test_profile_hook.py` (Phase 2's whole suite) passes completely unedited; the full `python/tests` suite (181 passed, 37 skipped) shows no regression

## Task Commits
1. **Task 1: Tracer — RSGLANG_PROFILE_MODE=gc_only keeps gc.callbacks on, tracemalloc off** - `4c5df13` (feat)
2. **Task 2: Process-name records, RED** - `3523534` (test)
3. **Task 2: Process-name records, GREEN** - `bfec040` (feat)

**Plan metadata:** (this commit, below)

## Files Created/Modified
- `python/rsglang/profiling/hook.py` - Added `PROFILE_MODE_ENV`/`MODE_FULL`/`MODE_GC_ONLY`, `_mode_from_env`, `_append_proc_record_if_changed`, `_last_proc_name`; threaded `mode` through `install()`, `_flush_loop`, `_final_flush`; added the `mode` kwarg to `hook_env`. `_SHIM_SOURCE`, `write_shim`, `chain_load`, and `load_hook_records` are byte-for-byte unchanged.
- `python/tests/test_hook_gc_only.py` - New file: `test_gc_only_tracer`, `test_proc_records_name_spawn_child`, `test_full_mode_writes_no_proc_records`, `test_mode_from_env` (parametrized, 6 cases).

## Decisions Made
- Built an intermediate "Task-1-only" state of `hook.py` (mode gating present, proc records absent) before writing Task 2's RED test, because both tasks' changes land inside the same functions (`install()`, `_flush_loop`, `_final_flush`). This produced a genuine RED (an `AssertionError` on the empty-proc-records assertion, not a collection/import error) rather than a cosmetic one, satisfying the TDD fail-fast rule against INVALID_RED.
- `_final_flush` gained its `mode` parameter only in Task 2's commit, matching the plan's task boundary exactly: Task 1's `<action>` only asks to thread `mode` through `install()` and `_flush_loop`; the extra proc-record flush at process exit is Task 2's addition.

## Deviations from Plan

None - plan executed exactly as written. Both tasks' acceptance criteria were verified directly:
- `test_gc_only_tracer` passes: `TRACING=False`, gc records present (generation 2), zero mem/alloc_top records
- `test_proc_records_name_spawn_child` passes: child's last proc record name is exactly `"rsgbench-TP0-scheduler"`, parent's is `"MainProcess"`
- `python/tests/test_profile_hook.py` passes unedited (`git diff --exit-code` on it is clean both after Task 1 and after Task 2)
- `grep -n 'RSGLANG_PROFILE_MODE' python/rsglang/profiling/hook.py` and `grep -n '"kind": "proc"' python/rsglang/profiling/hook.py` both match
- `git diff` on `hook.py` touches nothing inside the `_SHIM_SOURCE` string literal

## Issues Encountered
None. `.venv` did not exist at session start (fresh worktree), so `scripts/bootstrap_mac_env.sh` was run first per the plan's `<read_first>`/`<action>` instruction — it only re-synced the already-approved, sha256-pinned Mac lock (torch 2.9.1, numpy 2.5.3, msgpack 1.2.3, pyzmq 27.2.0, transformers 4.57.3, pytest 9.1.1) and installed nothing new.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
Plan 07-05's Rust reader (`crates/rsg-bench/src/gclog.rs`) can now consume gc_only mode's record set: `start` (unchanged shape), `gc` (unchanged shape), and the new `proc` records with fields `pid`/`name`/`t`/`wall`. Role attribution is intended to map a pid's last proc record's `name` to a benchmark role via suffix/substring match (`-scheduler`, `tokenizer`, `MainProcess`), per this plan's `<interfaces>` contract. No blockers for downstream plans.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07*

## Self-Check: PASSED

- FOUND: python/rsglang/profiling/hook.py
- FOUND: python/tests/test_hook_gc_only.py
- FOUND: .planning/phases/07-frontend-benchmarks/07-02-SUMMARY.md
- FOUND: 4c5df13 (feat: gc_only mode)
- FOUND: 3523534 (test: RED for proc records)
- FOUND: bfec040 (feat: GREEN for proc records)
