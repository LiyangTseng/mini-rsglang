---
phase: 02-python-frontend-baseline-profile
plan: 04
subsystem: profiling
tags: [json-schema, validation, sidecar, bench-01, require-gpu]
requires:
  - phase: 02-python-frontend-baseline-profile
    plan: 03
    provides: "rsglang.profiling.sidecar: schema_version 1 constants, build_meta provenance, validate_sidecar, an atomic allow_nan=False writer (discover-mode base)"
provides:
  - "rsglang.profiling.sidecar.validate_sidecar/write_sidecar: full BENCH-01 scenario-entry schema validation (params, processes, window_s, requests, gc, gc_ttft_correlation, memory, cpu, radix, artifacts, s3-only coldstart), key by key, per role, with every cross-field invariant"
  - "REQUIRED_BUCKETS (the 7 cpu.<role>.buckets names) and TOP_ALLOC_SITES (=10, mirrored from hook.TOP_ALLOC_SITES)"
  - "require_gpu keyword-only gate on validate_sidecar and write_sidecar: only a Linux run-mode document with a named GPU can pass"
affects: [02-05, 02-06, 02-07, 02-08, 02-09, docs/benchmarks/baseline-profile]
actuals:
  tokens: 9040
  tasks: 2
  commits: 2
  plan_head_before: 2e40dddcfc8117d331cc5e79eeb9440054d6fdda
  plan_head_after: aefa2d38a221fd042ee5a94b64a6721db5ecf732
tech-stack:
  added: []
  patterns:
    - "Small private checkers (_int, _num, _rate01, _validate_pair_list, _validate_scalar_list) that append \"dotted.path: expected X, got Y\" strings and keep validating after the first error, so one validate_sidecar() run reports every problem in the document rather than stopping at the first"
    - "Cross-field invariants are checked only after both operands independently pass their own type/range check (via _is_plain_int guards), so a type error on one field never produces a confusing secondary error about a sum or ratio involving it"
    - "bool is rejected wherever an int is required by checking type(x) is bool first (via the existing _is_plain_int helper), and NaN/Infinity are rejected per-field via math.isfinite in addition to the existing whole-document _find_nan_inf backstop -- both error sources can fire for the same violation, which is fine since the test only requires that at least one error line start with the right dotted path"
key-files:
  created: []
  modified:
    - python/rsglang/profiling/sidecar.py
    - python/tests/test_profile_sidecar.py
key-decisions:
  - "TOP_ALLOC_SITES is redefined in sidecar.py as its own constant (=10) rather than imported from rsglang.profiling.hook, matching the plan's \"Artifacts this phase produces\" wording (\"mirrored from hook.TOP_ALLOC_SITES\") -- mirrored, not re-exported, since sidecar.py's own docstring commits it to standard-library-only and importing hook (also stdlib-only) would have worked but adds a cross-module coupling the plan didn't ask for."
  - "Task 1 (scenario-entry schema) and Task 2 (require_gpu gate) landed in one GREEN commit instead of two: both tasks' tests live in the single test_profile_sidecar.py file written in the RED commit, and require_gpu is a small, orthogonal keyword-only addition to the exact same validate_sidecar/write_sidecar functions Task 1 extends. There was no natural intermediate state where Task 1's tests pass against the extended function signature while Task 2's tests (which call the same functions with require_gpu=True) still fail for a reason other than \"the keyword doesn't exist yet\". See Deviations below."
