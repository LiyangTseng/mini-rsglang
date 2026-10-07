---
phase: 05-request-lifecycle-http-api
plan: 08
subsystem: api
tags: [tokenizer, hf-hub, cli, rust-frontend, launcher]

requires:
  - phase: 05-06
    provides: "rsg_server::http::{AppState::new(model, metrics), AppState::set_engine}, rsg_server::metrics::ServerMetrics, /health, /health/ready, /metrics"
  - phase: 05-07
    provides: "The full engine.rs/chat.rs/generate.rs lifecycle driver (cancellation, abort-timing, overlong rejection, backend timeout) this plan wires main.rs onto, unchanged"
  - phase: 04-06
    provides: "rsg_tokenizer::{loader::load_model_assets, encode::encode_prompt, template::build_environment, detokenize::Detokenizer} — the real HF tokenizer/chat-template/incremental-detokenizer this plan adapts onto TextCodec"
provides:
  - "rsg_server::hf_codec::HfCodec — the real TextCodec/IncrementalDecoder adapter over Phase 4's rsg-tokenizer crate"
  - "The rsg-server binary: tokenizer load before the handshake, HTTP serving from startup, Engine wired up only once the handshake succeeds, new --host/--port/--abort-timing/--backend-timeout-ms CLI flags"
  - "crates/rsg-server/tests/common/rsg_process.rs — RsgServer subprocess test harness"
  - "python/rsglang/sockets.py rust_cli_args forwards --host/--port"
affects: [05-09, 06, 07]

actuals:
  tokens: 10939
  tasks: 2
  commits: 2
  plan_head_before: 8d7e282204cdecbcdd6c3a7255d4d1ed4fc83840
  plan_head_after: 1a06ab8eecf16c21e4572d5f45c5e739e17ad922

tech-stack:
  added: []
  patterns:
    - "HfCodec is a thin adapter: every tokenization/template/detokenization call delegates straight into rsg_tokenizer (loader::load_model_assets, encode::encode_prompt, template::build_environment, detokenize::Detokenizer); no tokenization logic is reimplemented in rsg-server"
    - "A CLI-supplied --model string is adapted onto rsg_tokenizer::ModelSpec's &'static str fields via a one-time Box::leak at process startup (not a per-request leak) — ModelSpec's lifetime shape was built for Phase 4's own two hardcoded model constants, not a runtime CLI value"
    - "load_model_assets_with_retry wraps the loader call with a bounded, backed-off retry on TokenizerError::Io/HfHub only — a transient-looking error from hf-hub's own non-atomic remove-then-symlink cache-pointer bookkeeping (cache/storage.rs's create_pointer_symlink), which a real multi-process deployment can hit, not just concurrent tests"
    - "main.rs's startup order (tokenizer load, then socket open, then HTTP bind-and-serve, then the stdin handshake select) binds the HTTP listener and answers /health before the engine exists; AppState::set_engine only runs after the readiness handshake, so the idle loop also watches the serve JoinHandle and treats an unexpected serve exit as EXIT_STARTUP"

key-files:
  created:
    - crates/rsg-server/src/hf_codec.rs
    - crates/rsg-server/tests/common/rsg_process.rs
    - crates/rsg-server/tests/server_binary.rs
  modified:
    - crates/rsg-server/src/main.rs
    - crates/rsg-server/src/lib.rs
    - crates/rsg-server/Cargo.toml
    - Cargo.toml
    - crates/rsg-server/tests/cli.rs
    - crates/rsg-server/tests/common/mod.rs
    - crates/rsg-server/tests/transport_misbehavior_e2e.rs
    - python/rsglang/sockets.py
    - python/tests/test_topology.py
    - python/tests/test_launch_rust_e2e.py

key-decisions:
  - "A CLI-driven --model is always treated as gated: false when built into an ad hoc ModelSpec (every model this flag loads today is Qwen3-0.6B, a public repo); a genuinely gated model would surface as an ordinary TokenizerError auth error rather than GatedAccessUnavailable, which is a faithful default, not a silent misbehavior"
  - "The tokenizer-load retry (load_model_assets_with_retry) lives in hf_codec.rs, not in rsg-tokenizer: Phase 4's crate is out of this plan's declared scope, and the fix only needed to be visible to rsg-server's own adapter"
  - "The pre-existing Phase-1 D-10 ordering assertion in test_rust_mode_handshake_reaches_rsg_server now checks against the \"rsg-server starting\" log line instead of \"awaiting handshake on stdin\": this plan's own mandated startup order moves the latter to after the tokenizer load completes, and the test's near-instant fake scheduler (unlike a real multi-second GPU weight load) can legitimately report ready before a real network-bound tokenizer load finishes"

