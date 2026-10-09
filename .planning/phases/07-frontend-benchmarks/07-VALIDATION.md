---
phase: "7"
slug: "frontend-benchmarks"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-06"
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in; `cargo-nextest` is CLAUDE.md's recommendation but not yet wired into `scripts/check_all.sh`) + `pytest` 9.1.1 for `python/tests` |
| **Config file** | none dedicated; `scripts/check_all.sh` is the phase-gate script |
| **Quick run command** | `cargo test -p rsg-bench` (once the crate exists); `.venv/bin/python -m pytest python/tests -k profiling -q` |
| **Full suite command** | `scripts/check_all.sh` |
| **Estimated runtime** | ~60-120 seconds (new crate, no baseline measured yet) |

---

## Sampling Rate

- **After every task commit:** Run the relevant `cargo test -p rsg-bench --test <name>` or targeted `pytest -k`
- **After every plan wave:** Run `scripts/check_all.sh`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 07-01-T1 | 07-01 | 1 | BENCH-02 | T-07-01, T-07-03 | Load only to 127.0.0.1; the group is fully torn down | integration (tracer) | `cargo test -p rsg-bench --test tracer tracer_one_request_end_to_end` | ❌ W0 | ⬜ pending |
| 07-01-T2 | 07-01 | 1 | BENCH-02 | T-07-01, T-07-02 | Refuses pgid <= 1 and its own group; Drop kills a leaked group | integration | `cargo test -p rsg-bench` | ❌ W0 | ⬜ pending |
| 07-02-T1 | 07-02 | 1 | BENCH-08 | T-07-05 | gc_only never starts tracemalloc; hook never raises into the host | unit (python) | `.venv/bin/python -m pytest python/tests/test_hook_gc_only.py python/tests/test_profile_hook.py -q` | ❌ W0 | ⬜ pending |
| 07-02-T2 | 07-02 | 1 | BENCH-08 | T-07-05 | proc records only in gc_only; full mode unchanged | unit (python) | `.venv/bin/python -m pytest python/tests/test_hook_gc_only.py python/tests/test_profile_hook.py -q` | ❌ W0 | ⬜ pending |
| 07-03-T1 | 07-03 | 1 | BENCH-06 | T-07-07, T-07-08 | No host flag; symlinked output refused | unit (python, tracer) | `.venv/bin/python -m pytest python/tests/test_bench_simple_reuse.py -q` | ❌ W0 | ⬜ pending |
| 07-03-T2 | 07-03 | 1 | BENCH-06 | — | N/A | smoke (python, bench_simple drift guard) | `.venv/bin/python -m pytest python/tests/test_bench_simple_reuse.py -q` | ❌ W0 | ⬜ pending |
| 07-04-T1 | 07-04 | 2 | BENCH-02, BENCH-03 | T-07-09 | N/A | integration (tracer) | `cargo test -p rsg-bench --test loadgen_cancel && cargo test -p rsg-bench --lib` | ❌ W0 | ⬜ pending |
| 07-04-T2 | 07-04 | 2 | BENCH-02 | — | N/A | unit (known-distribution fixture) | `cargo test -p rsg-bench --test metrics && cargo test -p rsg-bench --test loadgen_cancel` | ❌ W0 | ⬜ pending |
| 07-04-T3 | 07-04 | 2 | BENCH-07 | — | N/A | unit | `cargo test -p rsg-bench --lib stats::` | ❌ W0 | ⬜ pending |
| 07-05-T1 | 07-05 | 3 | BENCH-08 | T-07-10 | Malformed hook lines counted, never panic | integration (cross-language tracer) | `cargo test -p rsg-bench --test gclog gclog_reads_real_gc_only_hook_files` | ❌ W0 | ⬜ pending |
| 07-05-T2 | 07-05 | 3 | BENCH-05, BENCH-08 | T-07-11 | PSS None (never 0) off Linux or on error | unit (Mac-safe; proves the gate, not a real Linux PSS number) | `cargo test -p rsg-bench --test memory_pss_gate` | ❌ W0 | ⬜ pending |
| 07-05-T3 | 07-05 | 3 | BENCH-08 | T-07-10 | N/A | unit | `cargo test -p rsg-bench --test gclog` | ❌ W0 | ⬜ pending |
| 07-06-T1 | 07-06 | 4 | BENCH-03 | — | N/A | integration (tracer) | `cargo test -p rsg-bench --test s1_report_schema s1_session_end_to_end_with_stub_arms` | ❌ W0 | ⬜ pending |
| 07-06-T2 | 07-06 | 4 | BENCH-07 | T-07-12, T-07-13, T-07-14, T-07-15 | Secrets redacted; atomic, symlink-refusing writes; no shell in templates; port-free check | unit + integration | `cargo test -p rsg-bench --test orchestrator_alternation && cargo test -p rsg-bench --test manifest_schema` | ❌ W0 | ⬜ pending |
| 07-06-T3 | 07-06 | 4 | BENCH-08 | — | Identical hook env for every arm | integration | `cargo test -p rsg-bench --test s1_report_schema` | ❌ W0 | ⬜ pending |
| 07-07-T1 | 07-07 | 5 | BENCH-04 | — | N/A | integration (tracer) | `cargo test -p rsg-bench --test s2_curve` | ❌ W0 | ⬜ pending |
| 07-07-T2 | 07-07 | 5 | BENCH-07 | — | N/A | unit + integration | `cargo test -p rsg-bench --test sweep` | ❌ W0 | ⬜ pending |
| 07-07-T3 | 07-07 | 5 | BENCH-04 | T-07-16, T-07-17, T-07-18 | Never installs tools; defensive parse; no shell | unit + integration | `cargo test -p rsg-bench --test crosscheck_parse` | ❌ W0 | ⬜ pending |
| 07-08-T1 | 07-08 | 6 | BENCH-05 | T-07-20 | Detached group signalled only after leader identity check | integration (tracer) | `cargo test -p rsg-bench --test coldstart` | ❌ W0 | ⬜ pending |
| 07-08-T2 | 07-08 | 6 | BENCH-05 | T-07-19 | POSIX quoting proven through sh | unit | `cargo test -p rsg-bench --test hyperfine_parse` | ❌ W0 | ⬜ pending |
| 07-08-T3 | 07-08 | 6 | BENCH-05 | T-07-21 | N/A | integration (needs hyperfine 1.20.0) | `cargo test -p rsg-bench --test s3_coldstart_e2e -- --include-ignored` | ❌ W0 | ⬜ pending |
| 07-09-T1 | 07-09 | 7 | BENCH-06 | T-07-22, T-07-23 | Regression stated; schema-checked driver output | integration (tracer) | `cargo test -p rsg-bench --test throughput_runner` | ❌ W0 | ⬜ pending |
| 07-09-T2 | 07-09 | 7 | BENCH-07, BENCH-08 | T-07-22 | Provenance banner; failed trials listed; n<2 never invented | unit | `cargo test -p rsg-bench --test report_render` | ❌ W0 | ⬜ pending |
| 07-10-T1 | 07-10 | 8 | BENCH-02 | T-07-24 | No orphaned rsg-server or mock-scheduler | integration (tracer) | `cargo build -p rsg-server --bins && cargo test -p rsg-bench --test mock_stack` | ❌ W0 | ⬜ pending |
| 07-10-T2 | 07-10 | 8 | BENCH-03..BENCH-08 | T-07-25, T-07-26 | Never echoes env values | script (python) | `.venv/bin/python -m pytest python/tests/test_gpu_bench_script.py -q` | ❌ W0 | ⬜ pending |
| 07-10-T3 | 07-10 | 8 | BENCH-02..BENCH-05, BENCH-07 | T-07-25 | No mock output under docs/benchmarks | e2e (Mac; Phase 5 precondition) | `bash scripts/bench_mac_devpass.sh && scripts/check_all.sh --offline` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky. Task IDs follow `<plan>-T<n>` against the PLAN.md task numbering.*

