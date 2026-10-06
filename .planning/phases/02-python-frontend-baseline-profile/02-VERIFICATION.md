---
phase: 02-python-frontend-baseline-profile
verified: 2026-10-05T00:00:00Z
status: human_needed
score: 4/4 must-haves verified
behavior_unverified: 0
overrides_applied: 0
covered_files: [".planning/phases/02-python-frontend-baseline-profile/02-01-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-01-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-02-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-02-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-03-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-03-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-04-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-04-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-05-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-05-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-06-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-06-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-07-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-07-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-08-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-08-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-09-PLAN.md", ".planning/phases/02-python-frontend-baseline-profile/02-09-SUMMARY.md", ".planning/phases/02-python-frontend-baseline-profile/02-CONTEXT.md", ".planning/phases/02-python-frontend-baseline-profile/02-DISCUSSION-LOG.md", ".planning/phases/02-python-frontend-baseline-profile/02-PATTERNS.md", ".planning/phases/02-python-frontend-baseline-profile/02-RESEARCH.md", ".planning/phases/02-python-frontend-baseline-profile/02-REVIEW-DISPOSITION.md", ".planning/phases/02-python-frontend-baseline-profile/02-REVIEW.md", ".planning/phases/02-python-frontend-baseline-profile/02-VALIDATION.md", "docs/benchmarks/baseline-profile.json", "docs/benchmarks/baseline-profile.md", "python/rsglang/profiling/__init__.py", "python/rsglang/profiling/analysis.py", "python/rsglang/profiling/hook.py", "python/rsglang/profiling/procs.py", "python/rsglang/profiling/scenarios.py", "python/rsglang/profiling/session.py", "python/rsglang/profiling/sidecar.py", "python/rsglang/testing/fake_profile_env.py", "python/tests/test_baseline_profile.py", "python/tests/test_baseline_profile_report.py", "python/tests/test_gpu_profile_script.py", "python/tests/test_profile_analysis.py", "python/tests/test_profile_hook.py", "python/tests/test_profile_scenarios.py", "python/tests/test_profile_sidecar.py", "requirements-mac.in", "requirements-mac.txt", "scripts/baseline_profile.py", "scripts/gpu_phase2_profile.sh"]
covered_digest: "v2:sha256:7916a1022aa0fda60d71461f0586b1f84130c09eb10eeb67f1b0a77f4c7e5034"
human_verification:
  - test: "Sign off on ROADMAP Phase 2 success criteria 1-4 against docs/benchmarks/baseline-profile.md's content (GC/memory quantification, GIL framing with per-request IPC/serde cost, radix share plus RADIX-01 recommendation, concrete Phase 7 benchmark-design inputs)."
    expected: "The human reviewer agrees the narrative report's framing, numbers, and recommendations are a faithful and sufficient answer to all 4 success criteria — not just mechanically present, but substantively useful as input to Phase 7 benchmark design and the v2 RADIX-01 decision."
    why_human: "This is a judgment call on report quality and sufficiency (e.g., whether the GIL-contention reasoning in SC2 — explaining why no cross-process GIL number exists in this 3-process topology, rather than reporting one — is an acceptable substitute for the roadmap's literal wording), not a presence/absence fact a grep or test can settle. Deferred to end-of-phase human_needed per workflow.human_verify_mode=end-of-phase (02-09-SUMMARY.md coverage item D3)."
---

# Phase 2: Python Frontend Baseline Profile Verification Report

**Phase Goal:** Measured numbers show where the frozen Python frontend spends host-side time and memory in each of the three benchmark scenarios, so the numbers can inform benchmark design and attribution.
**Verified:** 2026-10-05
**Status:** human_needed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

