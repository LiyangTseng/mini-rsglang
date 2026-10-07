---
phase: 06-gpu-end-to-end-parity
plan: 04
subsystem: testing
tags: [parity, concurrency, endpoints, verdict, pytest, aiohttp]

requires:
  - phase: 06-gpu-end-to-end-parity
    provides: "06-01's rsglang.parity package contract (sweep.run_session/send_sequential/join_sequential, compare.compare_prompt/summarize, sidecar schema/validate_sidecar/write_sidecar, parity_check.py run/validate CLI) and 06-03's curated 128-item canonical corpus with full D-05 layer precedence -- both consumed through their fixed signatures, unchanged"
provides:
  - "sweep.send_concurrent/join_by_input_ids: the whole corpus in flight at once per frontend, joined by input_ids against that frontend's own sequential output (no ordering assumption)"
  - "sweep.endpoint_smoke/PYTHON_ENDPOINTS/RUST_ENDPOINTS/port_free: a real HTTP check of every endpoint a frontend must answer, plus the pre-session port-busy guard"
  - "scripts/parity_check.py discover and verdict subcommands; run's endpoints/concurrent parts and multi-model gated-checkpoint handling; the --concurrency/--frontend/--server-cmd/--skip-tap-check/--criterion flags"
  - "sidecar.validate_discover and the concurrent/endpoints block schemas; validate_sidecar(require_gpu=True) now enforces mode, canonical corpus identity, both endpoint lists, and per-model/gate-model block presence"
  - "fake_parity_server.py --flavor/--diverge-under-load/--gated/--gated-models, SSE-framed streaming and /generate, and a fixed 256-entry listen backlog"
affects: [06-05-abort-stress, 06-06-gpu-script, 06-07-report, 06-08-ship]

actuals:
  tokens: 21912
  tasks: 3
  commits: 3
  plan_head_before: 12105443006769b34d4a2297d13a7687d6c0d8fc
  plan_head_after: 6454fb2920bc21e84cfd7662497aad1d47f3c5b2

tech-stack:
  added: []
  patterns:
    - "join_by_input_ids mirrors join_sequential's record shape but drops the ordering assumption: it maps tap user records to prompts by matching recorded input_ids against a frontend-owned expected-ids table, so concurrent completion order never matters"
    - "Session-failure classification (status unavailable vs failed) reads the ServerExited exception's own embedded log tail for a gated-repo/401/restricted regex match, rather than re-opening the log file -- the exception message already carries the evidence"
    - "Every new run part (endpoints, concurrent) launches its own fresh python-then-rust session pair, guarded by a port_free() check immediately before each launch, mirroring the existing sequential part's fresh-session-per-frontend discipline"

key-files:
  created: []
  modified:
    - python/rsglang/parity/sweep.py
    - python/rsglang/parity/sidecar.py
    - scripts/parity_check.py
    - python/rsglang/testing/fake_parity_server.py
    - python/tests/test_parity_check.py

key-decisions:
  - "Found and fixed a real concurrency bug while proving Task 1 end to end: Python's ThreadingHTTPServer defaults request_queue_size to 5 (from socketserver.BaseServer); at --concurrency 8 the OS silently dropped/refused the backlog overflow, producing spurious \"no tap user record\" joins that looked like backend divergence but were actually lost connections. Fixed by subclassing with request_queue_size=256 and daemon_threads=True -- a Rule 1 bug fix, not a plan deviation, since it was required for the plan's own tracer test to be reliable."
  - "sidecar.build_meta previously never set meta.gpu, so validate_sidecar(require_gpu=True) could never pass even on a real GPU box -- the require_gpu platform/gpu check silently depended on a key that was never written. Added \"gpu\": sidecar._gpu_name() to build_meta's return (Rule 1 bug fix, discovered while implementing Task 3's require_gpu extension)."
  - "Report-model (non-gate) sequential status/match-rate no longer sets any_failure on its own -- only the gate model's status/match-rate and any genuinely \"failed\" (non-gated) session do. This matches Task 3's exit-code contract exactly: an unavailable report-model checkpoint is recorded with a warning and judged later by verdict --criterion 2, not by run's own exit code."
  - "endpoint_smoke and the streaming/ /generate framing checks are implemented as plain async helper closures over one aiohttp.ClientSession rather than a class, matching send_sequential/send_concurrent's existing module-level-function style in this file."

requirements-completed: [PAR-01, PAR-02]

