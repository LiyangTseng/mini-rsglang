---
phase: 07-frontend-benchmarks
verified: 2026-10-07T20:31:11Z
status: human_needed
score: 7/7 must-haves verified
covered_files: [".planning/phases/07-frontend-benchmarks/07-01-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-01-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-02-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-02-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-03-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-03-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-04-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-04-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-05-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-05-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-06-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-06-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-07-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-07-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-08-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-08-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-09-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-09-SUMMARY.md", ".planning/phases/07-frontend-benchmarks/07-10-PLAN.md", ".planning/phases/07-frontend-benchmarks/07-10-SUMMARY.md", "crates/rsg-bench/Cargo.toml", "crates/rsg-bench/src/bin/bench-stub.rs", "crates/rsg-bench/src/bin/rsg-mock-stack.rs", "crates/rsg-bench/src/client.rs", "crates/rsg-bench/src/cmdline.rs", "crates/rsg-bench/src/gclog.rs", "crates/rsg-bench/src/lib.rs", "crates/rsg-bench/src/loadgen.rs", "crates/rsg-bench/src/main.rs", "crates/rsg-bench/src/manifest.rs", "crates/rsg-bench/src/memory.rs", "crates/rsg-bench/src/metrics.rs", "crates/rsg-bench/src/orchestrator.rs", "crates/rsg-bench/src/procs.rs", "crates/rsg-bench/src/report.rs", "crates/rsg-bench/src/rng.rs", "crates/rsg-bench/src/roles.rs", "crates/rsg-bench/src/scenarios/crosscheck.rs", "crates/rsg-bench/src/scenarios/mod.rs", "crates/rsg-bench/src/scenarios/s1_cancel.rs", "crates/rsg-bench/src/scenarios/s2_saturation.rs", "crates/rsg-bench/src/scenarios/s3_coldstart.rs", "crates/rsg-bench/src/scenarios/standard_throughput.rs", "crates/rsg-bench/src/scenarios/sweep.rs", "crates/rsg-bench/src/sse.rs", "crates/rsg-bench/src/stats.rs", "crates/rsg-bench/tests/coldstart.rs", "crates/rsg-bench/tests/common/mod.rs", "crates/rsg-bench/tests/crosscheck_parse.rs", "crates/rsg-bench/tests/gclog.rs", "crates/rsg-bench/tests/hyperfine_parse.rs", "crates/rsg-bench/tests/loadgen_cancel.rs", "crates/rsg-bench/tests/manifest_schema.rs", "crates/rsg-bench/tests/memory_pss_gate.rs", "crates/rsg-bench/tests/metrics.rs", "crates/rsg-bench/tests/mock_stack.rs", "crates/rsg-bench/tests/orchestrator_alternation.rs", "crates/rsg-bench/tests/report_render.rs", "crates/rsg-bench/tests/s1_report_schema.rs", "crates/rsg-bench/tests/s2_curve.rs", "crates/rsg-bench/tests/s3_coldstart_e2e.rs", "crates/rsg-bench/tests/sweep.rs", "crates/rsg-bench/tests/teardown.rs", "crates/rsg-bench/tests/throughput_runner.rs", "crates/rsg-bench/tests/tracer.rs", "crates/rsg-tokenizer/src/loader.rs", "python/rsglang/bench/__init__.py", "python/rsglang/bench/standard_throughput.py", "python/rsglang/profiling/hook.py", "python/tests/test_bench_simple_reuse.py", "python/tests/test_gpu_bench_script.py", "python/tests/test_hook_gc_only.py", "scripts/bench_mac_devpass.sh", "scripts/gpu_phase7_bench.sh"]
covered_digest: "v2:sha256:700fa150745a6df8b7639b34d1658263f4a2350cdf69877c2dbc6b6d36f28987"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Run `scripts/gpu_phase7_bench.sh` on the Linux CUDA GPU box (after Phase 6 lands) to produce the real measured Python-vs-Rust comparison and write `docs/benchmarks/frontend-benchmarks.{json,md}`."
    expected: "The report shows no `NOT A FRONTEND COMPARISON` banner (backend_kind: real, GPU present), with S1/S2/S3/throughput numbers, GC/memory tables, and the BENCH-06 regression/no-regression sentence with a Welch 95% CI."
    why_human: "Requires a Linux machine with a CUDA GPU; this verification ran on a Mac dev machine with no GPU, per the project's own documented constraint. This is the phase's single explicitly-deferred `<human-check>` item (07-10 plan Task 2, SUMMARY coverage item D4, `human_judgment: true`), not a gap in this phase's own deliverable (the harness)."
