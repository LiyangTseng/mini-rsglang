---
phase: 02-python-frontend-baseline-profile
plan: 05
subsystem: profiling
tags: [speedscope, py-spy, radix-cache, gc-correlation, percentile, pure-functions]
requires:
  - phase: 02-python-frontend-baseline-profile
    provides: "plan 02-02's rsglang.profiling.hook JSONL record contract (gc/mem/alloc_top, keyed by kind+pid)"
provides:
  - "rsglang.profiling.analysis: load_speedscope, frame_matches, RADIX_RULES, BUCKETS, bucket_samples, radix_share, cpu_metrics, percentile, slice_window, gc_stats, gc_ttft_correlation, summarize_requests, memory_role_summary"
  - "Exact, tested definitions for every BENCH-01 number: radix share (D-09/D-10), per-process CPU-active/GIL-held fractions, IPC/serde/tokenize/detokenize/HTTP per-request cost (D-04), GC pause stats and P99-spike overlap (D-03), memory growth (D-02)"
affects: [02-08-run-orchestration]
actuals:
  tokens: 7986
  tasks: 2
  commits: 4
  plan_head_before: 2e40dddcfc8117d331cc5e79eeb9440054d6fdda
  plan_head_after: a184351e9dbe097cb2f3d4586a636cec1c1311bc
tech-stack:
  added: []
  patterns:
    - "Frame matching: (path_fragment, names|None) rule tuples, matched via substring-on-file + co_name-or-qualified-suffix-on-name, precomputed per-frame-index once per bucket_samples() call so cost stays linear in sample count"
    - "Null-on-zero-denominator throughout: every ratio (share, pct, share_of_active, per_request_ms, rps, overlap_rate, percentile of empty input) returns None rather than 0 or raising, so a scheduler/process with 0 samples never silently reports a wrong number"
    - "Pure duck-typed request records (t_send/t_first/t_end/outcome attributes) -- analysis.py never imports scenarios.py, matching the interfaces contract for plan 02-06's RequestRecord"
key-files:
  created:
    - python/rsglang/profiling/analysis.py
    - python/tests/test_profile_analysis.py
  modified: []
key-decisions:
  - "gc_ttft_correlation's test fixture uses n=60 qualifying requests (not literally the 100 in the plan's prose), because nearest-rank P99 over n=100 with a single outlier mathematically always selects the top of the non-outlier group (ceil(0.99*100)-1 = 98, the second-highest value, never the unique max) -- it can never isolate exactly one spike via '>= P99'. For n<100, ceil(0.99*n)=n, so the nearest-rank P99 is exactly the max, which is what makes 'the outlier is the only spike' achievable. n=60 preserves every other behavioral assertion (single spike, GC-pause-inside overlap, touching-boundary non-overlap, t_first exclusion) the plan's behavior bullet specifies."
patterns-established:
  - "BUCKETS insertion order and names (radix, ipc_zmq, serde, tokenize, detokenize, http_stack, api_handlers) are the fixed contract plan 02-04's sidecar.REQUIRED_BUCKETS and plan 02-08 must match verbatim."
requirements-completed: [BENCH-01]
coverage:
  - id: D1
    description: "load_speedscope validates shared.frames/profiles presence and per-profile samples/weights length parity, raising SpeedscopeError naming the offending profile instead of silently producing a wrong bucket count (RESEARCH A3)"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_load_speedscope_valid_and_invalid"
        status: pass
    human_judgment: false
  - id: D2
    description: "Radix frame rules (match_req/cache_req in scheduler/cache.py; match_prefix/insert_prefix/evict/_tree_walk in kvcache/radix_cache.py) match both co_name and qualified-name forms, never cross-file, and a nested match in one sample counts once; radix_share divides by total scheduler-process samples across all profiles (D-09/D-10), None when zero"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_radix_frame_bucketing"
        status: pass
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_radix_share_zero_samples"
        status: pass
    human_judgment: false
  - id: D3
    description: "bucket_samples buckets every sample across every profile into ipc_zmq/serde/tokenize/detokenize/http_stack/api_handlers (plus radix), one sample can hit multiple buckets, every bucket key always present even at 0"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_bucket_samples_all_buckets"
        status: pass
    human_judgment: false
  - id: D4
    description: "cpu_metrics computes cpu_active_pct/gil_held_pct and per-bucket share_of_active/per_request_ms, None on every zero-denominator case (window_s=0, requests_completed=0, active_samples=0, gil_doc=None)"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_cpu_metrics"
        status: pass
    human_judgment: false
  - id: D5
    description: "percentile uses nearest rank (ceil(p/100*n)-1), None on empty input, ValueError outside (0,100]"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_percentile"
        status: pass
    human_judgment: false
  - id: D6
    description: "slice_window filters hook records to a closed [t0,t1] interval on their 't' field"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_slice_window"
        status: pass
    human_judgment: false
  - id: D7
    description: "gc_stats builds count/by_generation/total_pause_ms/pause_ms percentiles/collected/events from already-sliced gc records, with an all-zero/None shape for 0 collections"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_gc_stats"
        status: pass
    human_judgment: false
  - id: D8
    description: "gc_ttft_correlation classifies spikes via nearest-rank P99 TTFT, detects strict-interval GC-pause overlap (touching an endpoint does not count), excludes requests without t_first, and returns None when no request has t_first"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_gc_ttft_correlation"
        status: pass
    human_judgment: false
  - id: D9
    description: "summarize_requests reports sent/completed/cancelled/failed counts, TTFT percentiles over completed-or-cancelled records with t_first, and rps (None when window_s<=0)"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_summarize_requests"
        status: pass
    human_judgment: false
  - id: D10
    description: "memory_role_summary builds RSS start/end/max/growth plus curve, and tracemalloc current_start/current_end/peak plus curve, from rss samples and hook.py mem records, with all-None/empty shape when there are no samples"
    requirement: "BENCH-01"
    verification:
      - kind: unit
        ref: "python/tests/test_profile_analysis.py::test_memory_role_summary"
        status: pass
    human_judgment: false