coverage:
  - id: D1
    description: "run --parts sequential,concurrent --concurrency 128 sends the whole corpus at once to a fresh Python session and then a fresh Rust session, reporting Rust-vs-Python and each frontend's own vs-sequential agreement as sidecar-recorded integer counts; the concurrent block never changes the run's exit code except on a session that fails to start"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_concurrent_tracer"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_concurrent_requires_sequential"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_concurrency_bounds"
        status: pass
    human_judgment: false
  - id: D2
    description: "Each concurrent-run tap user record is joined to its prompt through that frontend's own sequential input_ids; records whose input_ids match no prompt are counted in summary.unmatched_tap instead of being silently dropped"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_concurrent_tracer"
        status: pass
    human_judgment: false
  - id: D3
    description: "discover --frontend {python,rust} proves a fresh session answers every endpoint that frontend must serve (including streaming/generate SSE framing and, for Rust, /health, /health/ready, /metrics) and that the backend tap is active; run's endpoints part records the same checks for both frontends; verdict --criterion 1 passes only from the sidecar alone, iff every Rust endpoint is ok"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_discover_rust_endpoints"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_discover_python_endpoints"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_discover_missing_endpoint_fails"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_run_endpoints_part_and_verdict_c1"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_port_in_use_exits_2"
        status: pass
    human_judgment: false
  - id: D4
    description: "run --models GATE,REPORT runs the sequential comparison for every model in its own fresh sessions; a report model whose gated checkpoint cannot be loaded is recorded as status unavailable with the log line as reason and does not fail the run; verdict --criterion 2 passes only if the gate model has >=100 prompts with matched==n and every other model has status ok"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_report_model_unavailable"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_verdict_c2_c3_synthetic"
        status: pass
    human_judgment: false
  - id: D5
    description: "validate --require-gpu rejects a document not produced on Linux with a named GPU, in run mode, from the canonical corpus with an unchanged sha256 and n>=100, with endpoints, a sequential block for every meta model and a concurrent block for the gate model; a non-GPU run can never write docs/benchmarks/parity-report.json"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_canonical_out_refused_off_gpu"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_check.py#test_validate_require_gpu_rejects_mac_doc"
        status: pass
    human_judgment: false

duration: 75min
completed: 2026-10-07
status: complete
---

# Phase 6 Plan 4: Concurrent Load, Endpoint Coverage, Multi-Model Gating and Verdicts 1-3 Summary

**Extended the parity measurement path with a fixed-concurrency informational match rate (PAR-02), a real per-endpoint HTTP smoke test for both frontends (ROADMAP criterion 1), honest multi-model sequential runs with gated-checkpoint handling (D-02/D-03/D-04), a GPU-only validation gate, and sidecar-only PASS/FAIL verdicts for criteria 1 through 3.**

## Performance

- **Duration:** 75 min
- **Tasks:** 3 completed
- **Files modified:** 5 (all pre-existing from 06-01/06-03)

## Accomplishments

- `sweep.send_concurrent`/`join_by_input_ids`: fires the whole corpus at once (bounded by `--concurrency`, default 128, via `TCPConnector` + `asyncio.Semaphore`), then joins each frontend's own tap user records to corpus prompts by matching `input_ids` against that same frontend's sequential sides -- no ordering assumption, with unmatched tap records counted rather than dropped
- `sweep.endpoint_smoke`/`PYTHON_ENDPOINTS`/`RUST_ENDPOINTS`/`port_free`: a real HTTP sweep proving a frontend answers `GET /v1/models`, `GET /v1`, non-streaming and streaming `POST /v1/chat/completions`, `POST /generate`, and (Rust only) `GET /health`, `GET /health/ready`, `GET /metrics`; `port_free` gates every session launch in both `run` and `discover` (T-06-09)
- `scripts/parity_check.py discover` and `verdict FILE --criterion N` subcommands; `run`'s default `--parts` is now `endpoints,sequential,concurrent`; `run` handles multiple `--models` honestly (a gated checkpoint's session failure is classified "unavailable" from its log tail, not "failed") and refuses to write the canonical `docs/benchmarks/parity-report.json` from a non-GPU run (T-06-10)
- `sidecar.validate_discover` and the `concurrent`/`endpoints` block schemas; `validate_sidecar(require_gpu=True)` now additionally enforces `meta.mode == "run"`, the canonical corpus's exact path/sha256/`n>=100`, both endpoint lists present, a `sequential` block per `meta.models` entry, and a `concurrent` block for `meta.gate_model`
- `fake_parity_server.py` gains `--flavor python|rust` (real SSE-framed streaming chat and `/generate`, plus the three Rust-only endpoints), `--diverge-under-load N`, `--gated`/`--gated-models`, and a fixed 256-entry listen backlog (`request_queue_size`) that fixed a real concurrency bug found while proving Task 1 end to end

