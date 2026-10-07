---
phase: 07-frontend-benchmarks
plan: 03
subsystem: benchmarking
tags: [python, bench_simple, standard-throughput, D-12, BENCH-06]

requires:
  - phase: 07-02
    provides: GC-only profiling mode and the python/rsglang package layout this driver's test suite runs alongside
provides:
  - "python/rsglang/bench/standard_throughput.py: BENCH-06 standard-inference workload driver, CLI `python -m rsglang.bench.standard_throughput --port --out --seed --batch-size --max-input`, writes schema `rsglang.bench.standard_throughput/1` JSON"
  - "summarize(tics_lists) reproducing upstream's process_benchmark_results formulas (num_tokens, duration, throughput_tok_s/req_s, upstream percentile indexing)"
  - "ast-based drift guard pinning bench_simple.py's TEST_BS/MAX_INPUT/random.seed(42)/random.randint(16,1024) and client.py's benchmark_one_batch/generate_prompt/get_model_name/benchmark_one signatures, without importing client.py (openai/tqdm absent on the Mac)"
affects: ["07-09 (Rust orchestrator standard_throughput scenario runner)"]

actuals:
  tokens: 3515
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "Lazy-import-inside-run() pattern (matches rsglang.profiling.scenarios.run_s2 / T-02-11): the module imports cleanly on the Mac where openai/transformers/minisgl.benchmark are absent"
    - "Injected-runner CLI testing: main(argv, runner=fake) lets the CLI's argument parsing, JSON schema and atomic-write path be proven end to end without the GPU-box dependencies"
    - "ast-based drift guard against frozen vendored files: parses bench_simple.py/client.py with ast instead of importing them, so the guard works without openai/tqdm and never risks mutating vendor/"

key-files:
  created:
    - python/rsglang/bench/__init__.py
    - python/rsglang/bench/standard_throughput.py
    - python/tests/test_bench_simple_reuse.py
  modified: []

key-decisions:
  - "Fixed RNG draw order (seed, warm-up prompt, then batch prompt lengths/prompts, then batch output lengths) instead of bench_simple.py's own interleaved-with-an-asyncio-task order, so the workload is identical for both frontends at a given seed (needed for the A/B throughput comparison, not for upstream fidelity)"
  - "Atomic JSON write: refuse if --out is a symlink, write a sibling temp file opened with mode \"x\", then os.replace onto the target (T-07-08)"
  - "No --host flag; base URL is always http://127.0.0.1:<port>/v1 (T-02-11 precedent)"

patterns-established:
  - "Workload drivers that need upstream's GPU-box-only client (openai/transformers/minisgl.benchmark) keep those imports lazy inside their async run() function, never at module scope"

requirements-completed: [BENCH-06]

coverage:
  - id: D1
    description: "standard_throughput.py's CLI, JSON schema, and summarize() throughput formula, proven end to end via an injected async runner"
    requirement: "BENCH-06"
    verification:
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_tracer_main_writes_json"
        status: pass
    human_judgment: false
  - id: D2
    description: "ast-based drift guard pinning bench_simple.py constants/RNG calls and client.py helper signatures, plus summarize() percentile/edge-case formulas"
    requirement: "BENCH-06"
    verification:
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_bench_simple_constants_match"
        status: pass
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_client_helper_signatures_match"
        status: pass
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_summarize_upstream_indexing"
        status: pass
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_summarize_rejects_empty_and_zero_duration"
        status: pass
      - kind: unit
        ref: "python/tests/test_bench_simple_reuse.py#test_main_failure_and_symlink_refusal"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-10-06
status: complete
---

# Phase 07 Plan 03: Standard-Throughput Workload Driver Summary

**BENCH-06's bench_simple-shaped standard-throughput driver (D-12): a CLI that reuses upstream's own `minisgl.benchmark.client` helpers and `process_benchmark_results` formulas, writing a schema-tagged JSON trial for the Rust orchestrator instead of just logging stats.**

## Performance
- **Duration:** ~25min
- **Tasks:** 2
- **Files modified:** 3 (all created)

