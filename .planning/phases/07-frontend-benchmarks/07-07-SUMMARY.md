---
phase: 07-frontend-benchmarks
plan: 07
subsystem: benchmarking
tags: [rust, clap, loadgen, saturation-curve, num-tokenizer-sweep, cross-check, json-parsing, tdd]

requires:
  - phase: 07-frontend-benchmarks
    provides: "07-06's shared orchestrator (TrialRunner, run_session, SessionArgs/SessionConfig, Arm), D-07 manifest, and loadgen::{run_open_loop, run_closed, OpenLoopParams, ClosedParams, summarize, histograms} -- this plan adds only its own TrialRunners and CLI subcommands on top (D-08)"
provides:
  - "rsg_bench::scenarios::s2_saturation::{LoopMode, S2Args, CurvePoint, validate_levels, build_curve, peak_rps, S2Runner} -- Scenario 2's open/closed-loop RPS-vs-latency saturation curve (BENCH-04, D-01)"
  - "rsg_bench::scenarios::sweep::{SweepArgs, validate_candidates, pick_best, sweep_arms} -- D-09's --num-tokenizer pre-pass sweep, picking the candidate with the highest peak RPS"
  - "rsg_bench::scenarios::crosscheck::{Tool, CrossArgs, VLLM_TEMPLATE, SGLANG_TEMPLATE, CrossCheckResult, parse_tool_result, CrossRunner} -- D-04's key-tolerant third-party cross-check against vllm bench serve / sglang.benchmark.serving"
  - "Three new rsg-bench subcommands: s2, sweep-num-tokenizer, crosscheck"
affects: [07-08-s3-coldstart, 07-09-standard-throughput-regression, 07-10-bench-06-report]

actuals:
  tokens: 13604
  tasks: 3
  commits: 5
plan_head_before: 087368c98921aa899e44cbc51cd705d77556fbc8
plan_head_after: 000f03748feff0e3bf1185be884d5ba0cae844fd

tech-stack:
  added: []
  patterns:
    - "S2Runner pushes one MeasuredWindow per load level with that level's own start/end timestamps and records; the orchestrator's existing per-window GC/memory/co-occurrence wiring (07-06, D-08) therefore applies per level for free -- BENCH-08's P99-vs-GC comparison is never pooled across load levels, with zero new orchestrator code"
    - "parse_tool_result (T-07-17) tries the whole trimmed document as one JSON value first (vllm's single-object --save-result shape), and only on that failing falls back to the last non-empty line (sglang's progress-then-final JSONL shape) -- one function handles both tools' shapes without a tool-specific branch"
    - "run_cross_tool spawns the external tool with stdout discarded and stderr drained on a dedicated background thread (not the main polling loop), so a chatty tool can never deadlock on a full pipe buffer while the harness is busy polling try_wait() for the timeout/kill decision"
    - "sweep-num-tokenizer overrides SessionConfig.arms after SessionConfig::from_args (bypassing build_arms' python-default/rust/python-best shape entirely) -- D-10's 'no Rust sweep' falls out structurally: sweep_arms only ever constructs FrontendKind::Python arms, and the rust_cmd template is simply never rendered"
    - "validate_cross_args runs before SessionConfig::from_args/run_session, so a missing --vllm-bin/--sglang-python/--tool-cmd is a same-process, no-subprocess-spawned exit 2 -- never a half-started session with a server launched and torn down for nothing"

key-files:
  created:
    - crates/rsg-bench/src/scenarios/s2_saturation.rs
    - crates/rsg-bench/src/scenarios/sweep.rs
    - crates/rsg-bench/src/scenarios/crosscheck.rs
    - crates/rsg-bench/tests/s2_curve.rs
    - crates/rsg-bench/tests/sweep.rs
    - crates/rsg-bench/tests/crosscheck_parse.rs
    - crates/rsg-bench/tests/fixtures/vllm_result.json
    - crates/rsg-bench/tests/fixtures/sglang_result.jsonl
    - crates/rsg-bench/tests/fixtures/fake_bench_tool.sh
  modified:
    - crates/rsg-bench/src/main.rs
    - crates/rsg-bench/src/scenarios/mod.rs
    - crates/rsg-bench/src/loadgen.rs
    - crates/rsg-bench/src/metrics.rs