## Task Commits

1. **Task 1: Tracer -- the whole corpus in flight at once through fake Python and Rust sessions gives a PAR-02 match count in the sidecar** - `aabbba1` (feat)
2. **Task 2: Endpoint check for criterion 1: the discover subcommand, the run endpoints part, the shim-backed tap check and verdict 1** - `d55bd49` (feat)
3. **Task 3: Multi-model runs with honest gated-checkpoint handling, the canonical-output guard, validate --require-gpu, and verdicts 2 and 3** - `6454fb2` (feat)

**Plan metadata:** recorded separately in the `docs(06-04)` commit that adds this SUMMARY, STATE.md, ROADMAP.md and REQUIREMENTS.md.

## Files Created/Modified

- `python/rsglang/parity/sweep.py` - `send_concurrent`, `join_by_input_ids`, `port_free` (Task 1); `PYTHON_ENDPOINTS`, `RUST_ENDPOINTS`, `endpoint_smoke` (Task 2); `_build_payload` factored out of `send_sequential` for reuse by `send_concurrent`
- `python/rsglang/parity/sidecar.py` - concurrent-block schema validation (Task 1); `validate_discover`, `write_discover`, endpoints-block validation (Task 2); extended `require_gpu` checks and the `build_meta` `"gpu"` fix (Task 3)
- `scripts/parity_check.py` - `--concurrency` flag, `concurrent` run part (Task 1); `discover` subcommand, `endpoints` run part, `verdict --criterion 1` (Task 2); multi-model loop with gated classification, the canonical-output guard, `verdict --criterion 2/3` (Task 3)
- `python/rsglang/testing/fake_parity_server.py` - in-flight counter, `--diverge-under-load`, per-token delay, fixed listen backlog (Task 1); `--flavor`, streaming/`/generate` framing, `/health`/`/health/ready`/`/metrics` (Task 2); `--gated`/`--gated-models` (Task 3)
- `python/tests/test_parity_check.py` - 12 new tests across the three tasks (concurrent tracer + bounds, discover/endpoints/port-in-use, multi-model gating/canonical-guard/require-gpu/verdict-synthetic)

## Decisions Made

- **[Rule 1 - Bug] `ThreadingHTTPServer`'s default `request_queue_size` (5) silently dropped connections under real concurrency.** Found while debugging a flaky `test_concurrent_tracer` (passed most runs, occasionally reported fewer matches than the corpus size). Traced to the fake server's listen backlog: at `--concurrency 8`, 2 of 8 simultaneous connections were refused/dropped by the OS before `do_POST` ever ran, so their tap `user` records were never written -- `join_by_input_ids` correctly reported them as "no tap user record" (working as designed), but the underlying cause was an environment limitation, not a frontend divergence. Fixed by subclassing `ThreadingHTTPServer` with `request_queue_size = 256` and `daemon_threads = True`. Verified with 25 repeated runs showing zero failures after the fix (25/25 clean vs. intermittent failures before). Committed in `aabbba1`.
- **[Rule 1 - Bug] `sidecar.build_meta` never recorded `meta.gpu`.** `validate_sidecar(require_gpu=True)`'s platform/gpu check reads `meta.get("gpu")`, but `build_meta` (from 06-01) never set that key -- so the require-gpu gate could never pass even on a real GPU box, silently defeating its own purpose. Fixed by adding `"gpu": _gpu_name()` to `build_meta`'s return. Found and fixed while implementing Task 3's require_gpu extension (the Mac tests still correctly fail on `platform`, since `sys.platform` is `darwin`, so this gap wasn't visible from the existing tests alone). Committed in `6454fb2`.
- Report-model (non-gate) sequential failures only add a warning and do not flip `any_failure`, matching Task 3's exit-code contract precisely: only the gate model's own status/match-rate, or any genuinely `"failed"` (non-gated) session for any model, decide `run`'s exit code. `verdict --criterion 2` is the sole judge of whether a report model's unavailability matters.
- Session-failure classification (`_classify_session_failure`) reads the regex match directly from the `ServerExited` exception's own message, which already embeds the session log's last 40 lines (from `procs.wait_ready`), rather than re-opening the log file from a separately-tracked path -- simpler and sufficient since the embedded tail already contains the gated-repo stderr line in every test scenario.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `ThreadingHTTPServer` default listen backlog dropped concurrent connections**
- **Found during:** Task 1, debugging a flaky `test_concurrent_tracer`
- **Issue:** `socketserver.BaseServer.request_queue_size` defaults to 5; at `--concurrency 8` some connections were refused/dropped under real load, producing spurious tap-join mismatches unrelated to any frontend divergence
- **Fix:** Subclassed `ThreadingHTTPServer` with `request_queue_size = 256`, `daemon_threads = True`
- **Files modified:** `python/rsglang/testing/fake_parity_server.py`
- **Verification:** 25 repeated runs of `test_concurrent_tracer` with zero failures after the fix (previously intermittent, ~1-in-8)
- **Commit:** `aabbba1`

