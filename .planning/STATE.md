---
gsd_state_version: "1.0"
current_phase: 01
current_phase_name: Vendored Base & Wire Codec
status: executing
stopped_at: Completed 01-01-PLAN.md
last_updated: "2026-10-04T02:55:23.517Z"
last_activity: 2026-10-03
last_activity_desc: Phase 01 execution started
state_head: e817254489c8ca927c217ebf8112da85469cc9af
progress:
  total_phases: 7
  completed_phases: 0
  total_plans: 6
  completed_plans: 2
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-02)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 01 — Vendored Base & Wire Codec

## Current Position

Phase: 01 (Vendored Base & Wire Codec) — EXECUTING
Plan: 3 of 6
Status: Ready to execute
Last activity: 2026-10-03 — Phase 01 execution started

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
**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 01 P02 | 6 min | 2 tasks | 9 files |
| Phase 01 P01 | 14 min | 3 tasks | 128 files |

## Accumulated Context

### Decisions

Decisions are logged in the Key Decisions table in PROJECT.md.
Recent decisions affecting current work:

- [Roadmap]: mini-sglang is vendored (not a submodule). The Python frontend is frozen, and small backend fixes shared by both modes (e.g. the readiness handshake) are allowed.
- [Roadmap]: The Rust radix cache is deferred to v2 and has no v1 phase. BENCH-01 only records radix's share of scheduler time.
- [Roadmap]: There is one minimal Rust mock scheduler only: no Python mock, no Python contract oracle, no Mac A/B rehearsal phase.
- [Roadmap]: Phase 2 (BENCH-01 profiling) runs on the GPU machine in parallel with Mac work. It informs benchmark design but does not gate the project.
- [Roadmap]: Phase 4 (tokenizer parity) does not depend on the transport and can run alongside Phase 3.
- [Phase 01]: rsg-server toolchain stays on Rust 1.99.0: zmq-sys bundled libzmq builds on it (A2 confirmed)
- [Phase 01]: rsg-server exit-code contract: 0 signal, 1 startup failure, 2 bad handshake, 3 stdin EOF
- [Phase 01]: Mac dev env is a project-local uv-managed .venv synced from the sha256-hashed requirements-mac.txt; never install into system/user Python
- [Phase 01]: Human package gate approved torch 2.9.1, numpy 2.5.3, msgpack 1.2.3, pyzmq 27.2.0, transformers 4.57.3, pytest 9.1.1, setuptools/wheel, their transitive deps, and crate thiserror 2.0.21

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

Last session: 2026-10-04T02:55:23.500Z
Stopped at: Completed 01-01-PLAN.md
Resume file: None