key-decisions:
  - "OutcomeCounts (loadgen.rs) and Percentiles/LatencySummary (metrics.rs) gained Deserialize alongside their existing Serialize, matching 07-06's own precedent for manifest-embedded types -- CurvePoint's interface contract states it is Serialize+Deserialize, and it embeds both directly"
  - "CurvePoint.offered is the raw level as f64 even in closed mode (where the level is really a u32 concurrency); the label itself (concurrency=4, not concurrency=4.0) carries the integer-looking presentation, while offered stays one consistent numeric field across both loop modes for 07-09's report code to sort/plot on"
  - "The level-to-seed mapping (ctx.seed.wrapping_add(i as u64), i = ascending index after validate_levels sorts) is applied identically regardless of loop mode, so a given load level draws the same prompts across arms for a fair comparison (P1), matching the plan's exact instruction"
  - "crosscheck's CrossRunner is HarnessManaged like every other scenario so far: the orchestrator launches/tears down the arm's server exactly as for s1/s2, and the cross-check tool is simply another subprocess spawned during run_trial against that already-running server"

patterns-established:
  - "TDD discipline on sweep.rs and crosscheck.rs (Tasks 2/3) used a genuine RED phase this time, unlike 07-06's TDD-discipline-collapse precedent: pick_best/validate_candidates were committed as intentionally-wrong stubs (pick_best always returns the first/zero entry; validate_candidates skips validation and sorting; parse_tool_result always returns empty metrics and never rejects anything), the tests were run and confirmed to fail on real assertions (not compile errors), then fixed to GREEN in a separate commit. See 'TDD Gate Compliance' below."

requirements-completed: []  # BENCH-04/BENCH-07 are each shared with other 07-xx plans; requirements.ready-ids reports 0/2 ready -- both stay open in REQUIREMENTS.md

coverage:
  - id: D1
    description: "rsg-bench s2 runs Scenario 2 (BENCH-04): 32-token short-prompt saturation, sweeping every offered load level (open-loop Poisson rate or closed-loop fixed concurrency, D-01) in ascending order on one server launch, each level its own MeasuredWindow and CurvePoint, with peak_rps as the trial-level maximum"
    requirement: BENCH-04
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/s2_curve.rs#s2_open_loop_curve_with_stub_arms"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/s2_curve.rs#closed_mode_curve"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/s2_curve.rs#validate_levels_rules"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/s2_curve.rs#build_curve_sorted_stable"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/s2_curve.rs#peak_rps_ignores_none"
        status: pass
    human_judgment: false
  - id: D2
    description: "rsg-bench sweep-num-tokenizer (D-09, BENCH-07) runs one S2 trial per candidate on the Python frontend only, picks the highest-peak-RPS candidate (ties toward the smallest), ignores failed candidates, errors non-zero if every candidate fails, and prints best_num_tokenizer=<K> as the final stdout line"
    requirement: BENCH-07
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/sweep.rs#pick_best_ties_to_smallest"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/sweep.rs#pick_best_ignores_failed"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/sweep.rs#pick_best_all_failed_errors"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/sweep.rs#validate_candidates_rules"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/sweep.rs#sweep_end_to_end_prints_best"
        status: pass
    human_judgment: false
  - id: D3
    description: "rsg-bench crosscheck (D-04) runs vllm bench serve or sglang.benchmark.serving against a freshly launched arm, requires an explicit --vllm-bin/--sglang-python/--tool-cmd path (never installs anything, T-07-16), parses the tool's result file key-tolerantly (T-07-17: whole-document JSON first, last-JSONL-line fallback, numeric ttft/itl/tpot/e2e_ms fields plus the three named throughput/completed fields), and records a failed tool run as a failed trial rather than crashing"
    requirement: BENCH-04
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#parse_vllm_fixture"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#parse_sglang_jsonl_last_line"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#parse_rejects_no_ttft"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#parse_rejects_non_object"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#crosscheck_session_with_fake_tool"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#crosscheck_requires_tool_path"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/crosscheck_parse.rs#crosscheck_tool_failure_recorded"
        status: pass
    human_judgment: false
  - id: D4
    description: "Default VLLM_TEMPLATE/SGLANG_TEMPLATE flag names (RESEARCH A3) come from docs.vllm.ai v0.22.1 / CLAUDE.md and are [ASSUMED], not re-verified against a real installed vllm/sglang on the GPU box -- both templates can be overridden with --tool-cmd, and the parser is key-tolerant, so flag drift needs a CLI override, not a code change"
    verification: []
    human_judgment: true
    rationale: "No vllm or sglang installation exists in this Mac dev environment (and the harness deliberately never installs either, T-07-16/T-07-SC), so the default templates' exact flag names cannot be exercised against real tool output here. 07-10's GPU-box human check is the point where this assumption gets confirmed or a --tool-cmd override is needed."

duration: 40min
completed: 2026-10-06
status: complete
---

