---
gsd_state_version: "1.0"
current_phase: 05
current_phase_name: Request Lifecycle & HTTP API
status: executing
stopped_at: Completed 05-04-PLAN.md
last_updated: "2026-10-06T22:45:45.575Z"
last_activity: 2026-10-06
last_activity_desc: Phase 05 execution started
state_head: 687f018f0c0b18b6f879456d08dc3611caf8568e
progress:
  total_phases: 7
  completed_phases: 3
  total_plans: 37
  completed_plans: 32
  percent: 43
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-06)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 05 — Request Lifecycle & HTTP API

## Current Position

Phase: 05 (Request Lifecycle & HTTP API) — EXECUTING
Plan: 5 of 9
Status: Ready to execute
Last activity: 2026-10-06 — Phase 05 execution started

Progress: [████░░░░░░] 43%

## Performance Metrics

**Velocity:**
- Total plans completed: 28
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 13 | - | - |
| 02 | 9 | - | - |
| 03 | 6 | - | - |

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
| Phase 03 P01 | 35min | 2 tasks | 10 files |
| Phase 03 P02 | 25min | 2 tasks | 4 files |
| Phase 03 P03 | 55min | 2 tasks | 2 files |
| Phase 03 P04 | 50min | 3 tasks | 2 files |
| Phase 03 P05 | 40min | 2 tasks | 2 files |
| Phase 03 P06 | 45min | 2 tasks | 1 files |
| Phase 05 P01 | 45min | 2 tasks | 15 files |
| Phase 05 P02 | 45min | 2 tasks | 2 files |
| Phase 05 P03 | 25min | 3 tasks | 6 files |
| Phase 05 P04 | 70min | 3 tasks | 4 files |

## Accumulated Context

### Decisions

Decisions are logged in the Key Decisions table in PROJECT.md.
Recent decisions affecting current work:

