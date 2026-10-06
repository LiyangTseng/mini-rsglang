---
phase: "05"
slug: "request-lifecycle-http-api"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-10-06"
---

# Phase 05 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `05-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `cargo test` (no `cargo-nextest` config present yet — `.config/nextest.toml` does not exist; `cargo-nextest` is listed in CLAUDE.md's dev tools but not yet adopted in `scripts/check_all.sh`) |
| **Config file** | none — `scripts/check_all.sh` is the Mac gate script |
| **Quick run command** | `cargo test -p rsg-server <test_name_substring>` |
| **Full suite command** | `cargo test --workspace` (step 1 of `scripts/check_all.sh`) |
| **Estimated runtime** | TBD — Wave 0 phase, no HTTP/fsm tests exist yet to time |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p rsg-server <module>`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** `scripts/check_all.sh` extended with a new fixture-freshness step for API fixtures (mirroring the existing wire-fixture step); full suite must be green
- **Max feedback latency:** not specified in research — default to full-suite-per-wave cadence above

---

## Per-Task Verification Map

Filled in by the planner/executor per task. Requirement → test map from research:

| Req ID | Behavior | Test Type | Automated Command | File Exists? | Status |
|--------|----------|-----------|---------------------|-------------|--------|
| LIFE-01 | Every uid reaches exactly one terminal state | unit | `cargo test -p rsg-server fsm::` | ❌ Wave 0 — `fsm` module doesn't exist | ⬜ pending |
| LIFE-02 | Disconnect → immediate abort; late tokens dropped+counted | integration | `cargo test -p rsg-server --test http_cancellation` | ❌ Wave 0 | ⬜ pending |
| LIFE-03 | 128-agent stress, no leaks/stuck connections | integration (throwaway stress test) | `cargo test -p rsg-server --test stress_128 -- --ignored` (long-running, gated behind `--ignored`) | ❌ Wave 0 | ⬜ pending |
| LIFE-04 | Overlong prompt 400; backend-unresponsive timeout | integration | `cargo test -p rsg-server --test http_errors` | ❌ Wave 0 | ⬜ pending |
| LIFE-05 | `--abort-timing` flag changes behavior | integration (parametrized, both flag values) | `cargo test -p rsg-server --test abort_timing` | ❌ Wave 0 | ⬜ pending |
| API-01 | Byte-parity on 4 endpoints incl. SSE framing | fixture-diff (new Python capture script + Rust-side comparison test) | `python scripts/gen_api_fixtures.py --check` (new, modeled on `gen_wire_fixtures.py`) | ❌ Wave 0 — script doesn't exist | ⬜ pending |
| API-02 | `/health`, `/health/ready`, `/metrics` respond with required series | integration | `cargo test -p rsg-server --test observability` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/rsg-server/src/fsm/` module + unit tests — the lifecycle state machine doesn't exist
- [ ] `crates/rsg-server/src/http/` module + axum `Router` — no HTTP code exists anywhere in the workspace yet
- [ ] `crates/rsg-server/tests/http_cancellation.rs`, `http_errors.rs`, `abort_timing.rs`, `observability.rs`, `stress_128.rs` — new integration test files, likely reusing/extending `tests/common/mod.rs`'s `MockScheduler` harness plus a new HTTP-client test helper
- [ ] `scripts/gen_api_fixtures.py` — new Python fixture-capture script against a live Python-frontend process, modeled on `scripts/gen_wire_fixtures.py`'s `--check` convention; must include a normalized/excluded-fields list for `created` timestamps
- [ ] Workspace `Cargo.toml` edits: add `axum`, `tower-http`, `tokio-util`, `metrics`, `metrics-exporter-prometheus`, `tokio-stream`; add `"net"` feature to the existing `tokio` dependency
- [ ] Cross-phase dependency gaps: Phase 3's per-uid dispatcher (D-04/D-05/D-06) and misbehavior flags (D-09) on `mock-scheduler` are not yet in `transport.rs`/the mock's CLI as of this research session

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
