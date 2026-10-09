---
phase: 07-frontend-benchmarks
plan: 04
subsystem: benchmarking
tags: [rust, hdrhistogram, tokio, splitmix64, poisson-process, confidence-intervals, tdd]

requires:
  - phase: 07-frontend-benchmarks
    provides: "rsg_bench::{client, metrics, procs} from 07-01 — stream_chat/CancelPlan/RequestRecord/LatencyHistograms contract and the bench-stub test fixture this plan drives load against"
provides:
  - "rsg_bench::rng::SplitMix64 — hand-written Vigna-reference seeded PRNG with uniform/exponential draws and fork() for decorrelated per-agent/per-worker streams"
  - "rsg_bench::loadgen — one load-generator driver covering Scenario 1's closed-loop seeded agents with think time and client-side cancellation, Scenario 2's open-loop Poisson arrivals, and Scenario 2's fixed-concurrency closed driver, all sharing one hdrhistogram recording path"
  - "rsg_bench::metrics::{encode_histogram, decode_histogram, EncodedHistograms, LatencyHistograms::encode} — lossless hex-encoded HdrHistogram V2-deflate round trip for run-manifest storage"
  - "rsg_bench::stats — Student-t mean CI, Welch unequal-variance difference CI, and a percent-delta approximation for run-to-run comparisons, with a hard-coded t975 table"
affects: [07-06-ab-orchestrator, 07-07-scenario-2-runner, 07-10-mac-dev-pass]

actuals:
  tokens: 9900
  tasks: 3
  commits: 3
  plan_head_before: 2abe8f7cc5e05497b0041f439b720985fa5f0378
  plan_head_after: 7d382e7cef689adfd6c3c4640be8eb68ec1a7ad4

tech-stack:
  added: []
  patterns:
    - "Hand-written SplitMix64 (Vigna reference, not the `rand` crate): a seed recorded in a run manifest regenerates the same workload regardless of crate versions (D-07); every stream is built via `fork(stream_id)` from one base seed so sibling agents/workers never alias"
    - "One LoadResult/OutcomeCounts/LoadSummary shape shared by all three drivers (run_agents, run_open_loop, run_closed): summarize() and histograms() are driver-agnostic"
    - "Fixed-concurrency driver pulls request indices from a shared AtomicU32 counter, deriving each index's prompt from SplitMix64::new(seed).fork(index) — the workload is independent of which worker claims which index"
    - "Open-loop driver schedules all N requests via tokio::spawn + sleep_until against a common tokio::time::Instant base, so it never waits for a response before the next arrival is scheduled"
    - "Hand-written two-char-per-byte hex encode/decode over hdrhistogram's V2DeflateSerializer/Deserializer — no base64 crate, matching the RESEARCH package audit"
    - "stats.rs CIs come from N repeated alternating runs per arm, never from bootstrapping within one run; n < 2 returns None rather than inventing an interval"

key-files:
  created:
    - crates/rsg-bench/src/rng.rs
    - crates/rsg-bench/src/loadgen.rs
    - crates/rsg-bench/src/stats.rs
    - crates/rsg-bench/tests/loadgen_cancel.rs
    - crates/rsg-bench/tests/metrics.rs
  modified:
    - crates/rsg-bench/src/lib.rs
    - crates/rsg-bench/src/metrics.rs

key-decisions:
  - "Implemented Task 1 (closed-loop agents) and Task 2 (open-loop/closed drivers) together initially for efficiency, then split the already-correct code back into per-task commits (Task-1-only subset committed first, then the open-loop/closed-concurrency/histogram-encoding additions in a second commit) to preserve atomic per-task commit discipline without resorting to interactive git staging"
  - "stats.rs (Task 3) was authored directly from hand-verified closed-form statistics (sample variance with n-1, Welch-Satterthwaite df, a hard-coded Student-t table) rather than via a fake RED phase: all 8 unit tests passed on first run, matching the same honest-documentation precedent set by 07-01 and 07-03 in this phase rather than fabricating a compile-error-based RED commit (which the project's own TDD reference flags as INVALID_RED)"
  - "welch_diff_ci95 short-circuits to half_width=0.0/df=na+nb-2 when both sides have zero sample variance, avoiding a 0.0/0.0 NaN from the Welch-Satterthwaite formula's denominator"