duration: 35min
completed: 2026-10-05
status: complete
---

# Phase 2 Plan 5: Analysis Layer (speedscope bucketing, radix share, CPU/GIL, GC-TTFT correlation) Summary

**Pure analysis layer computes radix-cache share of scheduler time, per-process CPU-active/GIL-held fractions, IPC/serialization per-request cost, GC pause statistics and their P99-TTFT-spike overlap, and memory growth -- entirely from speedscope profiles and hook.py JSONL records, with every ratio null rather than zero or a crash on a zero denominator.**

## Performance
- **Duration:** ~35min | **Started:** 2026-10-05 | **Completed:** 2026-10-05 | **Tasks:** 2/2 | **Files modified:** 2 (both newly created)

## Accomplishments
- Built `rsglang.profiling.analysis` end to end: `SpeedscopeError`, `load_speedscope`, `frame_matches`, `RADIX_RULES`, `BUCKETS`, `bucket_samples`, `radix_share`, `cpu_metrics`, `percentile`, `slice_window`, `gc_stats`, `gc_ttft_correlation`, `summarize_requests`, `memory_role_summary` -- the exact names plan 02-08 will import.
- 11 tests pass (`python/tests/test_profile_analysis.py`), none marked `slow` (pure logic over synthetic fixtures, no process spawning). Full fast suite (`pytest python/tests -q -m "not slow"`) shows no regression: 79 passed (up from 74 before this plan), 36 skipped.
- Frame rules verified against the vendored source directly (`vendor/mini-sglang/python/minisgl/scheduler/cache.py`'s `match_req`/`cache_req`, `vendor/mini-sglang/python/minisgl/kvcache/radix_cache.py`'s `match_prefix`/`insert_prefix`/`evict`/`_tree_walk`) before writing the rules, not assumed from the plan text alone.
- `analysis.py` is standard-library only (`math`, `pathlib.Path`, `typing`, inline `json` for file loading) and contains zero `round(` calls, per the plan's explicit "rounding belongs to the markdown report only" constraint.

## Task Commits
1. **Task 1: Speedscope loading, frame rules, bucketing, radix share, CPU/GIL metrics**
   - `207e29a` test(02-05): add failing tests for speedscope loading, bucketing, radix share, cpu/gil metrics (RED)
   - `660fd41` feat(02-05): implement speedscope loading, frame bucketing, radix share, cpu/gil metrics (GREEN)
2. **Task 2: GC stats, GC-to-P99 correlation, memory and request summaries**
   - `b8d532c` test(02-05): add failing tests for gc stats, gc/ttft correlation, request/memory summaries (RED)
   - `a184351` feat(02-05): implement gc stats, gc/ttft correlation, request/memory summaries (GREEN)

**Plan metadata:** commit pending (this SUMMARY + REQUIREMENTS.md, written by the worktree executor per its orchestrator contract)

## Files Created/Modified
- `python/rsglang/profiling/analysis.py` - the analysis layer itself: `SpeedscopeError`, `load_speedscope`, frame-matching rules (`RADIX_RULES`, `BUCKETS`, `frame_matches`, `bucket_samples`), `radix_share`, `cpu_metrics`, `percentile`, `slice_window`, `gc_stats`, `gc_ttft_correlation`, `summarize_requests`, `memory_role_summary`. Standard library only.
- `python/tests/test_profile_analysis.py` - 11 tests across two task sections, with inline speedscope-fixture helpers (`_frame`, `_profile`, `_doc`) and a local `_FakeReq` dataclass for duck-typed request records.

