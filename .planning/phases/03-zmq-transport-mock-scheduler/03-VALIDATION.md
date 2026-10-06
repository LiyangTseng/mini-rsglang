---
phase: "03"
slug: "zmq-transport-mock-scheduler"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-10-05"
validated: "2026-10-06"
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `03-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (plain `#[test]`/integration tests), workspace-wide — no `cargo-nextest` wired in yet despite `.claude/CLAUDE.md` recommending it |
| **Config file** | none — `scripts/check_all.sh` runs `cargo test --workspace` |
| **Quick run command** | `cargo test -p rsg-server --test ordering_proptest` / `cargo test -p rsg-server --test dispatch_backpressure` |
| **Full suite command** | `scripts/check_all.sh --offline` |
| **Estimated runtime** | `cargo test -p rsg-server`: ~10.9s (74 tests); `ordering_proptest`'s 64-case property test is ~62% of that (~6.75s). Full `scripts/check_all.sh --offline` (cargo workspace + 172 pytest + fixtures + decode + upstream-tree check): under 2 min |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p rsg-server`
- **After every plan wave:** Run `scripts/check_all.sh`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** not specified in research — default to full-suite-per-wave cadence above

---

## Per-Task Verification Map

Requirement → test map, expanded from the Wave-0 sketch to the full set actually built across all 6 plans:

| Req ID | Behavior | Test Type | Automated Command | File Exists | Status |
|--------|----------|-----------|-------------------|-------------|--------|
| MOCK-01 | `mock-scheduler` binds scheduler-side sockets, announces readiness, echoes tokens; process contract (exit codes, observe file, stdin-EOF guard, signals) | integration (real subprocess) | `cargo test -p rsg-server --test mock_scheduler_process` | ✅ 03-01 | ✅ green (10 passed) |
| WIRE-03 | Submit through the single ordered writer; replies routed back by uid; bare-vs-batch coalescing; 32-way concurrent routing | integration + unit | `cargo test -p rsg-server --test transport_e2e` + `cargo test -p rsg-server --lib writer::` | ✅ 03-02 | ✅ green (2 + 5 passed) |
| MOCK-01 | `mock-scheduler` reproduces late-tokens-after-abort, silently-dropped-overlong-prompt, and batched-reply behaviors, one test per behavior, plus CLI validation | integration (spawns `mock-scheduler` subprocess) | `cargo test -p rsg-server --test mock_scheduler_behaviors` | ✅ 03-03 | ✅ green (12 passed) |
| WIRE-03 | Scheduler never observes an abort before its own submit, under concurrent load; ticket cannot be forged | property (proptest, 64 cases) + integration (real subprocess) + compile_fail doctest | `cargo test -p rsg-server --test ordering_proptest` + `cargo test -p rsg-server --doc` | ✅ 03-04 | ✅ green (2 passed + 1 doctest) |
| WIRE-03 | Replies route to the correct in-flight uid; unknown uids dropped without crash; slow consumer on one uid doesn't stall others; drop accounting (event + counter + warn) | integration + unit | `cargo test -p rsg-server --test dispatch_backpressure` + `cargo test -p rsg-server --lib dispatch::` | ✅ 03-05 | ✅ green (1 + 10 passed) |
| WIRE-03 / MOCK-01 | All four phase success criteria hold together through the real mock-scheduler, writer, and dispatcher composed; rsg-server accepts the mock's handshake | integration (real subprocess, end-to-end) | `cargo test -p rsg-server --test transport_misbehavior_e2e` | ✅ 03-06 | ✅ green (4 passed) |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

Full-suite cross-check (this audit, re-run directly): `cargo test -p rsg-server` → 74 tests, 0 failed, 1 doctest passed. `cargo clippy -p rsg-server --all-targets -- -D warnings` → clean. `scripts/check_all.sh --offline` → `check_all: OK` (all 5 steps, including the pre-existing Phase 1 pytest/fixture/decode/upstream-tree suite — no regressions).

---

## Wave 0 Requirements

- [x] `rsg-server/src/lib.rs` exposing `transport` (and new `writer`/`dispatch` modules) — done in 03-01/03-02
- [x] `rsg-server/src/bin/mock-scheduler.rs` — done in 03-01
- [x] `proptest = "1.11.0"` (std only) and `rustc-hash = "2.1.3"` added to `[workspace.dependencies]` in 03-01
- [x] `tests/ordering_proptest.rs`, `tests/dispatch_backpressure.rs`, `tests/mock_scheduler_behaviors.rs` — all created (03-04, 03-05, 03-03 respectively), plus `tests/mock_scheduler_process.rs`, `tests/transport_e2e.rs`, `tests/transport_misbehavior_e2e.rs`
- [x] Proptest case-count spike (03-04): 8 cases ~1.08s, 64 cases ~6.93-7.05s across repeated runs — well under the 60s budget, so 64 was chosen as final

---

## Manual-Only Verifications

*None — all phase behaviors have automated verification (no GPU/Linux-only dependency; everything in this phase runs on the Mac mock scheduler).*

---

## Validation Audit 2026-10-06

| Metric | Count |
|--------|-------|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |

No gaps — every requirement (WIRE-03, MOCK-01) has automated, passing coverage across all 6 plans. No auditor subagent spawn was needed; audited directly against this phase's `03-VERIFICATION.md` (independently re-ran tests) and a fresh `cargo test -p rsg-server` + `scripts/check_all.sh --offline` run during this audit.

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s (full `check_all.sh --offline` completes well under 2 minutes)
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated 2026-10-06