# Phase 07 Plan 07: Scenario 2 Saturation Curve, Num-Tokenizer Sweep and Third-Party Cross-Checks Summary

**The Scenario 2 RPS-vs-latency saturation curve (open/closed loop) added to the shared orchestrator, alongside D-09's `--num-tokenizer` sweep pre-pass and D-04's key-tolerant `vllm`/`sglang` cross-check parsing -- three new `rsg-bench` subcommands, all reusing 07-06's shared launch/teardown/manifest/GC-memory wiring with zero orchestrator changes.**

## Performance
- **Duration:** ~40min
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 13 (9 created, 4 modified)

## Accomplishments
- `s2_saturation::{LoopMode, S2Args, CurvePoint, validate_levels, build_curve, peak_rps, S2Runner}`: Scenario 2's RPS-vs-latency saturation curve (BENCH-04, D-01). `S2Runner::run_trial` walks every validated load level ascending (`--mode open` Poisson rate via `loadgen::run_open_loop`, or `--mode closed` fixed concurrency via `loadgen::run_closed`), each level its own `MeasuredWindow` (label `rate=<r>` / `concurrency=<c>`) and `CurvePoint` built from `loadgen::summarize`. `validate_levels` rejects empty/`<=0`/`NaN`/duplicate levels and sorts ascending; `build_curve` stable-sorts by offered load; `peak_rps` is the curve's maximum achieved RPS, ignoring levels with no achieved RPS.
- `sweep::{SweepArgs, validate_candidates, pick_best, sweep_arms}`: D-09's `--num-tokenizer` pre-pass (BENCH-07). `sweep-num-tokenizer` runs one S2 trial per candidate on Python-only arms (`python-nt<K>`, ascending, D-10's "no Rust sweep" falls out structurally since `sweep_arms` only ever builds `FrontendKind::Python` arms), then `pick_best` picks the highest `peak_rps` (ties toward the smallest candidate), ignoring failed candidates, erroring if every candidate failed. The final stdout line is `best_num_tokenizer=<K>`.
- `crosscheck::{Tool, CrossArgs, VLLM_TEMPLATE, SGLANG_TEMPLATE, CrossCheckResult, parse_tool_result, CrossRunner}`: D-04's third-party cross-checks. `CrossRunner` renders the tool's argv (default templates or `--tool-cmd`), spawns it against the already-launched arm with a kill-on-timeout, background-stderr-drain subprocess runner, then parses its result file with `parse_tool_result` (T-07-17: whole-document JSON first, last-non-empty-JSONL-line fallback; keeps numeric `*ttft*_ms`/`*itl*_ms`/`*tpot*_ms`/`*e2e*_ms` fields plus `request_throughput`/`output_throughput`/`completed`; errors on a missing TTFT field or a non-object document, never a crash). `validate_cross_args` requires `--vllm-bin`/`--sglang-python`/`--tool-cmd` for the chosen tool *before* any server launch (exit 2, T-07-16: the harness never installs either tool).
- Three new `main.rs` subcommands: `s2`, `sweep-num-tokenizer`, `crosscheck` -- each only adding its own `TrialRunner` and CLI flags per D-08; alternation, launch/teardown, the D-07 manifest, secret redaction and BENCH-08's per-window GC/memory wiring are all 07-06's shared code, untouched.
- `OutcomeCounts`/`Percentiles`/`LatencySummary` gained `Deserialize` alongside `Serialize` (matching 07-06's precedent), since `CurvePoint`'s own `Serialize`+`Deserialize` contract embeds them directly.
- All 17 new tests pass across `s2_curve.rs` (5), `sweep.rs` (5) and `crosscheck_parse.rs` (7); the full `cargo test -p rsg-bench` (41 lib tests + 12 integration targets) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` are both clean; `cargo build --workspace` is unaffected.

## Task Commits
1. **Task 1: Tracer -- rsg-bench s2 sweeps load levels per trial and records one curve point and window per level** - `f9bbd41` (feat)
2. **Task 2a: RED -- failing tests for D-09 num-tokenizer sweep** - `950d82f` (test)
2. **Task 2b: GREEN -- implement validate_candidates/pick_best** - `c4d57c2` (feat)
3. **Task 3a: RED -- failing tests for D-04 cross-check parsing** - `1c67a37` (test)
3. **Task 3b: GREEN -- implement parse_tool_result** - `000f037` (feat)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/src/scenarios/s2_saturation.rs` - `LoopMode`, `S2Args`, `CurvePoint`, `validate_levels`, `build_curve`, `peak_rps`, `S2Runner`
- `crates/rsg-bench/src/scenarios/sweep.rs` - `SweepArgs`, `validate_candidates`, `pick_best`, `sweep_arms`
- `crates/rsg-bench/src/scenarios/crosscheck.rs` - `Tool`, `CrossArgs`, `VLLM_TEMPLATE`, `SGLANG_TEMPLATE`, `CrossCheckResult`, `parse_tool_result`, `CrossRunner`, `validate_cross_args`
- `crates/rsg-bench/src/main.rs` - the `s2`, `sweep-num-tokenizer` and `crosscheck` subcommands, plus `EXIT_SWEEP_NO_CANDIDATE`/`EXIT_BAD_USAGE`
- `crates/rsg-bench/src/scenarios/mod.rs` - added `pub mod s2_saturation; pub mod sweep; pub mod crosscheck;`
- `crates/rsg-bench/src/loadgen.rs` - `OutcomeCounts` gained `Deserialize`
- `crates/rsg-bench/src/metrics.rs` - `Percentiles`/`LatencySummary` gained `Deserialize`
- `crates/rsg-bench/tests/s2_curve.rs` - 5 tests: open/closed-loop curve end-to-end, `validate_levels`/`build_curve`/`peak_rps` edge rules
- `crates/rsg-bench/tests/sweep.rs` - 5 tests: `pick_best`/`validate_candidates` edge rules, sweep end-to-end
- `crates/rsg-bench/tests/crosscheck_parse.rs` - 7 tests: `parse_tool_result` fixture/rejection rules, crosscheck end-to-end (fake tool, missing-path, tool-failure)
- `crates/rsg-bench/tests/fixtures/{vllm_result.json, sglang_result.jsonl, fake_bench_tool.sh}` - parse fixtures and a fake third-party tool script

## Decisions Made
See `key-decisions` in the frontmatter above.

## TDD Gate Compliance

Tasks 2 and 3 both carry `tdd="true"`. `workflow.tdd_mode` is off for this project, so the orchestrator-level RED-commit hard gate did not apply, but the full RED-GREEN-REFACTOR discipline was followed for both.

**Task 2 (`sweep.rs`):** `tests/sweep.rs` was written first, against an intentionally-wrong `sweep.rs` (`pick_best` always returns `0`; `validate_candidates` performs no validation or sorting at all) and the already-correct `main.rs`/`scenarios/mod.rs` wiring needed just to compile. `cargo test -p rsg-bench --test sweep` was run and confirmed a genuine RED: all 5 tests failed on real assertions (wrong tie-break winner, wrong failure-ignoring result, missing error on all-failed, missing validation errors, wrong arm schedule order) -- never a compile error. That RED state was committed (`950d82f`). `validate_candidates`/`pick_best` were then implemented correctly; all 5 tests passed (GREEN, `c4d57c2`). No refactor commit was needed (the first correct implementation needed no cleanup).

**Task 3 (`crosscheck.rs`):** `tests/crosscheck_parse.rs` plus its three fixtures were written first, against an intentionally-wrong `parse_tool_result` (always returns empty metrics, never rejects anything) alongside the correctly-implemented `CrossRunner`/`validate_cross_args`/CLI wiring needed to compile and run the end-to-end tests. Running the suite confirmed a genuine RED: 5 of 7 tests failed on real assertions (empty metrics instead of the fixture's values, no error where one was expected); the 2 infra-only tests (`crosscheck_requires_tool_path`, `crosscheck_tool_failure_recorded`) legitimately passed already, since that plumbing didn't depend on parsing logic. That RED state was committed (`1c67a37`). `parse_tool_result` was then implemented correctly; all 7 tests passed (GREEN, `000f037`), after also fixing 5 pre-existing `clippy::unnecessary_get_then_check` lints the test file's own assertions triggered (`get(...).is_none()` → `!contains_key(...)`), unrelated to the RED/GREEN cycle itself but caught by the same gate run.

Unlike 07-06's TDD-discipline-collapse precedent (where the tracer's own end-to-end test already forced the shared-code implementation to be correct before any TDD task started), this plan's pure functions (`pick_best`, `validate_candidates`, `parse_tool_result`) were genuinely independent enough from the infrastructure-only Task 1 tracer to support a real RED phase, so one was done.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking issue] `OutcomeCounts`/`Percentiles`/`LatencySummary` needed `Deserialize`, not just `Serialize`**
- **Found during:** Task 1
- **Issue:** The interface contract states `CurvePoint` is `(Serialize/Deserialize)`. `CurvePoint` embeds `OutcomeCounts` (loadgen.rs) and `LatencySummary`/`Percentiles` (metrics.rs) directly, none of which derived `Deserialize` (they were `Serialize`-only, sufficient for 07-06's own needs).
- **Fix:** Added `Deserialize` alongside the existing `Serialize` derive on each type. No field types or serialization shape changed.
- **Files modified:** `crates/rsg-bench/src/loadgen.rs`, `crates/rsg-bench/src/metrics.rs`
- **Verification:** `cargo test -p rsg-bench` (all 12 integration targets) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` both pass.
- **Commit:** `f9bbd41`

**2. [Rule 1 - Bug] 5 `clippy::unnecessary_get_then_check` lints in the crosscheck test file**
- **Found during:** Task 3 GREEN, running `cargo clippy --all-targets -- -D warnings` after `parse_tool_result` was implemented
- **Issue:** `result.metrics.get("backend").is_none()` (and four similar lines) triggers `clippy::unnecessary_get_then_check`, which is implied by `-D warnings`. This only surfaced once the test's own assertions were exercised against a real (non-stub) `BTreeMap`, since the lint fires on the pattern regardless of what `parse_tool_result` returns.
- **Fix:** Replaced each with `!result.metrics.contains_key(...)`. No assertion semantics changed.
- **Files modified:** `crates/rsg-bench/tests/crosscheck_parse.rs`
- **Verification:** `cargo clippy -p rsg-bench --all-targets -- -D warnings` clean; all 7 `crosscheck_parse` tests still pass.
- **Commit:** `000f037`

**Total deviations:** 2 auto-fixed (1 Rule 3 blocking-issue, 1 Rule 1 bug). **Impact:** Neither changes the plan's scope or design; both were necessary for the plan's own stated interface contract (`Serialize`+`Deserialize`) and a clean `-D warnings` clippy gate to hold.

## Issues Encountered
None beyond the deviations above.

## User Setup Required
None - no external service configuration required. No new crates were added: `run_cross_tool`'s kill-on-timeout spawn-and-poll loop is hand-rolled on `std::process::Command`/`std::thread`/`std::sync::mpsc` (same technique `manifest.rs`'s `run_with_timeout` already established in 07-06), not a new timeout/subprocess crate. `vllm`/`sglang` themselves are never installed by this harness (T-07-16/T-07-SC) -- the human verifies them on the GPU box per 07-10's planned human check, confirming D4's [ASSUMED] default flag templates against real tool output.

## Next Phase Readiness
`rsg_bench::scenarios::{s2_saturation, sweep, crosscheck}` are complete and tested against `bench-stub` arms on the Mac. `BENCH-04`/`BENCH-07` both stay open in `REQUIREMENTS.md` per the shared-ID gate (`requirements.ready-ids` reports 0/2 ready — other 07-xx plans also declare them). The 07-09 report plan can read `result.curve`/`result.peak_rps`/`result.metrics` directly from the manifest via the `s2_saturation`/`num_tokenizer_sweep`/`crosscheck_vllm`/`crosscheck_sglang` scenario names fixed by this plan's interface contract; the 07-10 wrapper can pass `--python-best-num-tokenizer` using `sweep-num-tokenizer`'s `best_num_tokenizer=<K>` stdout line. No blockers. The one open assumption (D4: default `vllm`/`sglang` flag templates, RESEARCH A3) is resolved by 07-10's GPU-box human check, not by further Mac-side work.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-06*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/scenarios/s2_saturation.rs
- FOUND: crates/rsg-bench/src/scenarios/sweep.rs
- FOUND: crates/rsg-bench/src/scenarios/crosscheck.rs
- FOUND: crates/rsg-bench/tests/s2_curve.rs
- FOUND: crates/rsg-bench/tests/sweep.rs
- FOUND: crates/rsg-bench/tests/crosscheck_parse.rs
- FOUND: crates/rsg-bench/tests/fixtures/vllm_result.json
- FOUND: crates/rsg-bench/tests/fixtures/sglang_result.jsonl
- FOUND: crates/rsg-bench/tests/fixtures/fake_bench_tool.sh
- FOUND: f9bbd41 (feat: tracer -- rsg-bench s2 sweeps load levels per trial)
- FOUND: 950d82f (test: RED -- failing tests for D-09 num-tokenizer sweep)
- FOUND: c4d57c2 (feat: GREEN -- implement validate_candidates/pick_best)
- FOUND: 1c67a37 (test: RED -- failing tests for D-04 cross-check parsing)
- FOUND: 000f037 (feat: GREEN -- implement parse_tool_result)
- No unexpected file deletions in any of the five task commits