## Decisions Made
- **gc_ttft_correlation test fixture uses n=60, not the literal "100 completed requests" described in the plan's `<behavior>` prose.** The plan's described scenario (99 requests tied at 10ms TTFT, 1 at 500ms, among 100 total) is mathematically unable to produce "exactly 1 spike at or above nearest-rank P99": for n=100 and p=99, `ceil(99/100*100)-1 = 98` (0-indexed), which is the *second-highest* order statistic -- with exactly one outlier, that index always lands in the tied "normal" group, never on the unique maximum. Since `>=` on that threshold matches both the rank-99 and rank-100 elements, at least 2 requests always qualify as "at or above P99" whenever the outlier is unique among 100 samples; "exactly 1" is unreachable under the formula fixed and verified by Task 1's own `test_percentile`. For `n<100`, `ceil(0.99*n)=n`, so the nearest-rank P99 *is* the max -- which is what the plan's intended behavior (single spike == the outlier) actually requires. I chose n=60 to preserve every other described assertion (GC pause strictly inside the spike's window, a second pause touching a non-spike request's `t_send` without counting, exclusion of a request lacking `t_first`, and the all-None edge case) while keeping the numbers internally consistent. This is a Rule 1 auto-fix (bug in the plan's own prescribed test data), not a change to any production-code contract -- `gc_ttft_correlation`'s implementation is unchanged from the `<action>` spec; only the test fixture's cardinality differs from the literal prose.
- Everything else implemented exactly as specified in `<action>`/`<interfaces>`: BUCKETS insertion order and fragment/name pairs, cpu_metrics' ticks/pct/share_of_active/per_request_ms formulas, percentile's nearest-rank formula, gc_stats' by_generation keys and events shape, summarize_requests' outcome-filtered TTFT percentiles, memory_role_summary's curve/summary shapes.

## Deviations from Plan
### Auto-fixed Issues

**1. [Rule 1 - Bug] gc_ttft_correlation's planned test data (n=100, 1 outlier) cannot satisfy its own "exactly 1 spike" assertion under the specified nearest-rank percentile formula**
- **Found during:** Task 2, writing the RED test for `test_gc_ttft_correlation`
- **Issue:** The plan's `<behavior>` bullet describes 100 completed requests (99 at 10ms TTFT, 1 at 500ms) and asserts the resulting P99 threshold equals 500.0 with exactly 1 spike. Proven above, this is mathematically impossible: nearest-rank P99 over 100 samples with a single outlier always selects the second-highest order statistic (a tied "normal" value), and `>=` against that threshold always matches at least 2 samples (itself and the true max).
- **Fix:** Constructed the test fixture with n=60 qualifying requests instead (1 outlier + 59 normal + 1 excluded-for-no-t_first), where `ceil(0.99*60)=60` lands nearest-rank P99 exactly on the unique maximum. All other described behaviors (overlap-inside, touching-boundary non-overlap, t_first exclusion, all-None edge case) are preserved with the same qualitative shape.
- **Files modified:** `python/tests/test_profile_analysis.py` (test fixture only; no production code affected)
- **Verification:** `pytest python/tests/test_profile_analysis.py::test_gc_ttft_correlation -q` passes; full suite shows no regression.
- **Commits:** `b8d532c` (RED), `a184351` (GREEN, though this function needed no further changes beyond the initial implementation matching the corrected test)

**Total deviations:** 1 auto-fixed. **Impact:** test-fixture-only; the `gc_ttft_correlation` function's behavior, signature, and output shape are exactly as specified in `<action>`/`<interfaces>` and unaffected by this fix.

## Issues Encountered
- `.venv` did not exist in this fresh worktree (expected, gitignored). Ran `bash scripts/bootstrap_mac_env.sh`, which created `.venv`, synced from `requirements-mac.txt`, and installed `rsglang`/`minisgl` in editable mode without issue.

## User Setup Required
None. This plan is pure logic over synthetic fixtures; no external services, GPU access, or real py-spy/hook.py output are needed.

## Next Phase Readiness
- Plan 02-08 (run orchestration) can now import every name listed in this plan's "Artifacts this phase produces" section directly from `rsglang.profiling.analysis` and feed it real speedscope files (from py-spy) and real hook.py JSONL records (from plan 02-02's `load_hook_records()`) and plan 02-06's `RequestRecord` instances, with no further changes to this module's public surface expected.
- `BUCKETS`' insertion order and the seven bucket names (`radix`, `ipc_zmq`, `serde`, `tokenize`, `detokenize`, `http_stack`, `api_handlers`) match the `REQUIRED_BUCKETS` contract plan 02-04's `sidecar.py` fixes independently in this same wave.
- No blockers identified for downstream plans in this phase.

---
*Phase: 02-python-frontend-baseline-profile*
*Completed: 2026-10-05*

## Self-Check: PASSED
- FOUND: python/rsglang/profiling/analysis.py
- FOUND: python/tests/test_profile_analysis.py
- FOUND commits: 207e29a, 660fd41, b8d532c, a184351 (all present in `git log --oneline -6`)
