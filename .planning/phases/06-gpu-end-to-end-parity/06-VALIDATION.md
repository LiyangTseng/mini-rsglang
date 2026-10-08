---
phase: "06"
slug: "gpu-end-to-end-parity"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-06"
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `06-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `pytest` (Python), `cargo test --workspace` (Rust) for any Mac-testable logic in the new driver/script; human-run bash GPU script for the hardware-gated criteria themselves — same split as Phase 1/2 |
| **Config file** | none new — reuses the existing `python/tests/`, `crates/*/tests/` layout |
| **Quick run command** | `cargo test --workspace && .venv/bin/python -m pytest python/tests -q` (existing `scripts/check_all.sh` steps 1-2) |
| **Full suite command** | `bash scripts/check_all.sh` (Mac) then the new `scripts/gpu_phase6_parity.sh` (GPU, human-run) |
| **Estimated runtime** | TBD — Wave 0 phase, GPU script doesn't exist yet to time |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace` / `pytest python/tests -q` for any new Mac-testable logic in `parity_check.py` or the health watcher's bash helper functions (sourced and unit-tested the same way `gpu_phase1_check.sh` already is).
- **After every plan wave:** Run `bash scripts/check_all.sh` (Mac gate).
- **Before `/gsd-verify-work`:** `scripts/gpu_phase6_parity.sh` green, human-signed-off, on the GPU machine — matches Phase 1/2's "a human runs it once at the end of the phase and signs off" convention exactly.
- **Max feedback latency:** not specified in research — default to full-suite-per-wave cadence above.

---

## Per-Task Verification Map

Filled in by the planner/executor per task. Requirement → test map from research:

| Req ID | Behavior | Test Type | Automated Command | File Exists? | Status |
|--------|----------|-----------|---------------------|-------------|--------|
| PAR-01 | `parity_check.py`'s argument parsing, JSON schema, and diff-computation logic are correct | unit (Mac) | `pytest python/tests/test_parity_check.py -q` | ❌ Wave 0 | ⬜ pending |
| PAR-01 | `parity_check.py discover` smoke-tests connectivity against `mock-scheduler` | integration (Mac) | `python scripts/parity_check.py discover --mock ...` | ❌ Wave 0 — tool doesn't exist yet | ⬜ pending |
| PAR-01 | 100-prompt greedy hard gate, Qwen3-0.6B and Llama-3.2-1B-Instruct, zero-tolerance | manual-only (GPU) | `bash scripts/gpu_phase6_parity.sh` | ❌ Wave 0 — GPU-gated by nature, matches Phase 1/2 precedent; real-model, real-GPU output cannot be reproduced on the Mac | ⬜ pending |
| PAR-02 | Concurrent-load match-rate measurement, one fixed concurrency point | manual-only (GPU) | same GPU script, separate step | ❌ Wave 0 | ⬜ pending |
| (criterion 4) | 128-request cancellation stress test against the real backend; process-health watcher | manual-only (GPU) | same GPU script, reusing Phase 5's tool | ❌ Wave 0 — also blocked on Phase 5's tool existing | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `scripts/parity_check.py` — does not exist yet; needed for PAR-01/PAR-02
- [ ] `scripts/gpu_phase6_parity.sh` — does not exist yet; the human-run GPU wrapper
- [ ] `python/tests/test_parity_check.py` — Mac-side unit coverage for the diff logic, following `python/tests/test_gpu_check_script.py`'s pattern of sourcing bash helpers for testability
- [ ] Phase 5's stress-test tool (D-11 dependency) and Phase 5's HTTP/FSM code generally — does not exist in this worktree as of this research session; Phase 6 cannot execute (only plan) until it lands

*Not all gaps are closable within Phase 6's own planning — the Phase 5 dependency gap is a cross-phase sequencing fact, not a Phase 6 tooling omission.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| 100-prompt greedy-decode identical-output hard gate (Qwen3-0.6B, Llama-3.2-1B-Instruct) | PAR-01 | Real model weights + real GPU output cannot be reproduced on the Mac | Run `bash scripts/gpu_phase6_parity.sh` on the GPU machine; compare Rust vs Python frontend output byte-for-byte on 100 prompts per model at temperature 0 |
| Concurrent-load Rust-vs-Python match-rate measurement | PAR-02 | GPU batch composition affects results; informational only, not a pass/fail gate | Same GPU script, concurrent-load step; record and report match rate |
| 128-request cancellation stress test against the real backend | (criterion 4) | Requires real scheduler + real GPU process under load; verifies no crash/wedge and records whether the abort-during-prefill bug reproduces | Same GPU script, reusing Phase 5's stress-test tool against the real backend |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < {N}s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