- [Roadmap]: mini-sglang is vendored (not a submodule). The Python frontend is frozen, and small backend fixes shared by both modes (e.g. the readiness handshake) are allowed.
- [Roadmap]: The Rust radix cache is deferred to v2 and has no v1 phase. BENCH-01 only records radix's share of scheduler time.
- [Roadmap]: There is one minimal Rust mock scheduler only: no Python mock, no Python contract oracle, no Mac A/B rehearsal phase.
- [Roadmap]: Phase 2 (BENCH-01 profiling) runs on the GPU machine in parallel with Mac work. It informs benchmark design but does not gate the project.
- [Roadmap]: Phase 4 (tokenizer parity) does not depend on the transport and can run alongside Phase 3.
- [Phase 02]: Real GPU run needed no py-spy privilege grant — this WSL2 box doesn't enforce `kernel.yama.ptrace_scope`. Environment-specific, not a general claim; a box with the default `ptrace_scope=1` still needs the documented setcap/sudo/ptrace_scope remediation.
- [Phase 02]: Radix-cache share measured at 1.58%/0.76%/0.98% of scheduler time (real GPU, 3 scenarios) — recommends not clearing RADIX-01's "meaningful share" bar; author's decision, not automatic.
- [Phase 02]: Scheduler (backend) hit 93% CPU-active in the heaviest scenario (128-agent load) — already near its own ceiling there, so Rust-frontend gains are likelier to show in lower-backend-load scenarios. Phase 7 benchmark design should attribute frontend vs. backend cost separately (see PROJECT.md Key Decisions).
- [Phase 02]: Two real bugs found only against real GPU/py-spy output (neither caught by Mac stand-ins): a WSL PATH gap for nvidia-smi/nvcc on non-interactive SSH, and py-spy occasionally emitting invalid UTF-8 in unresolvable native-frame names. Both fixed with regression tests (commits `c53a4b3`, `d4272b3`).
- [Phase 02]: Code review found 4 critical bugs (process-teardown signal handling, a missing exit-code mapping, a divide-by-zero, and a subprocess-timeout gap that could discard a completed measurement run) — all fixed with regression tests (commit `0c78fe6`). 3 non-blocking warnings remain open in `02-REVIEW-DISPOSITION.md`.
- [Phase 03]: mock-scheduler opens ZmqSchedulerTransport on its own dedicated engine thread (not created in main() and moved in), and ZMQ_RECONNECT_IVL is lowered to 1ms on all Connect-role sockets in the shared open_socket helper — fixes a real ZMQ connect-before-bind race where libzmq's 100ms default reconnect interval silently delayed a socket's first message by up to ~100ms; the fix lives in shared code so every later transport consumer (Phases 5-7) inherits it
- [Phase 03]: mock-scheduler is a same-package src/bin/ binary reusing the new rsg_server library (handshake, transport); readiness travels out-of-band on stdout as the Phase 1 handshake JSON line, observation goes to a --observe-file, and neither becomes a 9th wire tag (D-08, prohibition on extending the wire schema)
- [Phase 03]: WriterHandle::abort(&Submitted) is ticket-gated: Submitted's private uid field makes "abort can never precede its own submit" structural; the abort-ordering proptest's final case count is 64 (largest of 64/32/16 under a 60s budget), measured ~7.0s across 4 runs against a real mock subprocess
- [Phase 03]: Per-uid reply channel is tokio::sync::broadcast::channel(16), drop-oldest, fixed capacity with no CLI/config knob (D-07); DispatchHandle::deregister and stats() -> DispatchStatsSnapshot {routed, unknown_uid, closed_route, malformed_frames} give Phases 5-7 the route-cleanup and drop-accounting surface
- [Phase 03]: Unknown-uid drops log at tracing::debug!, not warn!: after a mass cancellation they can number in the thousands and would flood the log; the unknown_uid counter is the signal
- [Phase 03]: Phase 3 complete — all 4 success criteria proven end-to-end (handshake/uid-routing, the abort-ordering proptest inside the gate, each mock misbehavior exercised both raw and through the transport, unknown-uid drops counted and silent uids never stall others); scripts/check_all.sh --offline green. Code review found 0 Critical/3 Warning/2 Info (all open, non-blocking); security review found 0 open threats across 23 registered; Nyquist validation confirmed full automated coverage
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
- [Phase 05]: Engine::new takes (writer, dispatch, codec, registry, config); the driver reports Received/Tokenizing/Submitted/Decoding/one-terminal through a single finish helper so LIFE-01's exactly-one-terminal invariant is structural
- [Phase 05]: Registry actor removes a uid's entry the instant it reaches a terminal state; active is simply the map length at snapshot time, so a leaked or double-terminated request is directly visible
- [Phase 05]: http_client::send() test helper writes and reads concurrently via tokio::join! on split TcpStream halves kept alive until the response is fully read, since OwnedWriteHalf shuts down the write direction on drop and an early half-close was read by the server as a client disconnect
- [Phase 05]: Human approved fastapi 0.142.2, uvicorn 0.54.0 and prompt_toolkit 3.0.53 (Task 1 checkpoint) after verifying each PyPI project links to its canonical GitHub repo
- [Phase 05]: Relock surfaced opentelemetry-api==1.45.1 as an unforeseen transitive dependency of fastapi; human separately approved it after confirming it is the CNCF open-telemetry-python project and correctly spelled
- [Phase 05]: Relock used no --upgrade flag; uv treated the existing requirements-mac.txt as preferences so all 40 pre-existing pins stayed byte-for-byte identical
- [Phase 05]: [Phase 05]: list_models is pub(crate), not pub like every other handler in rsg-server, because its return type exposes the crate-private ModelList struct
- [Phase 05]: The non-streaming chat_completions branch keeps the ActiveRequest (and its AbortGuard) alive in the handler's own future rather than spawning a background stream, so a client disconnect before the response is ready still cancels the backend request
- [Phase 05]: [Phase 05]: cancel_after_submit(engine, uid, submitted, stream, first_token_seen) is the single decision point for Immediate-vs-Deferred abort timing; both post-submit cancellation checkpoints call through it rather than duplicating the split
- [Phase 05]: deferred_wait's four outcomes (finished-token/non-finished-token/Dropped/timeout) all end Cancelled, never Decoding -> Cancelled, reported by the caller after deferred_wait returns
- [Phase 05]: finish_silent reports a terminal state without sending a RequestEvent: every cancellation path is reached only because the AbortGuard/events receiver was already dropped, so nobody is listening
- [Phase 05]: No tower-http timeout layer: the backend-inactivity deadline is a pinned, resettable tokio::time::sleep_until inside the driver, per CLAUDE.md's prohibition on tower_http::timeout for streaming routes
- [Phase 05]: An abort reaching Cancelled in the registry only means the AbortBackendMsg was enqueued onto the writer's channel, not that mock-scheduler has received and recorded it in its observe file yet; tests must poll (wait_for_abort/wait_for_observed) rather than assert immediately after a registry snapshot goes idle

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

Last session: 2026-10-06T22:45:45.527Z
Stopped at: Completed 05-04-PLAN.md
Resume file: None