---

# Phase 07: Frontend Benchmarks Verification Report

**Phase Goal:** A reproducible harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios over the Python frontend on the same backend, and shows that standard throughput does not regress.
**Verified:** 2026-10-07T20:31:11Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | A Rust load generator exists: open-loop, mid-stream cancellation, records TTFT/P99/RPS (BENCH-02) | ✓ VERIFIED | `crates/rsg-bench/src/{client,loadgen,metrics,sse,procs}.rs`; `cargo test -p rsg-bench` run by this verifier: all 36 unit + every integration test green (tracer.rs, loadgen_cancel.rs, metrics.rs). `rsg_bench::loadgen::run_open_loop`/`run_closed` both exist and are exercised by `s2_open_loop_curve_with_stub_arms`/`closed_mode_curve`. |
| 2 | Scenario 1 (128 agents, random cancellations, P99 TTFT, Python vs Rust) is runnable through the shared orchestrator (BENCH-03) | ✓ VERIFIED | `crates/rsg-bench/src/scenarios/s1_cancel.rs` defaults confirmed by direct read: `agents=128, duration_s=120.0, cancel_fraction=0.25, max_tokens=256, think_max_s=0.5` — matches the plan's must-have text verbatim. `s1_report_schema.rs` tests pass. This verifier independently ran `scripts/bench_mac_devpass.sh`, which drives a live S1 session (16 agents override) against the real `rsg-server` and confirmed non-zero completed/cancelled counts and alternation. |
| 3 | Scenario 2 (32-token saturation, RPS-vs-latency curve, open/closed modes, sweep, cross-check) is runnable (BENCH-04) | ✓ VERIFIED | `crates/rsg-bench/src/scenarios/{s2_saturation,sweep,crosscheck}.rs`. `cargo test`: `s2_curve.rs` (5/5), `sweep.rs` (5/5), `crosscheck_parse.rs` pass. `sweep::pick_best` read directly: ties break to the smaller candidate exactly as specified. This verifier's own `bench_mac_devpass.sh` run produced both an open-loop and a closed-loop S2 curve against the live Rust frontend. |
| 4 | Scenario 3 (cold start, frontend memory, end-to-end startup reported separately) is runnable (BENCH-05) | ✓ VERIFIED | `crates/rsg-bench/src/scenarios/s3_coldstart.rs`, `crates/rsg-bench/src/memory.rs`. `coldstart.rs`/`hyperfine_parse.rs` tests pass (1 test correctly `ignored` only in environments without `hyperfine`; this verifier has `hyperfine 1.20.0` installed and the full `bench_mac_devpass.sh` run exercised the real hyperfine-timed S3 path end to end against the live Rust frontend, producing `means.frontend_tail_s` and `means.e2e_ready_s`). |
| 5 | Standard-inference throughput is measured and a regression is explicitly reported in the summary, never hidden (BENCH-06) | ✓ VERIFIED | `python/rsglang/bench/standard_throughput.py` (ports `bench_simple.py`'s own workload/formulas, drift-guarded by `ast` parsing of the vendored files) + `crates/rsg-bench/src/scenarios/standard_throughput.rs` + `crates/rsg-bench/src/report.rs`. `report.rs:515-520` carries the literal regression/no-regression sentences with "±2% is a reference target, not a gate." `report_render.rs::no_regression_sentence` test passes. `throughput_runner.rs` (3/3) and `test_bench_simple_reuse.py` (part of 21/21 pytest pass) pass. |
| 6 | Python is benchmarked at both default and best `--num-tokenizer`; A/B runs alternate strictly; results carry 95% CIs and a reproducible manifest (BENCH-07) | ✓ VERIFIED | `crates/rsg-bench/src/{orchestrator,stats,manifest}.rs`. `orchestrator_alternation.rs` (6/6: `two_arms_alternate_p_r`, `three_arms_round_robin`, `best_equal_to_default_merges`, `runs_zero_rejected_by_cli`, …) and `manifest_schema.rs` pass. `stats::welch_diff_ci95`/`mean_ci95` exist and are exercised by `metrics.rs` tests. |
| 7 | Every scenario report shows frontend memory and Python GC pause counts alongside TTFT/P99/RPS (BENCH-08) | ✓ VERIFIED | `crates/rsg-bench/src/{gclog,memory,roles}.rs` + `report.rs`. `gclog.rs` and `memory_pss_gate.rs` tests pass; `report_render.rs::{rust_gc_row_not_applicable, pss_unavailable_rendered}` pass. This verifier's live dev-pass run produced a report with GC/memory tables next to latency numbers and the `NOT A FRONTEND COMPARISON` banner (correct, since the mock backend was used). |

**Score:** 7/7 truths verified (0 present-but-behavior-unverified)

### GPU-Box Headline Comparison — Explicitly Deferred, Not a Gap

The phase's own roadmap success criteria require measured numbers from a real Linux CUDA GPU box; that run was never expected on this Mac dev machine (no GPU present) and is documented by the executor itself as a single end-of-phase `<human-check>` (07-10 Task 2, SUMMARY coverage item D4, `human_judgment: true`). This verifier confirms the harness itself (`scripts/gpu_phase7_bench.sh`) is built, dry-run tested (reproduced independently below), and ready for that run — the phase's own job (build + prove the harness) is complete. This is reported as the sole human-verification item, not a gap.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/rsg-bench/` (lib + 2 bins) | Load generator, scenarios, orchestrator, report | ✓ VERIFIED | Builds clean (`cargo build -p rsg-bench --bins`); all unit/integration tests pass |
| `crates/rsg-bench/src/bin/bench-stub.rs` | OpenAI-SSE test fixture | ✓ VERIFIED | Used by every Phase 7 mechanics test |
| `crates/rsg-bench/src/bin/rsg-mock-stack.rs` | Mac launcher adapter (real rsg-server + mock-scheduler) | ✓ VERIFIED | `mock_stack.rs` tests pass; this verifier ran it live via `bench_mac_devpass.sh` |
| `python/rsglang/bench/standard_throughput.py` | BENCH-06 workload driver | ✓ VERIFIED | Imports lazily, drift-guarded against vendored `bench_simple.py`; `test_bench_simple_reuse.py` passes |
| `python/rsglang/profiling/hook.py` (gc_only mode) | BENCH-08 input: GC/proc records | ✓ VERIFIED | `test_hook_gc_only.py` passes (part of 21/21) |
| `scripts/gpu_phase7_bench.sh` | One-command GPU-box wrapper | ✓ VERIFIED | `--dry-run` reproduced independently by this verifier, printed the exact 7-step `RUN:` sequence described in the must-haves |
| `scripts/bench_mac_devpass.sh` | D-03 Mac verification pass | ✓ VERIFIED | Ran independently by this verifier end to end; printed `devpass: OK` |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| `loadgen.rs` | `client.rs::stream_chat` | every planned request → one `stream_chat` call | ✓ WIRED | `loadgen_cancel.rs::loadgen_counts_match_server` passes |
| `scenarios/s1_cancel.rs` | `loadgen::run_agents` | `S1Runner::run_trial` | ✓ WIRED | `s1_report_schema.rs` passes |
| `orchestrator.rs` | `procs`, `memory::MemorySampler`, `gclog::read_hook_dir`, `roles::RoleMap` | per-trial lifecycle | ✓ WIRED | Live dev-pass run produced manifests with GC/memory/co-occurrence blocks populated |
| `scenarios/standard_throughput.rs` | `python/rsglang/bench/standard_throughput.py` | subprocess + schema check | ✓ WIRED | `throughput_runner.rs` passes |
| `report.rs` | `stats::{mean_ci95, welch_diff_ci95}` and `scenarios::sweep::pick_best` | aggregation | ✓ WIRED | `report_render.rs::sweep_best_recomputed` passes |
| `scripts/gpu_phase7_bench.sh` | `rsg-bench sweep-num-tokenizer → s1/s2/crosscheck/s3/throughput → report → check_upstream.py` | `BEST` parsed from sweep's stdout | ✓ WIRED | Independently reproduced `--dry-run` output shows the full chain in order |
| `scripts/bench_mac_devpass.sh` | `rsg-mock-stack` as both arms' launch template | `--backend-kind mock` | ✓ WIRED | Live run: report's first line carried the `NOT A FRONTEND COMPARISON` banner naming `mock, no-gpu` |

### Behavioral Spot-Checks (independently executed by this verifier, not taken from SUMMARY.md)

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| rsg-bench crate test suite | `cargo test -p rsg-bench` | 36 unit + all integration tests across 19 binaries: 0 failed, 1 correctly ignored (needs hyperfine, present here but one test still gated) | ✓ PASS |
| Python bench/profiling/gpu-script tests | `.venv/bin/python -m pytest python/tests/test_bench_simple_reuse.py python/tests/test_hook_gc_only.py python/tests/test_gpu_bench_script.py -q` | 21 passed | ✓ PASS |
| GPU wrapper dry-run | `bash scripts/gpu_phase7_bench.sh --dry-run` | Printed exact 7-step `RUN:` sequence (build, sweep, s1, s2, throughput, report, check_upstream.py) | ✓ PASS |
| D-03 Mac dev pass (full end-to-end harness vs. the real Rust frontend) | `bash scripts/bench_mac_devpass.sh` | `devpass: OK` — S1/S2(open)/S3/S2(closed) manifests written, report decoded with mock banner, `rsg_requests_cancelled_total` grew 0→3 from a curl-aborted stream | ✓ PASS |
| `docs/benchmarks/` untouched by mock-backed runs | `git status --porcelain docs/benchmarks` | Empty; directory contains only Phase 2's `baseline-profile.{json,md}` | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| BENCH-02 | 07-01, 07-04, 07-10 | Rust load generator: open-loop, mid-stream cancellation, TTFT/P99/RPS | ✓ SATISFIED | See truth #1 |
| BENCH-03 | 07-04, 07-06, 07-10 | Scenario 1: 128 agents, cancellations, P99 TTFT | ✓ SATISFIED | See truth #2 |
| BENCH-04 | 07-07, 07-10 | Scenario 2: saturation RPS-vs-latency | ✓ SATISFIED | See truth #3 |
| BENCH-05 | 07-05, 07-08, 07-10 | Scenario 3: cold start + memory | ✓ SATISFIED | See truth #4 |
| BENCH-06 | 07-03, 07-09, 07-10 | Standard throughput, no regression claim | ✓ SATISFIED | See truth #5 |
| BENCH-07 | 07-04, 07-06, 07-07, 07-09, 07-10 | A/B num-tokenizer sweep, CIs, manifest | ✓ SATISFIED | See truth #6 |
| BENCH-08 | 07-02, 07-05, 07-06, 07-09, 07-10 | GC/memory alongside latency in every report | ✓ SATISFIED | See truth #7 |

No orphaned requirements: `.planning/REQUIREMENTS.md`'s traceability table maps exactly these 7 IDs to Phase 7, all marked `Complete`, matching the union of `requirements:` fields across all 10 plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `crates/rsg-bench/src/scenarios/s2_saturation.rs:138` | — | `--prompt-words-max` unvalidated; `0` underflows `u32` subtraction in `range_inclusive_u32` (debug panic / release near-u32::MAX balloon) | ⚠️ Warning (code review WR-01, disposition: open) | Narrow operator-input edge case; does not affect default-configuration runs or any test in this suite |
| `crates/rsg-bench/src/client.rs:225-258` | — | `read_stream`: an ambiguous clean-EOF-without-`[DONE]` (no `Content-Length`/chunked framing) is always classified `Outcome::Completed`, even if the backend died mid-stream | ⚠️ Warning (code review WR-02, disposition: open) | Could silently inflate a 0-token request into a "success" on a real backend that uses `Connection: close` framing without explicit length; bounded on mock-backed runs by `report.rs`'s `comparison_valid` gate, but is a genuine correctness edge for the eventual real GPU run |
| `crates/rsg-bench/src/manifest.rs:224-230` | — | `probe_upstream_sha` silently treats an uppercase-hex 40-char SHA as "file doesn't exist" (`None`, no warning) | ⚠️ Warning (code review WR-03, disposition: open) | Cosmetic/provenance-only; `git rev-parse` always emits lowercase so unlikely to trigger in practice |
| `crates/rsg-bench/src/cmdline.rs` | — | `redact_argv` whitespace normalization; `is_secret_name` fixed substring allowlist; `also_best` true-by-default without a sweep | ℹ️ Info (code review IN-01/02/03, disposition: open) | Low-impact, already flagged for future revisit in `07-REVIEW.md` |

No debt markers (`TODO`/`FIXME`/`XXX`) found in any phase-7 file. All findings above come from `.planning/phases/07-frontend-benchmarks/07-REVIEW.md` (severity: 0 critical, 3 warning, 3 info) — none were fixed or dismissed (disposition: `open` for all 6 in `07-REVIEW-DISPOSITION.md`), carried here for visibility since they remain live in the merged code. None blocks the phase goal: all three are narrow, bounded edge cases in operator input validation, ambiguous-EOF classification, and cosmetic provenance parsing — not failures of the harness's core measurement mechanics, which this verifier independently exercised end to end against the real Rust frontend.

### Human Verification Required

### 1. GPU-box headline Python-vs-Rust measured comparison

**Test:** Run `scripts/gpu_phase7_bench.sh` on a Linux machine with a CUDA GPU, after Phase 6 lands, pointing `--python-cmd`/`--rust-cmd` at the real launch commands.
**Expected:** The script completes preflight, the `--num-tokenizer` sweep, S1/S2/S3/throughput, optional vllm/sglang cross-checks, and writes `docs/benchmarks/frontend-benchmarks.{json,md}` with `backend_kind: real`, no `NOT A FRONTEND COMPARISON` banner, GC/memory tables, and a BENCH-06 regression/no-regression sentence with a Welch 95% CI.
**Why human:** Requires physical/cloud access to a CUDA GPU box; this verification environment is a GPU-less Mac, by the project's own documented constraint. This is the phase's one explicitly-deferred `<human-check>` (declared by 07-10's own plan and SUMMARY, `human_judgment: true`), not a defect discovered by this verification.

### Gaps Summary

No gaps. Every must-have truth across all 10 plans is backed by passing automated tests (`cargo test -p rsg-bench`: all green; `pytest` on the phase's Python tests: 21/21 green), and this verifier additionally reproduced two independent behavioral runs not taken from SUMMARY.md: `scripts/gpu_phase7_bench.sh --dry-run` and the full `scripts/bench_mac_devpass.sh` end-to-end pass against the project's real `rsg-server` (confirmed live cancellation reaching `/metrics`, manifests written, report rendered with the correct mock-provenance banner, and `docs/benchmarks/` left untouched). The only outstanding item is the GPU-box measured run itself, which the phase's own roadmap and plan documentation correctly scope to a future, hardware-gated human check rather than this phase's deliverable (the harness).

---

_Verified: 2026-10-07T20:31:11Z_
_Verifier: Claude (gsd-verifier)_