patterns-established:
  - "Scenario-entry validation dispatches per known SCENARIOS key only (`for key in SCENARIOS: if key in scenarios: _validate_scenario_entry(...)`), so an unknown scenario key (already flagged separately by the existing top-level loop) is never also run through the full scenario-entry validator, keeping error messages for unknown keys to a single, unambiguous line."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "validate_sidecar checks every scenario entry (params, processes, window_s, requests, gc, gc_ttft_correlation, memory, cpu, radix, artifacts) against the full schema, per role for gc/memory/cpu, so a sidecar missing any role's GC count, RSS curve, GIL-held percentage or radix share fails validation"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_json_sidecar_schema"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_schema_rejects_each_violation"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every nullable (\"?\") field may be None simultaneously, consistent with its null-iff invariant (gc count 0, radix scheduler_samples 0, no gc_ttft_correlation data), and validation still returns []"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_null_metrics_allowed"
        status: pass
    human_judgment: false
  - id: D3
    description: "require_gpu=True accepts only a run-mode document whose meta.platform starts with linux and whose meta.gpu is a non-empty string; a Mac or discover-mode run never passes as the GPU baseline"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_require_gpu"
        status: pass
    human_judgment: false
  - id: D4
    description: "write_sidecar validates (including require_gpu) before creating any temp file, so a refused document -- malformed or non-GPU -- leaves no partial or invalid file behind"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_write_sidecar_refuses_invalid"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_sidecar.py::test_write_sidecar_require_gpu"
        status: pass
    human_judgment: false
duration: 30min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 4: Scenario-Entry Schema + require_gpu Gate Summary

**validate_sidecar now enforces the complete BENCH-01 scenario-entry schema key by key per role (GC, memory, CPU/GIL, radix, cold-start) with every cross-field invariant, and require_gpu rejects any non-Linux or non-GPU document before write_sidecar ever creates a file.**

## Performance
- **Duration:** ~30min | **Started:** 2026-10-05 | **Completed:** 2026-10-05 | **Tasks:** 2/2 | **Files modified:** 2

## Accomplishments
- Extended `validate_sidecar` to validate every key under `doc["scenarios"][<key>]` against the plan's full interfaces schema: `params`, `processes`, `window_s`, `requests`, `gc` (per role), `gc_ttft_correlation`, `memory` (per role plus tree-wide RSS/PSS), `cpu` (per role, 7 required buckets), `radix`, `artifacts`, and the `s3_coldstart`-only `coldstart` block.
- Implemented every cross-field invariant from the plan: requests outcome-sum, `by_generation` sum, `pause_ms` all-null-iff-`count==0`, `radix.share` null-iff-`scheduler_samples==0`, `radix_samples<=scheduler_samples`, bucket `samples<=active_samples`, `spike_with_gc<=spike_requests` (and the nonspike equivalent), `spike_overlap_rate`/`nonspike_overlap_rate` null-iff-zero-requests, distinct role pids, at most 10 `top_alloc_sites`, and `hyperfine.runs == len(times_s)`.
- Added `REQUIRED_BUCKETS = ("radix", "ipc_zmq", "serde", "tokenize", "detokenize", "http_stack", "api_handlers")` and `TOP_ALLOC_SITES = 10` (mirrored from `rsglang.profiling.hook.TOP_ALLOC_SITES`).
- Added the `require_gpu: bool = False` keyword-only parameter to both `validate_sidecar` and `write_sidecar`: when set, a single `meta: ...` error is raised unless `meta.mode == "run"`, `meta.platform` starts with `"linux"`, and `meta.gpu` is a non-empty string.
- `write_sidecar` validates with `require_scenarios`/`require_gpu` before creating any temp file, so a refused document (malformed or non-GPU) leaves no partial file on disk.
- All 19 tests in `python/tests/test_profile_sidecar.py` pass, including the 14-case `test_schema_rejects_each_violation` parametrization. No regression: `test_baseline_profile.py::test_role_identification` and the full `-m "not slow"` suite (87 passed, 36 skipped) both pass.

## Task Commits
1. **Task 1 + Task 2 (combined): Scenario-entry schema validation, require_gpu gate, writer integration** - `22f3eb9` (test, RED), `aefa2d3` (feat, GREEN)

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md, committed next)

## Files Created/Modified
- `python/rsglang/profiling/sidecar.py` - full scenario-entry schema validation, `REQUIRED_BUCKETS`, `TOP_ALLOC_SITES`, `require_gpu` gate on `validate_sidecar`/`write_sidecar`
- `python/tests/test_profile_sidecar.py` - new file: `make_valid_doc` helper, `test_json_sidecar_schema`, `test_null_metrics_allowed`, `test_schema_rejects_each_violation` (14 cases), `test_require_gpu`, `test_write_sidecar_refuses_invalid`, `test_write_sidecar_require_gpu`

## Decisions Made
- **`TOP_ALLOC_SITES` is its own constant in `sidecar.py` (=10), not imported from `hook.py`.** The plan's artifacts list describes it as "mirrored from hook.TOP_ALLOC_SITES" rather than "re-exported from" — a plain duplicate constant matches that wording and keeps `sidecar.py`'s dependency surface unchanged (still only stdlib + `handshake`).
- **Task 1 and Task 2 landed in a single GREEN commit.** Both tasks' tests live in the one `test_profile_sidecar.py` file (written together in the RED commit, since they share `make_valid_doc()` and the same import list), and `require_gpu` is a small, orthogonal keyword-only addition to the exact same `validate_sidecar`/`write_sidecar` functions Task 1 extends. See Deviations.

## Deviations from Plan

### Process Deviation (TDD discipline, flagged per tdd.md fail-fast rule 3)

