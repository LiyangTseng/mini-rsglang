---
phase: "2"
slug: "python-frontend-baseline-profile"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-05"
validated: "2026-10-05"
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `02-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | pytest 9.1.1 (already project-pinned) |
| **Config file** | root `pyproject.toml` `[tool.pytest.ini_options]` — `testpaths = ["python/tests"]`, `markers = ["slow: spawns processes or builds binaries"]` |
| **Quick run command** | `.venv/bin/python -m pytest python/tests -q -m "not slow"` |
| **Full suite command** | `.venv/bin/python -m pytest python/tests -q` (per `scripts/check_all.sh` step 2) |
| **Estimated runtime** | ~30 seconds (new Wave 0 suite; no GPU-dependent test runs on the Mac) |

---

## Sampling Rate

- **After every task commit:** Run `.venv/bin/python -m pytest python/tests -q -m "not slow"`
- **After every plan wave:** Run `.venv/bin/python -m pytest python/tests -q`
- **Before `/gsd-verify-work`:** Full suite must be green on the Mac AND the GPU profiling run (`scripts/gpu_phase2_profile.sh`) signed off
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

Filled in by the planner/executor per task. Requirement → test map from research:

| Req ID | Behavior | Test Type | Automated Command | File Exists | Status |
|--------|----------|-----------|-------------------|-------------|--------|
| BENCH-01 | `baseline-profile.json` schema is well-formed (GC pauses, RSS curve, GIL %, radix share %, per scenario) | unit | `pytest python/tests/test_profile_sidecar.py::test_json_sidecar_schema -q` | ✅ 02-04 | ✅ green |
| BENCH-01 | speedscope-bucketing function correctly attributes `match_req`/`cache_req`/`match_prefix`/`insert_prefix`/`evict`/`_tree_walk` samples as "radix time" | unit | `pytest python/tests/test_profile_analysis.py::test_radix_frame_bucketing -q` | ✅ 02-05 | ✅ green |
| BENCH-01 | role-identification grep logic correctly distinguishes a stub "scheduler" dump from a stub "tokenize_worker" dump | unit | `pytest python/tests/test_baseline_profile.py::test_role_identification -q` | ✅ 02-03 | ✅ green |
| BENCH-01 | startup shim propagates `gc.callbacks` into a `multiprocessing.Process(spawn)` child (Pitfall 3 pre-flight check) | integration (slow) | `pytest python/tests/test_profile_hook.py::test_hook_propagates_to_spawn_child -q -m slow` | ✅ 02-02 | ✅ green |
| BENCH-01 | bash wrapper's `--help` works anywhere (mirrors `gpu_phase1_check.sh`'s existing pattern) | smoke | `scripts/gpu_phase2_profile.sh --help` | ✅ 02-07 | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

**Renames from the draft (expected — written before any code existed):** the `.pth`-file hook design
was superseded during 02-02 execution by a generated `sitecustomize.py` shim (Claude's discretion per
CONTEXT, documented in 02-02-SUMMARY.md) — same behavior, renamed test
(`test_pth_hook_propagates_to_spawn_child` → `test_hook_propagates_to_spawn_child`), moved to its own
`test_profile_hook.py` file rather than `test_baseline_profile.py`. The sidecar/analysis tests
likewise landed in their own `test_profile_sidecar.py`/`test_profile_analysis.py` files (02-04/02-05)
once those modules got dedicated plans, rather than all living in `test_baseline_profile.py` as the
pre-build draft guessed.

---

## Wave 0 Requirements

- [x] `python/tests/test_baseline_profile.py` (plus dedicated `test_profile_hook.py`, `test_profile_sidecar.py`, `test_profile_analysis.py`, `test_profile_scenarios.py`, `test_gpu_profile_script.py`, `test_baseline_profile_report.py`) — covers BENCH-01's parsing/bucketing/role-ID/hook-propagation logic across 9 plans, 110 fast tests + slow/e2e tests, 0 failures
- [x] `scripts/baseline_profile.py` and `scripts/gpu_phase2_profile.sh` — built, both executable, both exercised end to end on the Mac (stand-ins) and on the real GPU box
- [x] `docs/benchmarks/` directory — now holds the real GPU-measured `baseline-profile.json` and the hand-written `baseline-profile.md`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Result |
|----------|-------------|------------|--------|
| Real GPU profiling run produces `docs/benchmarks/baseline-profile.{json,md}` with GC/memory/GIL/radix numbers for all 3 scenarios | BENCH-01 | Needs GPU (Linux, CUDA) | **DONE.** `bash scripts/gpu_phase2_profile.sh` on `wsl-gpu` (NVIDIA RTX 3050): `ALL PASS` on the second attempt. First attempt caught a real bug — py-spy's `--nonblocking` output occasionally contains invalid UTF-8 in an unresolvable native-frame name (api_server's asyncio/uvloop event loop); fixed in `analysis.load_speedscope` (decode with `errors="replace"`) with its own regression test, committed `d4272b3`. A WSL PATH gap in the wrapper script (nvidia-smi/nvcc not on a non-interactive SSH PATH) was also found and fixed, committed `c53a4b3`, mirroring a precedent fix already in `gpu_phase1_check.sh` on `main`. |
| `py-spy dump`/`record --pid` attach succeeds against the scheduler and tokenize_worker PIDs | BENCH-01 | Assumed to need root/`CAP_SYS_PTRACE` on the GPU box | **DONE — assumption revised.** Verified empirically: this WSL2 box does not enforce `ptrace_scope` restrictions at all (`/proc/sys/kernel/yama/ptrace_scope` doesn't exist there), so `py-spy dump --pid <sibling>` attached successfully with no privilege grant of any kind. No `sudo setcap` was run; `getcap` on the py-spy binary printed nothing both before and after the run, confirming no lingering grant. This is environment-specific, not a general claim about every Linux host — a box with yama's default `ptrace_scope=1` would still need the grant this plan documents. |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated (audited post-execution by /gsd-validate-phase, 2026-10-05) — see [02-09-SUMMARY.md](./02-09-SUMMARY.md) for the GPU run details and [baseline-profile.md](../../../docs/benchmarks/baseline-profile.md) for the findings.

---

## Validation Audit 2026-10-05

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 (none needed) |
| Escalated | 0 |

All 5 draft-anticipated behaviors were already covered by the time all 9 plans completed (with the
expected naming/file-location drift documented above). Both manual-only items are now actually done,
not just theoretically coverable — see the Result column. Full project test suite: 110 passed, 36
skipped (fast), plus slow/e2e tests across `test_baseline_profile.py`, `test_profile_scenarios.py`,
`test_gpu_profile_script.py`; `.venv/bin/python -m pytest python/tests -q` (full suite including slow)
passed 163/163 as of plan 02-08's merge.
