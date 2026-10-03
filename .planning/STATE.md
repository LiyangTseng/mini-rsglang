---
gsd_state_version: "1.0"
current_phase: 1
current_phase_name: Vendored Base & Wire Codec
status: executing
stopped_at: Phase 1 context gathered
last_updated: "2026-10-03T08:52:16.676Z"
last_activity: 2026-10-02
last_activity_desc: Roadmap created (7 phases, 28/28 v1 requirements mapped)
state_head: 480d56440d029c9cd3ff54adcf8dcf75f6b1c0d2
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 6
  completed_plans: 0
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-02)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 1: Vendored Base & Wire Codec

## Current Position

Phase: 1 (Vendored Base & Wire Codec) — READY TO EXECUTE
Plan: 0 of TBD in current phase
Status: Ready to execute
Last activity: 2026-10-02 — Roadmap created (7 phases, 28/28 v1 requirements mapped)

Progress: [░░░░░░░░░░] 0%

## Performance Metrics

**Velocity:**
- Total plans completed: 0
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| - | - | - | - |

**Recent Trend:**
- Last 5 plans: -
- Trend: -

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in the Key Decisions table in PROJECT.md.
Recent decisions affecting current work:

- [Roadmap]: mini-sglang is vendored (not a submodule). The Python frontend is frozen, and small backend fixes shared by both modes (e.g. the readiness handshake) are allowed.
- [Roadmap]: The Rust radix cache is deferred to v2 and has no v1 phase. BENCH-01 only records radix's share of scheduler time.
- [Roadmap]: There is one minimal Rust mock scheduler only: no Python mock, no Python contract oracle, no Mac A/B rehearsal phase.
- [Roadmap]: Phase 2 (BENCH-01 profiling) runs on the GPU machine in parallel with Mac work. It informs benchmark design but does not gate the project.
- [Roadmap]: Phase 4 (tokenizer parity) does not depend on the transport and can run alongside Phase 3.

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 1]: Phase 1 needs GPU machine access, because launcher criterion 2 runs the real backend. `zmq` vs `zeromq` interop with pyzmq and the bind/connect topology are not yet decided. It is also unverified whether `minisgl.message` imports on macOS for golden-fixture export.
- [Phase 4]: minijinja must cover the Qwen3 and Llama-3 templates. Two open choices: export the effective tokenizer from Python or load the raw `tokenizer.json`, and how Llama-3.x sets `clean_up_tokenization_spaces`. Access to the gated Llama-3.x repo is needed.
- [Phase 5]: It is not yet known how quickly hyper/axum detects a client disconnect while a request is queued.
- [Phase 6]: The upstream abort-during-prefill double free comes from code reading only. If it reproduces, the abort-timing setting (LIFE-05) must apply equally to the baseline.

## Deferred Items

Items acknowledged and deferred at milestone close, most recent first:

| Category | Item | Status | Deferred At | Milestone |
|----------|------|--------|-------------|-----------|
| *(none)* | | | | |

## Session Continuity

Last session: 2026-10-03T07:35:39.810Z
Stopped at: Phase 1 context gathered
Resume file: .planning/phases/01-vendored-base-wire-codec/01-CONTEXT.md