patterns-established:
  - "Binary-level rsg-server tests (tests/server_binary.rs) spawn both a MockScheduler and the real rsg-server binary via the new RsgServer harness, relay the mock's own handshake line to rsg-server's stdin exactly as the launcher does, and assert on /health, /health/ready and the real tokenizer's own round-tripped text — the same subprocess-harness family as tests/cli.rs and tests/common/mod.rs's MockScheduler, extended rather than duplicated"

requirements-completed: [API-01, API-02, LIFE-05]

coverage:
  - id: D1
    description: "The rsg-server binary loads Phase 4's tokenizer for --model before the handshake, binds its HTTP listener on --host/--port, serves /health (200) and /health/ready (503) while the backend is still booting, and becomes ready (/health/ready 200, generation endpoints live) only after the readiness handshake arrives on stdin"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/server_binary.rs#tracer_binary_serves_generate_through_real_tokenizer"
        status: pass
    human_judgment: false
  - id: D2
    description: "Against the mock-scheduler, the binary's POST /generate with prompt \"Hello world\" and max_tokens equal to its Qwen3-0.6B token count streams text whose concatenation is exactly \"Hello world\", then data: [DONE]\\n — the real tokenizer and incremental detokenizer through the full stack"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/server_binary.rs#tracer_binary_serves_generate_through_real_tokenizer"
        status: pass
    human_judgment: false
  - id: D3
    description: "rsg-server accepts --host, --port, --abort-timing (immediate|deferred, default immediate) and --backend-timeout-ms (default 60000, minimum 1); invalid values exit 2 through clap; a port already in use exits 1 (EXIT_STARTUP); the existing 0/2/3 exit contract is unchanged"
    requirement: "LIFE-05"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/server_binary.rs#invalid_cli_values_exit_2"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/server_binary.rs#port_in_use_exits_1"
        status: pass
      - kind: integration
        ref: "crates/rsg-server/tests/cli.rs"
        status: pass
    human_judgment: false
  - id: D4
    description: "--abort-timing and --backend-timeout-ms reach the engine: the startup log prints the effective values, and with --backend-timeout-ms 300 a request the mock never answers ends without data: [DONE] within 1.5s"
    requirement: "LIFE-05"
    verification:
      - kind: integration
        ref: "crates/rsg-server/tests/server_binary.rs#flags_reach_the_engine"
        status: pass
    human_judgment: false
  - id: D5
    description: "python -m rsglang.launch --frontend rust passes the upstream --host/--port to rsg-server, and on the Mac (fake scheduler) GET /v1/models on the forwarded port returns 200 with id Qwen/Qwen3-0.6B after the handshake"
    requirement: "API-01"
    verification:
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py#test_rust_mode_handshake_reaches_rsg_server"
        status: pass
      - kind: unit
        ref: "python/tests/test_topology.py#test_rust_cli_args_exact"
        status: pass
    human_judgment: false

duration: 90min
completed: 2026-10-07
status: complete
---

# Phase 5 Plan 8: rsg-server Becomes the Real Rust Frontend Summary

**`HfCodec` adapts Phase 4's `rsg-tokenizer` crate onto the `TextCodec`/`IncrementalDecoder` seam; `rsg-server`'s `main.rs` now loads the real Qwen3-0.6B tokenizer, serves HTTP from startup, and builds the real `Engine` only once the readiness handshake arrives — with `--host`, `--port`, `--abort-timing` and `--backend-timeout-ms` as new server-wide flags the launcher's `--frontend rust` now reaches end to end on the Mac.**

## Performance

- **Duration:** ~90 min
- **Completed:** 2026-10-07
- **Tasks:** 2
- **Files modified:** 13 (3 created, 10 modified)

