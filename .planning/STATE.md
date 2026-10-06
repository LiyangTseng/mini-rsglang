---
gsd_state_version: "1.0"
current_phase: 05
current_phase_name: request-lifecycle-http-api
status: executing
stopped_at: Phase 5 context gathered
last_updated: "2026-10-06T18:23:23.754Z"
last_activity: 2026-10-05
last_activity_desc: Phase 02 execution started
state_head: c17f9e7559c72321354e24226da0ca4863c77a2b
progress:
  total_phases: 7
  completed_phases: 1
  total_plans: 37
  completed_plans: 22
  percent: 14
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-02)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 02 — Python Frontend Baseline Profile

## Current Position

Phase: 05 (request-lifecycle-http-api) — READY TO EXECUTE
Plan: 1 of 9
Status: Ready to execute
Last activity: 2026-10-05 — Phase 02 execution started

Progress: [█░░░░░░░░░] 14%

## Performance Metrics

**Velocity:**
- Total plans completed: 13
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 13 | - | - |

**Recent Trend:**
- Last 5 plans: -
- Trend: -

*Updated after each plan completion*
**Per-Plan Metrics:**

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 01 P02 | 6 min | 2 tasks | 9 files |
| Phase 01 P01 | 14 min | 3 tasks | 128 files |
| Phase 01 P03 | 5 min | 2 tasks | 8 files |
| Phase 01 P04 | 5 min | 2 tasks | 41 files |
| Phase 01 P05 | 44 min | 3 tasks | 7 files |
| Phase 01 P06 | 8 min | 2 tasks | 6 files |
| Phase 01 P07 | 20 min | 2 tasks | 4 files |
| Phase 01 P08 | 25 min | 3 tasks | 6 files |

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
- [Phase 01]: Launcher SIGINT/SIGTERM stop handlers are installed before any child spawns, so a stop during the readiness wait tears down the whole process group
- [Phase 01]: Scheduler process: a KeyboardInterrupt after the ready point ends quietly; before it, an error envelope with the traceback goes to the launcher
- [Phase 01]: Python mode (--frontend python) execs python -m minisgl; the launcher never imports minisgl or parses upstream args in python mode
- [Phase 01]: rsg-wire uses rmp-serde to_vec_named with derived internally tagged serde types; all 34 golden fixtures pass byte-for-byte, so no rmpv fallback is needed
- [Phase 01]: gen_wire_fixtures.py pins minisgl to vendor/mini-sglang/python (exit 2 otherwise); --check diffs fixture bytes and manifest keys except the generator versions block
- [Phase 01]: D-12 failure tests assert non-zero exit (1, or -9 after SIGKILL escalation); both satisfy the contract
- [Phase 01]: Launcher supervise loop reads scheduler error envelopes after ready so crashes print the traceback
- [Phase 01]: Scheduler wrapper runs a getppid watchdog (os._exit(1)) so kill -9 of the launcher leaves no scheduler
- [Phase 01]: GPU check script restores SIGINT before exec: non-interactive shells start background jobs with SIGINT ignored
- [Phase 01]: check_upstream.py verifies the fetched upstream commit root tree against KNOWN_TREES before use (T-01-17); exit 2 on mismatch
- [Phase 01]: check_upstream.py adds UPSTREAM_SHA_INVALID, OFFLINE_UNSUPPORTED and TREE_HASH_MISMATCH categories; a parse error or invalid SHA stops the check before any fetch
- [Phase 01]: test_wire_decode.py skips without DUMP_DIR inside the full suite; check_wire_decode.sh sets RSGLANG_REQUIRE_DUMP=1 so the gate cannot pass by skipping
- [Phase 01]: scripts/check_all.sh [--offline] is the Phase 1 Mac gate: cargo tests, pytest, fixture freshness, WIRE-02 decode, check_upstream.py

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 1]: Phase 1 needs GPU machine access, because launcher criterion 2 runs the real backend. `zmq` vs `zeromq` interop with pyzmq and the bind/connect topology are not yet decided. It is also unverified whether `minisgl.message` imports on macOS for golden-fixture export.
- [Phase 4]: minijinja must cover the Qwen3 and Llama-3 templates. Two open choices: export the effective tokenizer from Python or load the raw `tokenizer.json`, and how Llama-3.x sets `clean_up_tokenization_spaces`. Access to the gated Llama-3.x repo is needed.
- [Phase 5]: It is not yet known how quickly hyper/axum detects a client disconnect while a request is queued.
- [Phase 6]: The upstream abort-during-prefill double free comes from code reading only. If it reproduces, the abort-timing setting (LIFE-05) must apply equally to the baseline.

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 261004-vqo | Fix CR-01: SIGKILL the process group on an unanticipated rust-mode launcher error | 2026-10-05 | a7ea175 | [261004-vqo-fix-cr-01-critical-finding-2026-10-05-in](./quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/) |

## Deferred Items

Items acknowledged and deferred at milestone close, most recent first:

| Category | Item | Status | Deferred At | Milestone |
|----------|------|--------|-------------|-----------|
| *(none)* | | | | |

## Session Continuity

Last session: 2026-10-06T08:30:39.298Z
Stopped at: Phase 5 context gathered
Resume file: .planning/phases/05-request-lifecycle-http-api/05-CONTEXT.md