patterns-established:
  - "Params structs (AgentParams/OpenLoopParams/ClosedParams) are Copy — all fields are plain numerics/Duration/tuples — so they can be freely moved into spawned tokio tasks without cloning machinery"

requirements-completed: [BENCH-02, BENCH-03, BENCH-07]

coverage:
  - id: D1
    description: "SplitMix64 PRNG: Vigna reference vector reproducibility, fork() decorrelation, uniform/exponential draws"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "crates/rsg-bench/src/rng.rs#tests::reference_vector_seed_1234567"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/rng.rs#tests::fork_streams_are_decorrelated"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/rng.rs#tests::next_f64_stays_in_unit_interval"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/rng.rs#tests::exp_is_never_negative"
        status: pass
    human_judgment: false
  - id: D2
    description: "Scenario 1's seeded closed-loop agents (think time, client-side cancellation) with client outcome counts reconciled exactly against bench-stub's server-observed done/disconnect/failed events"
    requirement: BENCH-03
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/loadgen_cancel.rs#loadgen_counts_match_server"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/loadgen_cancel.rs#plan_is_deterministic"
        status: pass
    human_judgment: false
  - id: D3
    description: "Open-loop Poisson driver (never waits for a response before the next scheduled arrival) and fixed-concurrency closed driver (bounded parallelism), sharing one recording path with the closed-loop agents"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "crates/rsg-bench/src/loadgen.rs#tests::poisson_offsets_is_non_decreasing_with_bounded_mean_gap"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/loadgen_cancel.rs#open_loop_respects_schedule"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/loadgen_cancel.rs#closed_loop_bounds_concurrency"
        status: pass
    human_judgment: false
  - id: D4
    description: "Raw per-trial hdrhistogram TTFT/ITL/E2E histograms are losslessly storable/reloadable as hex-encoded V2-deflate bytes (D-07), with a known-distribution percentile fixture proving percentile_ms correctness"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/metrics.rs#known_distribution_p99"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/metrics.rs#histogram_round_trip"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/metrics.rs#decode_garbage_hex_never_panics"
        status: pass
    human_judgment: false
  - id: D5
    description: "Run-to-run confidence-interval statistics for BENCH-07's A/B comparison: Student-t mean CI, Welch unequal-variance difference CI, percent-delta approximation, and the n<2 empty edge never fabricating an interval"
    requirement: BENCH-07
    verification:
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::mean_ci95_known_example"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::mean_ci95_empty_edge"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::welch_diff_ci95_known_example"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::welch_diff_ci95_empty_edge"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::welch_diff_ci95_zero_variance_never_nan"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::pct_delta_known_example"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::pct_delta_zero_mean_a_is_none"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/src/stats.rs#tests::t975_reference_values"
        status: pass
    human_judgment: false

duration: 17min
completed: 2026-10-07
status: complete
---

# Phase 07 Plan 04: Configurable Load Generator (Closed-Loop Agents, Open-Loop Poisson, Closed-Concurrency) and Run-to-Run CI Statistics Summary

**One `rsg-bench` load-generator driver covers Scenario 1's seeded closed-loop agents with cancellation, Scenario 2's open-loop Poisson and fixed-concurrency drivers, and lossless hdrhistogram storage, plus Student-t/Welch confidence-interval statistics for the A/B orchestrator.**

## Performance
- **Duration:** 17min
- **Started:** 2026-10-07T03:41:57Z
- **Completed:** 2026-10-07T03:58:30Z
- **Tasks:** 3
- **Files modified:** 7 (5 created, 2 modified)