## Accomplishments
- `rsg_server::hf_codec::HfCodec` loads a model through Phase 4's `loader::load_model_assets`, resolves `eos_token_id`/`clean_up_tokenization_spaces` from `tokenizer_config.json` exactly as Phase 4's own test helpers do, and implements `TextCodec`/`IncrementalDecoder` purely by delegating into `rsg_tokenizer::encode::encode_prompt`, `template::build_environment` and `detokenize::Detokenizer` — no tokenization logic is duplicated.
- `main.rs` now follows the plan's mandated startup order: tracing/signals, tokenizer load ("tokenizer loaded" log with elapsed ms), ZMQ transport open/split/writer/dispatcher, `TcpListener::bind` + `http::serve` spawn ("http server listening" log), then the stdin handshake select; on success it builds `EngineConfig` from the handshake's `max_seq_len` plus the new `--abort-timing`/`--backend-timeout-ms` flags and calls `AppState::set_engine` ("ready to serve" log with both effective values). The idle loop also watches the HTTP server's `JoinHandle` and exits 1 if it ever ends unexpectedly.
- The binary-level tracer test spawns a real `mock-scheduler` and the real `rsg-server` binary, confirms `/health` is 200 and `/health/ready`/`/generate` are 503 before the handshake, relays the mock's handshake line to `rsg-server`'s stdin, polls `/health/ready` to 200, then sends `POST /generate` with `max_tokens` computed from the real tokenizer's own encoding of "Hello world" and asserts the streamed text round-trips byte-for-byte through the real tokenizer and incremental detokenizer.
- New CLI validation and bind-failure coverage: `--abort-timing sometimes` / `--backend-timeout-ms 0` both exit 2 via clap before any socket opens; a port already held by another listener exits 1 with the bind failure named in stderr; `--abort-timing deferred --backend-timeout-ms 300` against a misbehaving mock shows up verbatim in the "ready to serve" log and produces an incomplete `/generate` stream (no `data: [DONE]`) well inside the configured timeout.
- `python/rsglang/sockets.py`'s `rust_cli_args` now forwards the upstream `--host`/`--port` the user asked for; the Mac end-to-end launcher test confirms `GET /v1/models` on the forwarded (ephemeral) port returns 200 with `id: "Qwen/Qwen3-0.6B"` once the handshake completes.

## Task Commits

Each task was committed atomically:

1. **Task 1: Tracer — the rsg-server binary serves /generate through Phase 4's real tokenizer, gated on the readiness handshake** - `6409654` (feat)
2. **Task 2: CLI validation, flag wiring, bind failure, and launcher host/port forwarding** - `1a06ab8` (feat)

**Plan metadata:** (this commit)

## Files Created/Modified
- `crates/rsg-server/src/hf_codec.rs` - `HfCodec` (`load`, `TextCodec`/`IncrementalDecoder` impls), `load_model_assets_with_retry`
- `crates/rsg-server/src/main.rs` - tokenizer load, HTTP listener bind/serve, handshake-gated `Engine`/`AppState::set_engine`, new `--host`/`--port`/`--abort-timing`/`--backend-timeout-ms` flags
- `crates/rsg-server/src/lib.rs` - `pub mod hf_codec;`
- `crates/rsg-server/Cargo.toml`, `Cargo.toml` - `rsg-tokenizer` workspace path dependency, `minijinja` dependency for rsg-server
- `crates/rsg-server/tests/cli.rs` - spawns with `--model Qwen/Qwen3-0.6B --port 0`
- `crates/rsg-server/tests/common/mod.rs` - `pub mod rsg_process;`
- `crates/rsg-server/tests/common/rsg_process.rs` - `RsgServer` subprocess harness (`spawn`, `send_line`, `wait_for_log`, `listening_addr`, `signal`, `wait_exit`)
- `crates/rsg-server/tests/server_binary.rs` - `tracer_binary_serves_generate_through_real_tokenizer`, `invalid_cli_values_exit_2`, `port_in_use_exits_1`, `flags_reach_the_engine`
- `crates/rsg-server/tests/transport_misbehavior_e2e.rs` - fixed pre-existing `--model mock-model` (now invalid) and pinned `--port 0`
- `python/rsglang/sockets.py` - `rust_cli_args` forwards `--host`/`--port`
- `python/tests/test_topology.py` - `test_rust_cli_args_exact` extended for the new flags
- `python/tests/test_launch_rust_e2e.py` - `--port 0` in `UPSTREAM_ARGS`, `GET /v1/models` assertion, relaxed D-10 ordering check

## Decisions Made
- `HfCodec::load`'s ad hoc `ModelSpec` always sets `gated: false`: every model this plan's `--model` flag loads is public (Qwen3-0.6B); a genuinely gated model surfaces as an ordinary auth error rather than a misleading `GatedAccessUnavailable`.
- `load_model_assets_with_retry` (bounded, backed-off retry on `TokenizerError::Io`/`HfHub` only) lives in `hf_codec.rs`, not in `rsg-tokenizer`, since Phase 4's crate is out of this plan's declared file scope and the fix is specific to rsg-server's own multi-process loading pattern.
- `test_rust_mode_handshake_reaches_rsg_server`'s D-10 ordering assertion now checks `"rsg-server starting"` (the very first log line) instead of `"awaiting handshake on stdin"` against `"backend ready; handshake sent"` — this plan's mandated startup order moves the latter log line to after the tokenizer load, which the test's near-instant fake scheduler can legitimately outrace.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Transient hf-hub cache-pointer race under concurrent process-level tokenizer loads**
- **Found during:** Task 1 (running `cargo test -p rsg-server --test cli`, which spawns 10 `rsg-server` subprocesses that each independently load the same cached Qwen3-0.6B tokenizer)
- **Issue:** `cargo test -p rsg-server --test cli` failed non-deterministically with `TokenizerError::Io("No such file or directory")`. Traced to hf-hub 1.0.0's `create_pointer_symlink` (`cache/storage.rs`), which does a bare `remove_file` immediately followed by `symlink` with no rename-based atomicity — when multiple OS processes resolve the same cached file concurrently, one process's `read_to_string` on the pointer path can land in the instant between another process's `remove_file` and `symlink`. Every `rsg-server` process now loads its own tokenizer independently (no cross-process coordination), so this is a real shape future multi-process deployments (e.g. Phase 7 benchmark runs) can also hit, not just this plan's own concurrent tests.
- **Fix:** Added `load_model_assets_with_retry` in `hf_codec.rs`: up to 5 attempts with a short linear backoff, retrying only `TokenizerError::Io`/`HfHub` (never a real config/content/gated-access error, which fails immediately and deterministically).
- **Files modified:** `crates/rsg-server/src/hf_codec.rs`
- **Verification:** `cargo test -p rsg-server --test cli` run 4 times in a row, 10/10 passing each time (previously failing on 3/4 runs).
- **Committed in:** `6409654` (Task 1 commit)

