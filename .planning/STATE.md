---
gsd_state_version: "1.0"
current_phase: 06
current_phase_name: GPU End-to-End Parity
status: executing
stopped_at: Completed 06-02-PLAN.md
last_updated: "2026-10-07T03:38:22.252Z"
last_activity: 2026-10-06
last_activity_desc: Phase 06 execution started
state_head: 7e2f457be1a8f6ef5689996b3f385cb236ca13b6
progress:
  total_phases: 7
  completed_phases: 4
  total_plans: 42
  completed_plans: 36
  percent: 57
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-06)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 06 — GPU End-to-End Parity

## Current Position

Phase: 06 (GPU End-to-End Parity) — EXECUTING
Plan: 3 of 8
Status: Ready to execute
Last activity: 2026-10-06 — Phase 06 execution started

Progress: [██████░░░░] 57%

## Performance Metrics

**Velocity:**
- Total plans completed: 34
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 13 | - | - |
| 02 | 9 | - | - |
| 03 | 6 | - | - |
| 04 | 6 | - | - |

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
| Phase 04 P01 | 50min | 2 tasks | 17 files |
| Phase 04 P02 | 20min | 2 tasks | 5 files |
| Phase 04 P03 | 25min | 2 tasks | 4 files |
| Phase 04 P04 | 20min | 2 tasks | 7 files |
| Phase 04 P05 | 30min | 2 tasks | 5 files |
| Phase 04 P06 | 25min | 2 tasks | 6 files |
| Phase 06 P01 | 55min | 2 tasks | 10 files |
| Phase 06 P02 | 20min | 2 tasks | 2 files |

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
- [Phase 04]: hf-hub 1.0.0 blocking API confirmed via docs.rs: HFClientSync::new()?.model(owner,name).download_file().filename(name).send()? -> PathBuf; blocking feature maps to tokio/rt only
- [Phase 04]: tokenizers 0.22.2 has no dedicated Error type (Result<T, Box<dyn Error+Send+Sync>>, confirmed via docs.rs); special_tokens_map.json is fetched best-effort since Qwen3-0.6B's repo has none (404), matching AutoTokenizer.from_pretrained's own tolerance
- [Phase 04]: minijinja's tojson filter overridden to match transformers' json.dumps separator spacing (Python's default ', '/': ' separators), since minijinja's built-in tojson is fully compact and diverges from the real oracle on every tool-call/arguments rendering
- [Phase 04]: eos_token_id is derived via tokenizer.token_to_id(eos_token) rather than a new ModelSpec field
- [Phase 04]: clean_up_tokenization applies to read_str/surr_str independently before the char-safe slice, matching Python's batch_decode internal behavior (unexercised by Qwen3, wired for Llama in 04-06)
- [Phase 04]: no-panic proptest uses TestRunner directly (not the proptest! macro) to fetch the real tokenizer once and clone it per case instead of 100x
- [Phase 04]: chrono approved via blocking-human package-legitimacy checkpoint before being added to the workspace (not in RESEARCH.md's audited six)
- [Phase 04]: Live canonical Llama-3.2-1B-Instruct spot-check found add_bos_token absent from tokenizer_config.json (diverging from RESEARCH.md mirror assumption); double-BOS risk confirmed real anyway via tokenizer.json's post-processor
- [Phase 04]: Llama chat-prompt fixture frozen-clock detection is template-content-based (strftime_now substring in chat_template), never model-identity-based, so Qwen3 is unaffected by construction
- [Phase 04]: GatedAccessError is raised only when a gated model's load failure cause-chain contains huggingface_hub's GatedRepoError/RepositoryNotFoundError, confirmed against real hf-hub/transformers source, not a bare except Exception
- [Phase 04]: Real Llama BOS count is 2 (not D-10's assumed 1), confirmed empirically against the canonical gated tokenizer -- the Rust test asserts 2, documenting the discrepancy rather than normalizing it
- [Phase 04]: cargo test -p rsg-tokenizer requires --test-threads=1 to be deterministic (pre-existing env-var/cache-lock races, unrelated to TOK-04); logged to deferred-items.md, not fixed in this plan's scope
- [Phase 06]: Phase 06: one tap (generated sitecustomize shim outside vendor/) and aiohttp instead of RESEARCH's two taps / openai SDK; tdd-red-evidence skipped for pytest (Node-TAP-format-only tool, workflow.tdd_mode disabled), RED verified manually
- [Phase 06]: D-12 process-health watcher (scripts/gpu_phase6_watch.sh): new standalone script copying gpu_phase1_check.sh's alive/gpu_pids/on_gpu helper semantics rather than sourcing it (preserves Phase 1's signed-off artifact); Task 2's zombie/restart/nvsmi-failure/usage tests passed against Task 1's implementation unmodified since Task 1 already specified the full counter set.

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 1]: Phase 1 needs GPU machine access, because launcher criterion 2 runs the real backend. `zmq` vs `zeromq` interop with pyzmq and the bind/connect topology are not yet decided. It is also unverified whether `minisgl.message` imports on macOS for golden-fixture export.
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

Last session: 2026-10-07T03:38:22.196Z
Stopped at: Completed 06-02-PLAN.md
Resume file: None
