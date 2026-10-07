---
gsd_state_version: "1.0"
current_phase: 07
current_phase_name: Frontend Benchmarks
status: executing
stopped_at: Reconciled this worktree onto origin/main (which had meanwhile finished both Phase 5 and Phase 6 while this phase-7 session was running); resuming 07-10 at Task 3, now unblocked
last_updated: "2026-10-07T07:30:00.000Z"
last_activity: 2026-10-07
last_activity_desc: Reconciled worktree history onto origin/main (Phase 6 landed); resuming 07-10 Task 3
state_head: d69ef77
progress:
  total_phases: 7
  completed_phases: 6
  total_plans: 61
  completed_plans: 60
  percent: 86
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-10-07)

**Core value:** Serving through the Rust frontend produces output identical to the Python frontend on the same backend. A reproducible benchmark harness measures how much the Rust frontend improves each of the three host-overhead-bound scenarios.
**Current focus:** Phase 07 — Frontend Benchmarks

## Current Position

Phase: 07 (Frontend Benchmarks) — EXECUTING
Plan: 10 of 10
Status: Resuming at Task 3 (D-03 Mac dev pass + phase gate) — this worktree was reconciled onto `origin/main`, which had meanwhile finished Phase 5 (Request Lifecycle & HTTP API) AND Phase 6 (GPU End-to-End Parity, now complete and human-check approved: PAR-01/PAR-02 128/128, zero divergence) while this phase-7 session was in progress, so Task 3's precondition is now met. Tasks 1-2 (rsg-mock-stack, gpu_phase7_bench.sh) were already complete and are carried forward by this reconciliation.
Last activity: 2026-10-07 — Reconciled worktree history onto origin/main (Phase 6 landed); resuming 07-10 Task 3

Progress: [████████▌░] 86%

## Performance Metrics