**2. [Rule 1 - Bug] `sidecar.build_meta` never set `meta.gpu`**
- **Found during:** Task 3, implementing `validate_sidecar(require_gpu=True)`'s extended checks
- **Issue:** The existing `require_gpu` platform/gpu check reads `meta.get("gpu")`, but `build_meta` (06-01) never populated that key -- the gate could never pass on a real GPU run
- **Fix:** Added `"gpu": _gpu_name()` to `build_meta`'s returned dict
- **Files modified:** `python/rsglang/parity/sidecar.py`
- **Verification:** `test_validate_require_gpu_rejects_mac_doc` still correctly fails on `platform`; the `gpu` key is now present and would be checked on a real Linux/GPU-produced document
- **Commit:** `6454fb2`

---

**Total deviations:** 2 auto-fixed (both Rule 1 bugs, both found while proving the plan's own tracer/acceptance tests end to end, neither changing scope or design).
**Impact:** Both fixes are required for the measurement surface to be correct on the real GPU box this plan's output is destined for; without them, concurrent-load numbers would be noisy from environment artifacts and `require_gpu` validation would be permanently unsatisfiable.

## Issues Encountered

None beyond the two auto-fixed deviations above, both resolved inline during their respective tasks.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

The sidecar's `endpoints`/`sequential`/`concurrent` schema, `parity_check.py`'s `run`/`discover`/`verdict`/`validate` CLI surface, and `fake_parity_server.py`'s `--flavor`/`--diverge-under-load`/`--gated` flags are locked per this plan's `key_links`/interfaces contract for 06-05 (abort stress, which adds the `abort_stress` block and the stress `--parts` entry without renaming anything here) and 06-06 (the GPU launch script, which will run `parity_check.py run` for real against the vendored backend). `annotate_sequence` (06-03) is still not wired into `run`'s output records -- that remains explicitly 06-05's job per 06-03's own `key_links` contract, unaffected by this plan. `vendor/` remains untouched; `scripts/check_upstream.py --offline` still passes.

Ready for 06-05 (abort stress, depends on this plan's run/sidecar/fake-server contract).

## Self-Check: PASSED

- All 5 modified files found on disk: `python/rsglang/parity/sweep.py`, `python/rsglang/parity/sidecar.py`, `scripts/parity_check.py`, `python/rsglang/testing/fake_parity_server.py`, `python/tests/test_parity_check.py`, plus this SUMMARY.
- All 3 task commits (`aabbba1`, `d55bd49`, `6454fb2`) found in `git log`.
- Re-ran plan-level `<verification>`: `.venv/bin/python -m pytest python/tests/test_parity_check.py python/tests/test_parity_tap.py -q` -> 21 passed; `scripts/parity_check.py verdict --help` and `discover --help` -> both exit 0.
- Re-ran every task's `<acceptance_criteria>` command: Task 1 (`pytest -k concurren` -> 3 passed, `grep` for `send_concurrent`/`join_by_input_ids` -> 2 lines, `run --help` contains `--concurrency`), Task 2 (`pytest -k "endpoint or discover or port_in_use"` -> 5 passed, `discover --help` contains `--skip-tap-check`, `grep -c '"GET /health/ready"'` -> 2), and Task 3 (`pytest -q` on the full file -> 15 passed, the canonical-output guard prints `refusing to write` and exits 2 with no file created, `verdict --help` exits 0) all passed.
- Full regression: `.venv/bin/python -m pytest python/tests/test_parity_check.py python/tests/test_parity_tap.py python/tests/test_parity_corpus.py python/tests/test_parity_compare.py -q` -> 39 passed (repeated 3x with no flakiness). `.venv/bin/python scripts/check_upstream.py --offline` -> `check_upstream: OK`.