Must-haves are the 4 ROADMAP Phase 2 Success Criteria (BENCH-01), merged with plan
02-09's frontmatter `must_haves.truths` (which restate and sharpen the same 4 criteria
one-for-one plus two plan-local truths about the py-spy privilege grant and the
report-JSON number-formatting proof).

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Report quantifies, per scenario, GC pauses (count, duration, correlation with P99 TTFT) and memory allocation/resident growth (SC1) | ✓ VERIFIED | `docs/benchmarks/baseline-profile.md` has per-role GC count/total-pause/P99-pause tables and GC-to-P99-overlap tables for all 3 scenarios, plus per-role RSS start/end/growth and tracemalloc peak + top-3 allocation-site tables. Spot-checked against the raw JSON: s1 GC counts (api_server 3097/scheduler 11/tokenizer 12) and RSS figures match `docs/benchmarks/baseline-profile.json` exactly (verified via direct `json.load` extraction, see below). |
| 2 | Report quantifies GIL contention between tokenize/detokenize/HTTP handling, and per-request serde/IPC cost across process hops (SC2) | ✓ VERIFIED | Report gives per-process CPU-active%/GIL-held% table (api_server/scheduler/tokenizer, all 3 scenarios), a tokenize-vs-detokenize share-of-tokenizer-process-time table, and per-request `ipc_zmq`/`serde` ms tables for api_server and tokenizer. The report explicitly argues no single cross-process "GIL contention" number exists in this 3-OS-process topology (each process has its own independent GIL; tokenize/detokenize run sequentially inside the same process/GIL) — a reasoned, measurement-backed finding rather than an invented number. This specific judgment call (reasoned substitution vs. literal non-compliance) is the human-verification item below, not a gap: the report does not omit the topic, it explains and measures around it. |
| 3 | Report records scheduler time share spent in radix cache, per scenario, as RADIX-01 input (SC3) | ✓ VERIFIED | Radix-share table (s1 1.58%, s2 0.76%, s3 0.98%) with an explicit recommendation for RADIX-01. Independently recomputed from the raw JSON: `radix_samples/scheduler_samples` = 194/12253=1.583%, 6/790=0.759%, 5/512=0.977% — matches the report exactly. |
| 4 | Profiling run is scripted/repeatable; findings written as concrete Phase 7 benchmark-design inputs (which metrics, which effects credited to frontend) (SC4) | ✓ VERIFIED | `scripts/gpu_phase2_profile.sh` + `scripts/baseline_profile.py run`/`validate` exist, are executable, and the report's "Reproduce" section gives the exact one-command repro. "Inputs to Phase 7 benchmark design" section lists 4 concrete metrics tied to measured findings plus an explicit frontend-vs-backend attribution split. |