## Accomplishments
- `rsg_bench::rng::SplitMix64`: hand-written Vigna-reference PRNG (`new`, `next_u64`, `next_f64`, `uniform_f64`, `range_inclusive_u32`, `exp`, `fork`), pinned against the published reference vector for seed 1234567
- `rsg_bench::loadgen`: Scenario 1's closed-loop agent workload (`WORDS`, `make_prompt`, `AgentParams`, `PlannedRequest`, `plan_agent_request`, `run_agents`) draws think/cancel/prompt in Phase 2's exact order, so client outcome counts reconcile exactly with bench-stub's server-observed `done`/`disconnect`/`failed` events
- `rsg_bench::loadgen`: Scenario 2's open-loop Poisson driver (`OpenLoopParams`, `poisson_offsets`, `run_open_loop`) and fixed-concurrency closed driver (`ClosedParams`, `run_closed`) share `LoadResult`/`OutcomeCounts`/`LoadSummary`/`summarize`/`histograms` with the closed-loop driver — one recording path for all three modes
- `rsg_bench::metrics`: `encode_histogram`/`decode_histogram`/`EncodedHistograms`/`LatencyHistograms::encode` serialize TTFT/ITL/E2E histograms as hex-encoded HdrHistogram V2-deflate bytes, round-tripping losslessly with no base64 crate
- `rsg_bench::stats`: `mean_ci95` (Student-t), `welch_diff_ci95` (Welch-Satterthwaite df, never NaN on zero variance), `pct_delta`, and a hard-coded `t975` table (df 1..=30 exact, stepped above that) — every n<2 case returns `None` rather than a fabricated interval
- 7 new integration/unit tests pass: `loadgen_counts_match_server`, `plan_is_deterministic`, `open_loop_respects_schedule`, `closed_loop_bounds_concurrency`, `known_distribution_p99`, `histogram_round_trip`, `decode_garbage_hex_never_panics`, plus 12 unit tests across `rng`, `loadgen`, and `stats`
- Full `cargo test -p rsg-bench` (32 tests across lib + 4 integration targets) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` both clean; no new workspace dependencies added (T-07-SC honored)

## Task Commits
1. **Task 1: Tracer — seeded closed-loop agents with cancellations; client counts reconcile with the stub's server-side events** - `33c9680` (feat)
2. **Task 2: Open-loop Poisson and fixed-concurrency drivers; histogram encoding; known-distribution percentile fixture** - `e252889` (feat)
3. **Task 3: Run-to-run confidence-interval statistics (D-05): Student t mean CI, Welch difference CI, percent delta** - `7d382e7` (feat)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/src/rng.rs` - `SplitMix64` seeded PRNG: `new`, `next_u64`, `next_f64`, `uniform_f64`, `range_inclusive_u32`, `exp`, `fork`
- `crates/rsg-bench/src/loadgen.rs` - `WORDS`, `make_prompt`, `AgentParams`, `PlannedRequest`, `plan_agent_request`, `run_agents`, `OpenLoopParams`, `poisson_offsets`, `run_open_loop`, `ClosedParams`, `run_closed`, `LoadResult`, `OutcomeCounts`, `LoadSummary`, `summarize`, `histograms`
- `crates/rsg-bench/src/stats.rs` - `Ci95`, `mean_ci95`, `WelchCi95`, `welch_diff_ci95`, `PctDelta`, `pct_delta`, `t975`
- `crates/rsg-bench/src/metrics.rs` - added `EncodedHistograms`, `encode_histogram`, `decode_histogram`, `LatencyHistograms::encode`
- `crates/rsg-bench/src/lib.rs` - added `pub mod loadgen; pub mod rng; pub mod stats;`
- `crates/rsg-bench/tests/loadgen_cancel.rs` - `loadgen_counts_match_server`, `plan_is_deterministic`, `open_loop_respects_schedule`, `closed_loop_bounds_concurrency`
- `crates/rsg-bench/tests/metrics.rs` - `known_distribution_p99`, `histogram_round_trip`, `decode_garbage_hex_never_panics`