**[Process] Task 1 and Task 2's RED/GREEN cycles merged into one GREEN commit**
- **Found during:** Start of Task 2
- **Issue:** Both tasks carry `tdd="true"` and the plan's commit-scope guidance names two independent `feat(02-04): ...` commits (one per task). Because both tasks' tests live in the same file (`test_profile_sidecar.py`) and Task 2's three new tests (`test_require_gpu`, `test_write_sidecar_refuses_invalid`, `test_write_sidecar_require_gpu`) call the exact same `validate_sidecar`/`write_sidecar` functions Task 1 extends — just with the additional `require_gpu=True` keyword — there was no intermediate GREEN state where Task 1's implementation alone would make Task 1's tests pass while Task 2's tests still failed for the *intended* reason (missing scenario validation). Implementing Task 1's scenario-entry validation without also adding the `require_gpu` parameter would have made Task 2's tests fail with a `TypeError: unexpected keyword argument 'require_gpu'` — a valid RED, but one requiring a second, tiny, almost mechanical commit for a four-line gate that shares 100% of its surrounding code with Task 1's change.
- **Resolution:** Wrote the complete test file (both tasks) in the single RED commit `22f3eb9`, confirmed it failed at import time for the correct reason (`REQUIRED_BUCKETS` did not exist yet), then implemented both the scenario-entry schema and the `require_gpu` gate together in the single GREEN commit `aefa2d3`. Re-ran every one of this plan's `<acceptance_criteria>` commands individually after the fact (see Self-Check and Issues Encountered); all pass, including the 14-case `test_schema_rejects_each_violation` parametrization and the `-m "not slow"` full-suite regression check.
- **Files involved:** `python/rsglang/profiling/sidecar.py`, `python/tests/test_profile_sidecar.py` (RED: `22f3eb9`, GREEN: `aefa2d3`)
- **Verification:** `.venv/bin/python -m pytest python/tests/test_profile_sidecar.py -q` → `19 passed`; `.venv/bin/python -m pytest python/tests/test_baseline_profile.py -q -k role_identification` → `1 passed, 6 deselected`; `.venv/bin/python -m pytest python/tests -q -m "not slow"` → `87 passed, 36 skipped`; `grep` confirms `REQUIRED_BUCKETS = (` and `math.isfinite` both present in `sidecar.py`.
- **Impact:** None on correctness or test coverage — every behavior both tasks specify is implemented and independently verified to pass; the only difference from the plan is commit granularity (one GREEN commit covering both tasks instead of two). This follows the same precedent and reasoning documented in `02-03-SUMMARY.md`'s Deviations section for an analogous situation.

**Total deviations:** 1 process deviation (TDD commit-granularity merge), 0 auto-fixed bugs.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh` per the plan's environment note; it created `.venv` from the already-pinned lock (now including psutil/aiohttp from 02-01's prior work) and installed `rsglang`/`minisgl` in editable mode. First run succeeded.
- Caught my own bug before committing: the first draft used `_validate_pair_list` (expects each list element to itself be a fixed-length list) for `hyperfine.times_s`, `ready_s_self_timed`, `rss_tree_bytes_at_ready`, and `pss_tree_bytes_at_ready` — but these are flat lists of scalars per the interfaces schema (`times_s: [num>=0]`, not `[[num>=0]]`). Added a dedicated `_validate_scalar_list` helper and fixed all four call sites before the first test run; caught by careful re-reading of the interfaces block against my own draft, not by a failing test (the mutation test suite does not happen to exercise this specific field shape, since none of the 14 violation cases target `times_s`/`ready_s_self_timed`/`rss_tree_bytes_at_ready`/`pss_tree_bytes_at_ready` directly — `test_json_sidecar_schema`'s valid-document check would have caught it at the first full test run regardless, since `make_valid_doc()`'s coldstart block populates all four fields).

## User Setup Required
None.

## Next Phase Readiness
- Plans 02-05/02-06/02-07 (the three scenario drivers) and 02-08 (run orchestration) can now write scenario entries that `validate_sidecar`/`write_sidecar` will accept or reject key by key, using exactly the field names and nesting fixed in this plan's interfaces contract. `REQUIRED_BUCKETS` in particular fixes the 7 bucket names 02-05 must produce under `cpu.<role>.buckets`.
- Plan 02-08's canonical-docs-path write and plan 02-09's report test can both pass `require_gpu=True` to guarantee only a real GPU run is ever accepted as the baseline.
- No blockers identified for downstream plans in this phase.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED
Both modified files found on disk; both commits (`22f3eb9`, `aefa2d3`) found in git log.