**Score:** 4/4 truths verified (0 present, behavior-unverified)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `docs/benchmarks/baseline-profile.json` | Real GPU-measured sidecar, schema-valid, `require_gpu` passing | ✓ VERIFIED | Exists (805 KB), `scripts/baseline_profile.py validate --require-gpu docs/benchmarks/baseline-profile.json` → `valid`, exit 0 (re-run live). `meta.mode="run"`, `meta.platform="linux"`, `meta.gpu="NVIDIA GeForce RTX 3050"`. |
| `docs/benchmarks/baseline-profile.md` | Hand-written narrative tied to the JSON, 10 required headings | ✓ VERIFIED | All 10 headings present; numbers spot-checked against JSON match exactly (see Data-Flow Trace below). |
| `python/tests/test_baseline_profile_report.py` | Mechanical report↔JSON consistency proof | ✓ VERIFIED | 4/4 tests pass live (`test_sidecar_is_a_valid_gpu_run`, `test_all_required_headings_present`, `test_every_radix_share_appears_formatted`, `test_every_p99_ttft_appears_formatted`). |
| `python/rsglang/profiling/{analysis,hook,procs,scenarios,session,sidecar}.py` | Profiling harness (GC/tracemalloc hook, role-ID/teardown, py-spy CPU/GIL/radix analysis, scenario drivers, JSON sidecar) | ✓ VERIFIED | All present, substantive (8-30 KB each, not stubs), and exercised by 113 passing unit/integration tests (fast suite). |
| `scripts/baseline_profile.py`, `scripts/gpu_phase2_profile.sh` | CLI + GPU-box wrapper | ✓ VERIFIED | Both present, executable (`-rwxr-xr-x`), `validate` subcommand confirmed working against the real JSON. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| `docs/benchmarks/baseline-profile.md` | `docs/benchmarks/baseline-profile.json` | Every radix share / P99 TTFT in the narrative is the JSON value, enforced by `test_baseline_profile_report.py` | ✓ WIRED | Tests pass live; independently re-derived s1/s2/s3 radix shares and s1 request counts/TTFT percentiles directly from the JSON via `json.load` and confirmed byte-exact match to the markdown's reported figures. |
| `scripts/baseline_profile.py run` | `docs/benchmarks/baseline-profile.json` | `session.run_session` → `sidecar.write_sidecar` | ✓ WIRED | `git diff --quiet HEAD -- docs/benchmarks/baseline-profile.json` exits 0 (committed JSON is byte-unedited since the GPU run per 02-09-SUMMARY.md, confirmed live). |
| `scripts/baseline_profile.py run` CLI exception handling | `procs.PySpyPermissionError` (CR-02 fix) | `cmd_run`'s except tuple now includes `procs.PySpyPermissionError`, returns exit 2 | ✓ WIRED | `grep` confirms `procs.PySpyPermissionError` in `scripts/baseline_profile.py`'s except tuple at line 386, returns `2 if isinstance(exc, procs.PySpyPermissionError) else 1`. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|---------------------|--------|
| `baseline-profile.md` Scenario 1 request stats | sent/completed/cancelled/TTFT p50/p90/p99/max/RPS | `docs/benchmarks/baseline-profile.json` → `scenarios.s1_cancel.requests` | Yes — independently re-extracted via `json.load`: `{'sent': 2012, 'completed': 1528, 'cancelled': 484, 'ttft_ms': {'p50': 98.14, 'p90': 227.50, 'p99': 968.38, 'max': 1069.12}, 'rps': 12.19}` matches report exactly | ✓ FLOWING |
| `baseline-profile.md` Radix cache share table | radix_samples/scheduler_samples/share, all 3 scenarios | `docs/benchmarks/baseline-profile.json` → `scenarios.{s1,s2,s3}.radix` | Yes — independently re-extracted: s1 194/12253=1.583%, s2 6/790=0.759%, s3 5/512=0.977%, matching the report's 1.58%/0.76%/0.98% | ✓ FLOWING |
| `docs/benchmarks/baseline-profile.json` itself | every metric | `meta.gpu="NVIDIA GeForce RTX 3050"`, `meta.platform="linux"`, `meta.mode="run"` | Yes — real GPU hardware name and Linux platform recorded, not a Mac stand-in; `validate --require-gpu` (which rejects non-GPU/non-run documents) passes | ✓ FLOWING |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|--------------|-------------|--------------|--------|----------|
| BENCH-01 | 02-01 through 02-09 (all 9 plans) | Profile the Python frontend on the GPU machine; quantify GC pauses, memory, GIL contention, serde/IPC cost, radix-cache share | ✓ SATISFIED | `REQUIREMENTS.md` line 99 marks `BENCH-01 | Phase 2 | Complete`. All 4 roadmap success criteria independently confirmed above. No orphaned requirements: BENCH-01 is the only requirement REQUIREMENTS.md maps to Phase 2, and all 9 plans declare it in frontmatter. |

### Anti-Patterns Found

None. Scanned all profiling-package source files, `scripts/baseline_profile.py`, and `scripts/gpu_phase2_profile.sh` for `TBD|FIXME|XXX|TODO|HACK|PLACEHOLDER`, empty-return stubs, and hardcoded-empty-data patterns — no blocking matches. (One `return []` in `scenarios.py:424` is a legitimate base case for `_tail(values, n<=0)`, not a stub.)