**Velocity:**
- Total plans completed: 52
- Average duration: -
- Total execution time: 0.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01 | 13 | - | - |
| 02 | 9 | - | - |
| 03 | 6 | - | - |
| 04 | 6 | - | - |
| 05 | 9 | - | - |
| 07 | 9 | - | - |

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
| Phase 05 P01 | 45min | 2 tasks | 15 files |
| Phase 05 P02 | 45min | 2 tasks | 2 files |
| Phase 05 P03 | 25min | 3 tasks | 6 files |
| Phase 05 P04 | 70min | 3 tasks | 4 files |
| Phase 05 P05 | 55min | 2 tasks | 20 files |
| Phase 05 P06 | 50min | 2 tasks | 11 files |
| Phase 05 P07 | 50min | 3 tasks | 3 files |
| Phase 05 P08 | 90min | 2 tasks | 13 files |
| Phase 05 P09 | 70min | 2 tasks | 3 files |
| Phase 06 P01 | 55min | 2 tasks | 10 files |
| Phase 06 P02 | 20min | 2 tasks | 2 files |
| Phase 06 P03 | 50min | 2 tasks | 5 files |
| Phase 06 P04 | 75min | 3 tasks | 5 files |
| Phase 06 P05 | 20min | 3 tasks | 8 files |
| Phase 06 P06 | 75 min | 1 tasks | 3 files |
| Phase 06 P06 | 90min | 1 tasks | 10 files |
| Phase 06 P07 | ~50 min | 1 tasks | 3 files |
| Phase 06 P08 | ~2h55m | 4 tasks | 13 files |
| Phase 07 P01 | 55min | 2 tasks | 11 files |
| Phase 07 P02 | 20min | 2 tasks | 2 files |
| Phase 07 P03 | 25min | 2 tasks | 3 files |
| Phase 07 P04 | 17min | 3 tasks | 7 files |
| Phase 07 P05 | 15min | 3 tasks | 6 files |
| Phase 07 P06 | 50min | 3 tasks | 14 files |
| Phase 07 P07 | 40min | 3 tasks | 13 files |
| Phase 07 P08 | 90min | 3 tasks | 12 files |
| Phase 07 P09 | 55min | 2 tasks | 9 files |

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
- [Phase 06]: 06-03: 128-item parity corpus built with 16 items reused from Phase 4's tokenizer corpus (marked phase4:<path>#<id>), 112 original (phase6); corpus.py's validate_canonical enforces exact per-category counts/properties, wired into load_corpus only for the canonical path
- [Phase 06]: 06-03: compare.py's full D-05 precedence (request_error > tokenization > sampling_params > backend > incomplete > detokenization_or_api) and annotate_sequence's radix-cache note are implemented; wiring annotate_sequence into parity_check.py run is deferred to plan 06-05 per the plan's key_links contract
- [Phase 06]: 06-04: concurrent sweep (PAR-02), endpoint check (criterion 1), multi-model gated handling and GPU-only require_gpu validation; fixed two real bugs found while proving the plan's own tests -- ThreadingHTTPServer's default request_queue_size (5) silently dropped connections under concurrency 8, and sidecar.build_meta never set meta.gpu so require_gpu could never pass even on a real GPU box
- [Phase 06]: 06-05: backend window probe (uids >= 2**40, upstream's own wire encoder) sends UserMsg/AbortBackendMsg directly to the scheduler's backend socket to exercise the abort-during-prefill window independently of frontend disconnect-detection latency; reproduced/conclusive fold in both the stress run's tap evidence and the probe's findings
- [Phase 06]: 06-05: failure_mode precedence (crash > wedge > corrupted_requests > double_free > none) reduces a stress run's watcher+tap evidence to one evidence-backed outcome, so a false-PASS from liveness alone (RESEARCH Pitfall 2) cannot happen
- [Phase 05]: Engine::new takes (writer, dispatch, codec, registry, config); the driver reports Received/Tokenizing/Submitted/Decoding/one-terminal through a single finish helper so LIFE-01's exactly-one-terminal invariant is structural
- [Phase 05]: Registry actor removes a uid's entry the instant it reaches a terminal state; active is simply the map length at snapshot time, so a leaked or double-terminated request is directly visible
- [Phase 05]: http_client::send() test helper writes and reads concurrently via tokio::join! on split TcpStream halves kept alive until the response is fully read, since OwnedWriteHalf shuts down the write direction on drop and an early half-close was read by the server as a client disconnect
- [Phase 05]: Human approved fastapi 0.142.2, uvicorn 0.54.0 and prompt_toolkit 3.0.53 (Task 1 checkpoint) after verifying each PyPI project links to its canonical GitHub repo
- [Phase 05]: Relock surfaced opentelemetry-api==1.45.1 as an unforeseen transitive dependency of fastapi; human separately approved it after confirming it is the CNCF open-telemetry-python project and correctly spelled
- [Phase 05]: Relock used no --upgrade flag; uv treated the existing requirements-mac.txt as preferences so all 40 pre-existing pins stayed byte-for-byte identical
- [Phase 05]: list_models is pub(crate), not pub like every other handler in rsg-server, because its return type exposes the crate-private ModelList struct
- [Phase 05]: The non-streaming chat_completions branch keeps the ActiveRequest (and its AbortGuard) alive in the handler's own future rather than spawning a background stream, so a client disconnect before the response is ready still cancels the backend request
- [Phase 05]: cancel_after_submit(engine, uid, submitted, stream, first_token_seen) is the single decision point for Immediate-vs-Deferred abort timing; both post-submit cancellation checkpoints call through it rather than duplicating the split
- [Phase 05]: deferred_wait's four outcomes (finished-token/non-finished-token/Dropped/timeout) all end Cancelled, never Decoding -> Cancelled, reported by the caller after deferred_wait returns
- [Phase 05]: finish_silent reports a terminal state without sending a RequestEvent: every cancellation path is reached only because the AbortGuard/events receiver was already dropped, so nobody is listening
- [Phase 05]: No tower-http timeout layer: the backend-inactivity deadline is a pinned, resettable tokio::time::sleep_until inside the driver, per CLAUDE.md's prohibition on tower_http::timeout for streaming routes
- [Phase 05]: An abort reaching Cancelled in the registry only means the AbortBackendMsg was enqueued onto the writer's channel, not that mock-scheduler has received and recorded it in its observe file yet; tests must poll (wait_for_abort/wait_for_observed) rather than assert immediately after a registry snapshot goes idle
- [Phase 05]: python_frontend.py reproduces upstream's start_subprocess spawn (detokenizer/tokenizer tokenize_worker processes) minus scheduler ranks, against an externally started mock-scheduler stand-in on the caller's --rsg-suffix addresses
- [Phase 05]: gen_api_fixtures.py captured the 18-case API-01 golden fixture set from one fresh live run; chat_nonstream_eos_final's max_tokens is computed at capture time from the chat template so the echoed final token lands on EOS, exercising detokenize.py's finished+EOS exclusion
- [Phase 05]: Treated the Task 1 precondition as met via a direct read-only check (local_files_only=True) rather than its literal HF_HUB_OFFLINE=1 command, which fails in this environment on a transformers 4.57.3 bug (_patch_mistral_regex calling model_info() for any large-vocab repo-id tokenizer) unrelated to cache completeness
- [Phase 05]: ServerMetrics uses a per-server PrometheusRecorder (never process-global, no set_global_recorder/install_recorder) with no labels on any of its 7 series (T-05-13/T-05-14 fixed cardinality)
- [Phase 05]: /health is process liveness only (always 200); /health/ready is 200 only after AppState::set_engine — the front-half/end-to-end split Phase 7's cold-start scenario measures
- [Phase 05]: ServerMetrics::render sets rsg_late_tokens_dropped_total via Counter::absolute(unknown_uid + closed_route) from a live DispatchStatsSnapshot at scrape time, never an internally-accumulated count, so it can never drift from the dispatcher's own single source of truth
- [Phase 05]: No driver fix was needed in engine.rs for the 128-agent stress test: the existing cancellation/abort-timing/timeout logic from plan 05-04 held up across repeated runs and multiple seeds
- [Phase 05]: A disconnect-mode stress-test agent picks OpenStream vs. a raw TcpStream based on when the server actually commits to response headers, not just streaming-vs-non-streaming as a label: chat non-stream has no headers until the whole generation is ready, so OpenStream::open would block past the intended disconnect point
- [Phase 05]: HfCodec::load's ad hoc ModelSpec always sets gated: false for a CLI-supplied --model; a genuinely gated model surfaces as an ordinary auth error instead of a misleading GatedAccessUnavailable
- [Phase 05]: Transient hf-hub cache-pointer race (non-atomic remove+symlink in create_pointer_symlink) under concurrent process-level tokenizer loads fixed with a bounded, backed-off retry in hf_codec.rs (rsg-tokenizer itself is out of plan scope)
- [Phase 05]: test_rust_mode_handshake_reaches_rsg_server's D-10 ordering check now compares against rsg-server's first log line (rsg-server starting) instead of awaiting handshake on stdin, since 05-08 moved the latter after the tokenizer load
- [Phase 05]: engine.rs builds the per-request IncrementalDecoder (clones the real tokenizer) before register/submit, not after -- doing it after let a zero-decode-delay backend overflow the per-uid broadcast buffer (capacity 16, drop-oldest) before the decode loop's first recv(), deterministically dropping tokens on any response over 16 tokens
- [Phase 05]: crates/rsg-server/tests/api_parity.rs replays all 18 fixtures/api cases against the real rsg-server binary on mock-scheduler, byte-diffing status/content-type/body (created normalized) -- API-01 is now proven end to end on the Mac; scripts/check_all.sh gained a 7th step (API fixture freshness) keeping the gate honest about both frontends
- [Phase 06]: 06-06: Task 1 (GPU wrapper tracer) complete and committed; Task 2 halted -- Phase 5's stress_128.rs has no way to target an already-running server, and D-11 forbids changing it in this phase. Plan marked status: halted, blocking 06-07/06-08 until a human/re-plan decision resolves the gap.
- [Phase 06]: 06-06 Task 2 checkpoint resolved: built a new, purpose-built external-target stress driver (rsglang.parity.stress_client) instead of reusing/modifying stress_128.rs, which has no external-target mode and D-11 forbids changing
- [Phase 06]: 06-06: rsglang.launch gains its own --abort-timing flag, forwarded to rsg-server via sockets.rust_cli_args -- 05-08-SUMMARY.md left this forwarding deliberately for Phase 6 to decide
- [Phase 06]: 06-06: rsglang.testing.rust_frontend (new, Mac-only) wires a real rsg-server binary to a real mock-scheduler subprocess, forwarding the handshake stdout->stdin; the existing RSGLANG_SCHEDULER_FACTORY=FakeScheduler harness cannot drive real generations (its run_forever never answers UserMsg)
- [Phase 06]: Reported Criterion 2 as a genuine FAIL (Qwen3-0.6B 127/128) rather than softening it; both mismatches bisected to prompt edge-08 with per-model hypotheses (max_tokens-cap off-by-one vs end-of-turn-token off-by-one) — **superseded, see the next two entries**
- [Phase 06]: Investigated both edge-08 one-token-short mismatches with a deterministic Mac-side reproduction (`crates/rsg-server/tests/backend_finish_boundary.rs`) before accepting the FAIL at face value; found the Rust frontend's engine/dispatch pair has no independent stopping logic of any kind and never second-guesses the backend's own `finished` flag — this finding stands unchanged by the correction below. The *explanation* for the mismatch (GPU backend run-to-run nondeterminism, recorded in the original 06-08-PLAN.md Task 0 and parity-report.md) was wrong; see the next entry.
- [Phase 06]: The true cause of the apparent 127/128 PAR-01 mismatch (both models, prompt edge-08) was a bug in the parity test harness itself, not GPU nondeterminism and not a frontend defect: `python/rsglang/parity/sweep.py`'s `join_sequential`/`join_concurrent` counted a post-finish straggler `detok` record (emitted by the scheduler's pipelined execution after it already sent `finished=true`, raced against session teardown) as an extra output token on the Python side only — even though the real HTTP response text both frontends sent was already byte-identical. Fixed by `_bounded_detoks()` (commit `ae8feec`), which truncates a uid's detok records at the first `finished=true` record, with two regression tests in `python/tests/test_parity_check.py`. A full GPU re-run with the fix in place (on top of the D-09 branch-C `deferred` default below) shows **128/128 for both models, zero divergence anywhere** — PAR-01's hard gate genuinely passes; no `## PAR-01 disposition (D-05)` was needed since the gate did not fail. See `docs/benchmarks/parity-report.md`'s corrected "PAR-01 off-by-one investigation" section for the full account.
- [Phase 06]: abort-timing default for Phase 7 = deferred (D-09 branch C; reproduced=yes, conclusive=yes, immediate=crash, deferred=none, probe double frees=0; evidence docs/benchmarks/parity-report.md)
- [Phase 06]: D-09 branch C chosen: --abort-timing default changed to deferred (project-wide); the vendored scheduler stays pristine. See STATE.md Decisions and docs/benchmarks/parity-report.md's Abort-timing decision (D-09) section.
- [Phase 06]: The apparent Criterion 2 (PAR-01) FAIL reported by 06-07 was a parity test harness bug (sweep.py double-counted a post-finish straggler detok record), not GPU nondeterminism and not a frontend defect; fixed in commit ae8feec. Final GPU re-run: 128/128 for both models, zero divergence. PAR-01/PAR-02 genuinely pass; no disposition was needed.
- [Phase 07]: rsg-bench: full CancelPlan implemented in Task 1 (client-side), server-side disconnect proof + pgid guards added in Task 2 TDD cycle
- [Phase 07]: reqwest default-features=false with no 'json' feature: request/response JSON handled manually via serde_json + .body()/.bytes()
- [Phase 07]: Env-var mode toggle (RSGLANG_PROFILE_MODE=gc_only) on the existing hook.py module, not a second shim: write_shim/hook_env's PYTHONPATH injection and the sitecustomize chain-load stay unchanged
- [Phase 07]: Proc-name records read multiprocessing from sys.modules (never import it from the hook thread) and emit only on name change, gated to gc_only mode only
- [Phase 07]: standard_throughput.py's Task 2 drift-guard tests all passed on first run against the Task 1 implementation (same plan); no feat/refactor commit was needed, documented as expected rather than a TDD violation
- [Phase 07]: loadgen.rs Task1/Task2 code was written together then split back into per-task commits (Task1-only subset first, open-loop/closed-concurrency delta second) to preserve atomic commits without interactive git staging
- [Phase 07]: welch_diff_ci95 short-circuits to half_width=0.0/df=na+nb-2 on zero combined variance, avoiding a 0.0/0.0 NaN
- [Phase 07]: classify() checks process-name match before hook-reported mp name before the pgid-leader fallback, so a named process's signal always wins over the generic leader heuristic
- [Phase 07]: memory::{memory_by_group, memory_at} treat a zero-member group in a sample as a real RSS/PSS sum of 0, not a sentinel; the never-fake-PSS rule applies only to a failed read
- [Phase 07]: rsg-bench cmdline::render makes a stray {num_tokenizer} in --rust-cmd a render error for free (D-10), via the same unknown-placeholder rule used for every other template
- [Phase 07]: redact_argv runs a flat token-boundary pass then recurses into any multi-word token (the harness's own quoted --python-cmd/--rust-cmd value), because a shell delivers that value as one opaque argv element
- [Phase 07]: Task 2/3 TDD discipline collapsed into test-only commits verified against Task 1's already-complete shared orchestrator/manifest implementation, documented rather than disguised as a true RED-then-GREEN cycle
- [Phase 07]: rsg-bench: S2Runner pushes one MeasuredWindow per load level, so 07-06's existing per-window GC/memory/co-occurrence wiring applies per level with zero orchestrator changes
- [Phase 07]: rsg-bench crosscheck: parse_tool_result tries the whole document as JSON first, then falls back to the last non-empty JSONL line, handling both vllm's single-object and sglang's progress-then-final shapes with one function
- [Phase 07]: rsg-bench sweep-num-tokenizer: Task 2/3 used a genuine RED-GREEN TDD cycle (intentionally-wrong stubs confirmed failing on real assertions, then fixed), unlike 07-06's documented TDD-discipline collapse
- [Phase 07]: rsg-bench: ColdstartRecord gets hand-rolled Serialize/Deserialize instead of #[serde(tag="kind")] -- serde's internally-tagged-enum deserialization silently fails on a non-string map key (roles: BTreeMap<i32, Role>) nested inside it
- [Phase 07]: rsg-bench: orchestrator::run_session's RunnerManaged branch (build_trial_env, run_one_trial_runner_managed, run_one_trial_dispatch) wired up for the first time -- 07-06 defined the Lifecycle enum but no runner used RunnerManaged until 07-08's S3Runner (hyperfine-wrapped coldstart-once/-stop)
- [Phase 07]: rsg-bench: bench-stub's --marker-after-ms fixed to be relative to process start (same clock as --ready-delay-ms), not listener-bind time, so a marker can land before readiness as 07-08's test oracle requires
- [Phase 07]: rsg-bench: standard_throughput's driver subprocess gets its own PYTHONPATH=<repo>/python, independent of the gc-hook env the orchestrator computed for the frontend-under-test
- [Phase 07]: rsg-bench report: num_tokenizer_sweep's D-10 'default' is the smallest --num-tokenizer candidate in the sweep's own arm list, since a sweep session has no python-default arm to read also_best from
- [Phase 07]: [Phase 07, 07-10]: rsg-mock-stack's two children inherit its own process group (no process_group override), so the harness's own killpg teardown already covers them (T-07-24)
- [Phase 07]: [Phase 07, 07-10]: gpu_phase7_bench.sh's sweep-num-tokenizer step relies on clap's own --backend-kind=Real default rather than passing it explicitly, so the sweep manifest is still backend_kind real without violating the dry-run behavior contract (which forbids --backend-kind on the sweep line)
- [Phase 07, reconciliation]: This worktree's own phases 1-5 lineage never finished (the phase-7 session started before that work landed on main). Reconciled by branching fresh from `origin/main` (ground truth for phases 1-5, squash-merged via PRs #3/#4/#5/#7/#8) and carrying forward only phase 7's new files (entire `crates/rsg-bench`, `.planning/phases/07-frontend-benchmarks/`, `python/rsglang/bench/`, three new test files, `scripts/gpu_phase7_bench.sh`) plus a hand-reapplied patch for 07-02's changes to `python/rsglang/profiling/hook.py` (confirmed byte-identical base, applied clean). Workspace `Cargo.toml` gained `hdrhistogram`/`nix`/`sysinfo`; `reqwest` and `tokio` were left at origin/main's settings since `rsg-tokenizer` depends on reqwest's default TLS features and rsg-bench's own crate-level feature additions already layer cleanly on top.
- [Phase 07, reconciliation]: A second rebase was needed shortly after the first: Phase 6 (GPU End-to-End Parity) landed on `origin/main` (PR #9) while this phase-7 session was still running. Rebased `phase7-on-main` onto the new `origin/main` tip; only `.planning/ROADMAP.md`/`STATE.md`/`state.json` conflicted (both phases advanced the same tracking docs independently from the same Phase-5 base) — resolved by combining both phases' content rather than picking a side.

### Pending Todos

None yet.

### Blockers/Concerns

- [Phase 6]: The abort-during-prefill double free was reproduced empirically as a scheduler process crash (watcher verdict `unhealthy`: 1 zombie sample, 1 GPU-unlisted sample out of 9) under `--abort-timing immediate`, with `failure_mode: none` under `deferred`; zero double-free/collision evidence in either run or the 72-trial window probe (1 `prefill_window` hit, at `delay_ms=1`). D-09 branch C: the project-wide default is changed to `deferred` (`crates/rsg-server/src/main.rs`); the vendored scheduler stays pristine. See `docs/benchmarks/parity-report.md`'s "Abort-timing decision (D-09)" section and `UPSTREAM.md`'s "Known upstream issues" entry.
- [Phase 6]: the frozen Python frontend has no abort-timing switch (it aborts only after a chunk plus 0.1 s); Phase 7 must state how the A/B comparison stays fair under the chosen `deferred` default.
- [Phase 4, pre-existing tech debt]: `cargo test -p rsg-tokenizer`'s `loader::tests::gated_access_unavailable_*` tests race under default parallel test threads (global env-var mutation between concurrently-run tests in that crate); deterministic on this machine. `scripts/check_all.sh --offline` does not pin `--test-threads=1` internally, so it can fail on this specific crate even when nothing in the phase under test is actually broken — confirm with `cargo test -p rsg-tokenizer --lib -- --test-threads=1` before trusting a `check_all.sh` red on this crate. Logged to `.planning/phases/04-tokenizer-detokenizer-parity/deferred-items.md`; not yet fixed.
- [Phase 5, code review WR-01, open]: `drive_request`'s `IncrementalDecoder` construction (full tokenizer vocab/merge clone) runs synchronously on the async driver task with no `.await` — can starve other concurrent requests' token streams under load. Worth a look before Phase 7's benchmark numbers are trusted at high concurrency; see `05-REVIEW.md`/`05-REVIEW-DISPOSITION.md`.

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

Last session: 2026-10-07T07:30:00.000Z
Stopped at: Reconciled worktree onto origin/main (Phase 6 now also landed); resuming 07-10 at Task 3
Resume file: .planning/phases/07-frontend-benchmarks/07-10-PLAN.md