## Accomplishments
- `python/rsglang/bench/standard_throughput.py`: constants (`SCHEMA`, `BATCH_SIZE`, `MAX_INPUT`, `OUTPUT_MIN`, `OUTPUT_MAX`, `SEED`, `WARMUP_INPUT`, `WARMUP_OUTPUT`), `summarize()`, `async run()`, `main()`, all matching the plan's interface contract exactly
- `summarize()` reproduces upstream's `process_benchmark_results` formulas bit-for-bit: `num_tokens = sum(len(tics))`, `duration = max(all tics) - min(all tics)`, `throughput_tok_s`/`throughput_req_s`, and upstream's `sorted[int(len*q)]` percentile indexing (not nearest-rank)
- CLI takes `--port --out --seed --batch-size --max-input`, deliberately no `--host` flag (T-02-11); writes the result JSON atomically, refusing symlinked `--out` targets (T-07-08)
- An ast-based drift guard parses `vendor/mini-sglang/benchmark/online/bench_simple.py` and `vendor/mini-sglang/python/minisgl/benchmark/client.py` without importing them (client.py needs `openai`/`tqdm`, absent on the Mac), pinning `TEST_BS == [64]`, `MAX_INPUT == 8192`, the `random.seed(42)` and `random.randint(16, 1024)` calls, and the `benchmark_one_batch`/`generate_prompt`/`get_model_name`/`benchmark_one` signatures
- Module imports cleanly on the Mac (`openai` absent) via the lazy-import-inside-`run()` pattern already established by `rsglang.profiling.scenarios.run_s2`

## Task Commits
1. **Task 1: Tracer — main() with an injected runner writes a schema-tagged JSON whose throughput follows upstream's formula** - `9094df1` (feat)
2. **Task 2: Drift guard against bench_simple.py and client.py, plus summarize percentile and edge tests** - `ac98961` (test)

**Plan metadata:** pending (this commit, docs: complete plan)

## Files Created/Modified
- `python/rsglang/bench/__init__.py` - one-line package docstring
- `python/rsglang/bench/standard_throughput.py` - BENCH-06 driver: constants, `summarize()`, `run()`, `main()`
- `python/tests/test_bench_simple_reuse.py` - tracer test plus the Task 2 drift guard, percentile, and edge-case tests

## Decisions Made
See `key-decisions` in frontmatter: fixed (non-interleaved) RNG draw order for A/B parity; atomic symlink-refusing JSON write; no `--host` flag.

## Deviations from Plan

None - plan executed exactly as written.

## TDD Gate Compliance

Task 2 (`tdd="true"`) was written test-first per the plan's `<behavior>` spec: `test_bench_simple_constants_match`, `test_client_helper_signatures_match`, `test_summarize_upstream_indexing`, `test_summarize_rejects_empty_and_zero_duration`, `test_main_failure_and_symlink_refusal` were all added in one commit and run immediately.

**Note on the RED phase:** all five new tests passed on the first run, with zero changes needed to `standard_throughput.py`. This was investigated per the TDD fail-fast rule ("unexpected GREEN in RED phase") and found to be the expected outcome, not a bug: Task 1 (this same plan, prior task, same session) had already implemented `standard_throughput.py` directly against the plan's pre-extracted `<interfaces>` contract — the same constants, formulas, and upstream signatures that Task 2's drift guard pins. There was no gap between "what Task 1 built" and "what Task 2 locks down" for these tests to expose. No separate `feat`/`refactor` commit was made because no implementation change was required; `git diff --exit-code vendor/` confirmed the frozen vendored files were untouched, and the full `python/tests` suite (187 passed, 37 skipped — pre-existing GPU-only skips, out of scope) passed afterward.

## Issues Encountered

None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness

`python/rsglang/bench/standard_throughput.py` is ready for plan 07-09's Rust orchestrator to invoke per A/B arm: it checks `schema == "rsglang.bench.standard_throughput/1"` and reads `summary.throughput_tok_s`. No blockers.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-06*

## Self-Check: PASSED

All created files found on disk; both task commits (`9094df1`, `ac98961`) found in `git log`.
