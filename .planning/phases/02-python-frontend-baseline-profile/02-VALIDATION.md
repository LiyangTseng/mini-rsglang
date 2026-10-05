---
phase: "2"
slug: "python-frontend-baseline-profile"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-05"
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
| BENCH-01 | `baseline-profile.json` schema is well-formed (GC pauses, RSS curve, GIL %, radix share %, per scenario) | unit | `pytest python/tests/test_baseline_profile.py::test_json_sidecar_schema -q` | ❌ W0 | ⬜ pending |
| BENCH-01 | speedscope-bucketing function correctly attributes `match_req`/`cache_req`/`match_prefix`/`insert_prefix`/`evict`/`_tree_walk` samples as "radix time" | unit | `pytest python/tests/test_baseline_profile.py::test_radix_frame_bucketing -q` | ❌ W0 | ⬜ pending |
| BENCH-01 | role-identification grep logic correctly distinguishes a stub "scheduler" dump from a stub "tokenize_worker" dump | unit | `pytest python/tests/test_baseline_profile.py::test_role_identification -q` | ❌ W0 | ⬜ pending |
| BENCH-01 | `.pth`-hook propagates `gc.callbacks` into a `multiprocessing.Process(spawn)` child (Pitfall 3 pre-flight check) | integration (slow) | `pytest python/tests/test_baseline_profile.py::test_pth_hook_propagates_to_spawn_child -q -m slow` | ❌ W0 | ⬜ pending |
| BENCH-01 | bash wrapper's `--help` works anywhere (mirrors `gpu_phase1_check.sh`'s existing pattern) | smoke | `scripts/gpu_phase2_profile.sh --help` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `python/tests/test_baseline_profile.py` — new file, covers BENCH-01's parsing/bucketing/role-ID/`.pth`-propagation logic
- [ ] `scripts/baseline_profile.py` and `scripts/gpu_phase2_profile.sh` — new files, no prior version exists
- [ ] `docs/benchmarks/` directory — does not exist yet (only `docs/mini-sglang-reading-guide.md` and `docs/agents/` currently exist under `docs/`)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Real GPU profiling run produces `docs/benchmarks/baseline-profile.{json,md}` with GC/memory/GIL/radix numbers for all 3 scenarios | BENCH-01 | Needs GPU (Linux, CUDA); `py-spy` requires `CAP_SYS_PTRACE`/root to attach to existing PIDs | `scripts/gpu_phase2_profile.sh` on the GPU machine; inspect `docs/benchmarks/baseline-profile.json` and `.md` for all 4 success criteria |
| `py-spy dump`/`record --pid` attach succeeds against the scheduler and tokenize_worker PIDs | BENCH-01 | Needs root/`CAP_SYS_PTRACE` grant on the GPU box, not verifiable on the Mac | Confirm `sudo` or capability grant before running the script; `py-spy dump --pid <pid>` returns a stack trace, not a permission error |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
