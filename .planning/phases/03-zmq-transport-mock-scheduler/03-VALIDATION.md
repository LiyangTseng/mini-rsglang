---
phase: "03"
slug: "zmq-transport-mock-scheduler"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-05"
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
| **Quick run command** | `cargo test -p rsg-server ordering_proptest` / `cargo test -p rsg-server dispatch_backpressure` |
| **Full suite command** | `scripts/check_all.sh` |
| **Estimated runtime** | TBD — Wave 0 phase, no tests exist yet to time |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p rsg-server`
- **After every plan wave:** Run `scripts/check_all.sh`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** not specified in research — default to full-suite-per-wave cadence above

---

## Per-Task Verification Map

Filled in by the planner/executor per task. Requirement → test map from research:

| Req ID | Behavior | Test Type | Automated Command | File Exists | Status |
|--------|----------|-----------|-------------------|-------------|--------|
| WIRE-03 | Scheduler never observes an abort before its own submit, under concurrent load | property (proptest) + integration (real subprocess) | `cargo test -p rsg-server --test ordering_proptest` | ❌ Wave 0 | ⬜ pending |
| WIRE-03 | Replies route to the correct in-flight uid; unknown uids dropped without crash; slow consumer on one uid doesn't stall others | integration | `cargo test -p rsg-server --test dispatch_backpressure` | ❌ Wave 0 | ⬜ pending |
| MOCK-01 | `mock-scheduler` reproduces late-tokens-after-abort, silently-dropped-overlong-prompt, and batched-reply behaviors, one test per behavior | integration (spawns `mock-scheduler` subprocess) | `cargo test -p rsg-server --test mock_scheduler_behaviors` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `rsg-server/src/lib.rs` exposing `transport` (and new `writer`/`dispatch` modules) — needed before any new integration test can `use rsg_server::...`
- [ ] `rsg-server/src/bin/mock-scheduler.rs` — needed before `env!("CARGO_BIN_EXE_mock-scheduler")` resolves in tests
- [ ] `cargo add proptest --dev -p rsg-server` and `cargo add rustc-hash -p rsg-server` — neither crate is in the workspace yet
- [ ] `tests/ordering_proptest.rs`, `tests/dispatch_backpressure.rs`, `tests/mock_scheduler_behaviors.rs` — none exist yet
- [ ] Spike the manual `Runtime::block_on()`-per-`proptest!`-case pattern for acceptable wall-clock time before committing to the full test design (no mature `proptest`+`tokio` glue crate exists)

---

## Manual-Only Verifications

*None — all phase behaviors have automated verification (no GPU/Linux-only dependency; everything in this phase runs on the Mac mock scheduler).*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < {N}s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