---

## Wave 0 Requirements

- [ ] `crates/rsg-bench/` — new crate and new `tests/` dir, created by 07-01 Task 1 (tracer), including the `bench-stub` fixture binary every later harness test launches
- [ ] `python/tests/test_hook_gc_only.py` — created by 07-02 Task 1. It covers BENCH-08's `hook.py` gc_only mode (GC-only instrumentation during timed runs, per 07-CONTEXT.md D-15) and the proc-name role records
- [ ] Known-distribution fixture for the hdrhistogram unit test — 07-04 Task 2 (`tests/metrics.rs` records 1..=1000 ms, expects P99 = 990 ± 1 ms and P50 = 500 ± 1 ms)
- [ ] `crates/rsg-bench/tests/common/mod.rs` — created by 07-01 with a bench-stub helper (`free_port`, `stub_spec`, `read_stub_events`). Planner decision: rsg-server's `MockScheduler` test helper is not shared, because its `CARGO_BIN_EXE_mock-scheduler` only resolves inside rsg-server's own tests. rsg-bench reaches mock-scheduler through the `rsg-mock-stack` binary (07-10) instead

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Full GPU-measured Python-vs-Rust A/B comparison (headline benchmark numbers) | BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08 | Requires the Linux GPU machine with Phase 6 (GPU End-to-End Parity) landed. The Mac dev pass only verifies harness mechanics against `mock-scheduler`, per 07-CONTEXT.md D-03 | The `<human-check>` in 07-10 Task 2: verify vllm and sglang on pypi.org, install them into separate venvs, run `bash scripts/gpu_phase7_bench.sh --model Qwen/Qwen3-0.6B`, confirm every step PASSes and that docs/benchmarks/frontend-benchmarks.{json,md} carry valid provenance, CIs, deltas and GC/memory tables, then compare the BENCH-06 delta and CI against the ±2% reference target and write the narrative findings |

*All phase behaviors that CAN run on the Mac (harness mechanics, hdrhistogram correctness, manifest schema, orchestrator alternation, GC-mode toggle) have automated verification above.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