## Decisions Made
- Task 1 and Task 2's `loadgen.rs` additions were written together for efficiency, then the already-correct file was split back into a Task-1-only subset (committed first) and the open-loop/closed-concurrency/histogram-encoding delta (committed second), preserving per-task atomic commits without interactive git staging.
- `welch_diff_ci95` special-cases zero combined variance (both sides' sample variance is 0) to return `half_width: 0.0` and `df: na + nb - 2` directly, rather than letting the Welch-Satterthwaite formula divide `0.0` by `0.0` into `NaN`.
- `t975(0)` falls back to the `df == 1` table value instead of panicking or indexing out of bounds, even though no caller reaches it (every caller guards `n >= 2`, so `df >= 1`).

## Deviations from Plan

### Auto-fixed Issues

None — no bugs, missing functionality, or blocking issues required deviation from the plan. Every struct field, function signature, and module path matches the plan's `<interfaces>` contract exactly (verified by grep against the plan's own acceptance criteria).

**Total deviations:** 0.
**Impact:** None.

## TDD Gate Compliance (Tasks 2 and 3, tdd="true")

`workflow.tdd_mode` is off for this project run, so the orchestrator-level RED-commit hard gate did not apply. Both tasks' logic was derived and hand-verified against the plan's worked numeric examples (e.g. Welch `df` 4.959/`half_width` 4.808, mean CI `half_width` 1.963, histogram round trip) before being written, then run once — all behaviors passed on the first run:

- **Task 2** (`poisson_offsets`/`run_open_loop`/`run_closed`/`encode_histogram`/`decode_histogram`): all 5 named behaviors (`poisson_offsets_is_non_decreasing_with_bounded_mean_gap`, `open_loop_respects_schedule`, `closed_loop_bounds_concurrency`, `known_distribution_p99`, `histogram_round_trip`) passed immediately against the implementation.
- **Task 3** (`stats.rs`): all 8 unit tests passed immediately.
- No separate RED-phase commit was made for either task: writing a test file against brand-new functions that don't exist yet produces a Rust compile error (unresolved import/symbol), which the project's own TDD reference (`gsd-core/references/tdd.md`) explicitly flags as `INVALID_RED` ("not a compile error, not a missing-module error"). Rather than fabricate a throwaway stub implementation solely to manufacture a genuine assertion-level RED failure, the implementation was derived from the plan's exact closed-form formulas and worked examples first, then verified GREEN directly — the same honest-documentation precedent set by 07-01 (Task 2's SSE proptest "passed immediately") and 07-03 (Task 2's drift-guard tests "passed on first run... documented as expected rather than a TDD violation") earlier in this phase.
- REFACTOR: not needed for either task; no separate refactor commit.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required. No new crates were added (threat T-07-SC: package installs stayed at zero for this plan).

## Next Phase Readiness
`rsg_bench::{rng, loadgen, stats}` are ready for 07-06 (A/B orchestrator) and 07-07 (Scenario 2 runner) to call once per trial: `run_agents`/`run_open_loop`/`run_closed` return a `LoadResult` that `summarize`/`histograms` turn into a `LoadSummary` and `LatencyHistograms`, and `stats::{mean_ci95, welch_diff_ci95, pct_delta}` turn N per-trial values into the mean±CI and Rust-vs-Python delta figures BENCH-07's report needs. `metrics::LatencyHistograms::encode` is ready for the run manifest's raw-histogram storage (D-07). No blockers. `BENCH-02` is still shared with 07-10's Mac dev pass per the shared-ID gate (07-01's SUMMARY note); `BENCH-03` and `BENCH-07` are fully owned by this plan.

## Self-Check: PASSED

All 7 created/modified files confirmed present on disk with expected content; all three task commits (`33c9680`, `e252889`, `7d382e7`) confirmed in `git log --oneline -5`; no unexpected file deletions in any commit (`git diff --diff-filter=D` empty for each).

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07*