The code review (`02-REVIEW.md`) found 4 CRITICAL and 7 WARNING issues. All 4 CRITICALs are confirmed fixed in code (commit `0c78fe6`), each with a passing named regression test (verified live below). The 7 WARNINGs + 1 INFO are recorded `open` in `02-REVIEW-DISPOSITION.md` — none of them concern the measured numbers in `docs/benchmarks/baseline-profile.json`/`.md` (they are about secondary robustness paths: stale-pgid self-heal, out-of-range frame-index validation, py-spy non-permission-failure logging, port-probe race, recorder-survives-SIGKILL detection, finally-block exception masking, hook thread synchronization). They do not block this phase's goal (measured numbers exist and are usable) and are correctly left open per the review-disposition policy rather than silently dropped.

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none blocking) | - | - | - | - |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| 4 CR-fix regression tests pass | `pytest python/tests/test_baseline_profile.py::test_teardown_survives_eperm_on_reused_pgid python/tests/test_baseline_profile.py::test_run_permission_denied_exits_2 python/tests/test_profile_analysis.py::test_cpu_metrics_rate_hz_zero_does_not_raise python/tests/test_profile_sidecar.py::test_build_meta_helpers_tolerate_subprocess_timeout -v` | 4 passed | ✓ PASS |
| Report↔JSON consistency tests pass | `pytest python/tests/test_baseline_profile_report.py -v` | 4 passed | ✓ PASS |
| Real GPU JSON validates against schema + require_gpu gate | `python scripts/baseline_profile.py validate --require-gpu docs/benchmarks/baseline-profile.json` | `valid`, exit 0 | ✓ PASS |
| Full fast test suite is green (no regressions) | `pytest python/tests -q -m "not slow"` | 113 passed, 36 skipped (GPU-only), 60 deselected (slow) | ✓ PASS |
| Committed JSON is byte-unedited since the GPU run | `git diff --quiet HEAD -- docs/benchmarks/baseline-profile.json` | exit 0 | ✓ PASS |

### Probe Execution

Skipped — no `scripts/*/tests/probe-*.sh` files exist, and no plan/SUMMARY in this phase declares probe-based verification.

### Human Verification Required

### 1. Sign off on ROADMAP Phase 2 success criteria 1-4 against the report's content

**Test:** Read `docs/benchmarks/baseline-profile.md` end to end and judge whether its framing, numbers, and recommendations substantively (not just mechanically) satisfy all 4 ROADMAP success criteria — in particular SC2's reasoned explanation of why no single cross-process "GIL contention" number exists in this 3-process topology (each process has its own independent GIL; tokenize/detokenize share one process/GIL sequentially, HTTP handling is a separate process), and the RADIX-01 recommendation framing (measured share left to the author's judgment, since REQUIREMENTS.md does not numerically define "meaningful share").

**Expected:** The human reviewer agrees that the report is a faithful, sufficient, and honestly-caveated answer to all 4 success criteria — not a shortfall dressed up as a reasoned substitution.

**Why human:** This is a qualitative judgment on report sufficiency and the acceptability of a reasoned substitution for one criterion's literal wording (SC2), which a grep/test cannot adjudicate. This item was explicitly deferred by the executing plan (`02-09-PLAN.md`'s Task 2 `<human-check>`, documented in `02-09-SUMMARY.md` coverage item D3) to end-of-phase verification per `workflow.human_verify_mode=end-of-phase` (the project default) — this is the designed routing for that deferral, not a newly discovered gap.

### Gaps Summary

No gaps found. All 4 roadmap success criteria are independently verified against the real, GPU-measured `docs/benchmarks/baseline-profile.json` (not a Mac stand-in — `require_gpu` validation passes live), with spot-checked numbers matching exactly between the JSON and the hand-written narrative report. All 4 critical code-review findings are confirmed fixed in code with passing regression tests. The full fast test suite is green (113 passed, 0 failed). The only outstanding item is the single end-of-phase human sign-off that this project's workflow configuration (`human_verify_mode=end-of-phase`) deliberately defers past automated verification — this routes the phase to `human_needed`, not `gaps_found`.

---

_Verified: 2026-10-05_
_Verifier: Claude (gsd-verifier)_