**2. [Rule 1 - Bug] Pre-existing test spawned rsg-server with an invalid placeholder --model**
- **Found during:** Task 2 (running `cargo test -p rsg-server`, full workspace)
- **Issue:** `transport_misbehavior_e2e.rs`'s `rsg_server_binary_accepts_mock_handshake` (a Phase 3 test, unrelated to this plan's declared files) spawned `rsg-server --model mock-model`. Before this plan, `--model` was never used to load a real tokenizer, so any string worked; now `HfCodec::load` panics inside Phase 4's loader (`repo_id.split_once('/').unwrap_or_else(|| panic!(...))`) on a model id with no `/`.
- **Fix:** Changed the spawn args to `--model Qwen/Qwen3-0.6B` (matching the convention already established in `tests/cli.rs`), and added `--port 0` since the binary now always binds a real HTTP listener.
- **Files modified:** `crates/rsg-server/tests/transport_misbehavior_e2e.rs`
- **Verification:** `cargo test -p rsg-server --test transport_misbehavior_e2e` passes (4/4).
- **Committed in:** `1a06ab8` (Task 2 commit)

**3. [Rule 1 - Bug] Pre-existing D-10 ordering assertion broken by the new tokenizer-load step**
- **Found during:** Task 2 (running `.venv/bin/python -m pytest python/tests/test_topology.py python/tests/test_launch_rust_e2e.py -q`)
- **Issue:** `test_rust_mode_handshake_reaches_rsg_server` asserted `index_of("awaiting handshake on stdin") < index_of("backend ready; handshake sent")` (a Phase 1 D-10 check). This plan's mandated startup order moves `"awaiting handshake on stdin"` to after the tokenizer load (a real, network-bound hf-hub call even when fully cached), so on this test's near-instant fake scheduler the ordering flipped.
- **Fix:** Changed the assertion to compare against `"rsg-server starting"` (the very first log line, emitted before the tokenizer load) instead — preserves D-10's real intent (rsg-server is spawned before the backend reports ready) without depending on tokenizer-load latency, which a real multi-second GPU weight load will always outlast anyway.
- **Files modified:** `python/tests/test_launch_rust_e2e.py`
- **Verification:** `.venv/bin/python -m pytest python/tests/test_topology.py python/tests/test_launch_rust_e2e.py -q` passes (23/23); full `python/tests` suite passes (176 passed, 37 skipped).
- **Committed in:** `1a06ab8` (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (3 Rule 1 bugs, all directly caused by this plan's own changes)
**Impact on plan:** All three fixes were necessary for the plan's own stated verification commands to pass reliably; none expand scope beyond making this plan's startup-order change work correctly against pre-existing tests and a real multi-process filesystem race. No scope creep.

## Issues Encountered
None beyond the three deviations above.

## User Setup Required
None - no external service configuration required. (Qwen/Qwen3-0.6B's tokenizer files were already present in the local HF cache from Phase 4.)

## Next Phase Readiness
- The real `rsg-server` binary, launched directly or through `python -m rsglang.launch --frontend rust`, now serves the HTTP API through the real tokenizer and detokenizer against the mock, with health/readiness checks and the D-01 abort-timing flag — exactly the binary plan 05-09 will byte-diff against the Python fixtures, and the one Phase 6 runs on the GPU box.
- `--abort-timing` is rsg-server's own flag (D-01, complete here); forwarding it through the launcher is deliberately left to Phase 6, which decides the benchmark setting.
- No blockers.

## Self-Check: PASSED

All created/modified files verified present on disk; both task commits
(`6409654`, `1a06ab8`) verified present in `git log`.

---
*Phase: 05-request-lifecycle-http-api*
*Completed: 2026-10-07*
