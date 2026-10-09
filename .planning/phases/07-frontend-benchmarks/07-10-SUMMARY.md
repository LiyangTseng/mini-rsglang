---
phase: 07-frontend-benchmarks
plan: 10
subsystem: benchmarking
tags: [rust, bash, launcher, gpu-wrapper, D-03, BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08]

requires:
  - phase: 07-09
    provides: "rsg_bench::report::{Report, build_report, render_markdown, run_report} and rsg_bench::scenarios::standard_throughput, the full subcommand surface (s1, s2, sweep-num-tokenizer, crosscheck, s3, throughput, report) gpu_phase7_bench.sh and bench_mac_devpass.sh both drive"
  - phase: 07-01
    provides: "crates/rsg-bench crate layout and bin/clap conventions rsg-mock-stack follows"
  - phase: 05
    provides: "rsg-server's real HTTP API (/v1/chat/completions, /health, /health/ready, /metrics) and HfCodec tokenizer loading -- Task 3's own precondition, landed in this checkout via the reconciliation commit (c23df8f) between Tasks 1-2 and Task 3"
provides:
  - "rsg-mock-stack: a Mac launcher adapter binary (crates/rsg-bench/src/bin/rsg-mock-stack.rs) that brings up mock-scheduler + rsg-server, relays the handshake, and tears both down on a signal or either child's exit (D-03, T-07-24)"
  - "scripts/gpu_phase7_bench.sh: the one GPU-box command that sequences preflight, sweep-num-tokenizer, s1, s2, the optional vllm/sglang cross-checks, s3, throughput, report and check_upstream.py, with a tested --dry-run"
  - "scripts/bench_mac_devpass.sh: the D-03 Mac verification pass -- both A/B arms launch the identical rsg-mock-stack command (R-vs-R mechanics), proving s1/s2(open+closed)/s3, manifest/report writing, histogram decoding, the mock provenance banner, and that a client-aborted streaming request increments rsg-server's /metrics cancellation counter, all against the project's real Rust frontend"
affects: ["The end-of-phase GPU-box human check (Task 2's own <human-check>) that depends on scripts/gpu_phase7_bench.sh", "Phase 08+ (if any): BENCH-02..BENCH-08 are now satisfied by harness mechanics; the GPU-measured numbers themselves are still outstanding"]

actuals:
  tokens: 18484
  tasks: 3
  commits: 6
plan_head_before: c23df8ff4819c1e913413e6379d6b0bd120b9f90
plan_head_after: 4b0117b8dc8b4f7166bef7585b26cd9be673553d

tech-stack:
  added: []
  patterns:
    - "rsg-mock-stack uses tokio::process::Command (not std::process + a manual wait thread) for both children, so the SIGINT/SIGTERM-or-child-exit race is one tokio::select! rather than hand-rolled polling -- the only bin in rsg-bench so far to need async subprocess supervision of two concurrent children"
    - "gpu_phase7_bench.sh's --dry-run branch is hand-written (not synthesized from the real-run argv arrays via printf %q), so every printed RUN: line's wording is fully under the script author's control and trivially substring-matchable by pytest, at the cost of keeping two copies of each command's flag list in sync"
    - "bench_mac_devpass.sh's D-03 Mac dev pass sets BOTH A/B arms' launch template to the identical rsg-mock-stack command (R-vs-R mechanics, RESEARCH Pitfall 3): this makes the report's NOT A FRONTEND COMPARISON banner structurally guaranteed (backend_kind: mock, no GPU recorded) rather than something a human must remember to interpret correctly"
    - "A subprocess that expects its own stdin piped-and-held-open for its whole lifetime (rsg-server, mock-scheduler) needs that same contract honored by anything that spawns it as a child, including a test harness's own direct Command::new() calls -- Stdio::null() or an unused free_port() silently breaks that contract without an immediate compile error, only a runtime one"

key-files:
  created:
    - crates/rsg-bench/src/bin/rsg-mock-stack.rs
    - crates/rsg-bench/tests/mock_stack.rs
    - scripts/gpu_phase7_bench.sh
    - python/tests/test_gpu_bench_script.py
    - scripts/bench_mac_devpass.sh
    - .planning/phases/07-frontend-benchmarks/deferred-items.md
  modified:
    - crates/rsg-bench/Cargo.toml
    - crates/rsg-tokenizer/src/loader.rs
    - python/tests/test_baseline_profile.py
    - python/tests/test_gpu_profile_script.py
    - .planning/WINDOWS.md

key-decisions:
  - "rsg-mock-stack's two children are spawned with no process_group override, so they inherit rsg-mock-stack's own process group -- when the harness wraps rsg-mock-stack itself in a fresh group via rsg_bench::procs::launch, that same group already covers both children and no special-casing is needed in procs.rs (T-07-24)"
  - "gpu_phase7_bench.sh's cross-check env-var gate (VLLM_BIN / SGLANG_PYTHON) reuses clap's own --backend-kind default (Real) for the sweep step rather than passing --backend-kind real explicitly on that one line, since the <behavior> contract's test_dry_run_sequence requires the sweep RUN: line to NOT contain --backend-kind real -- the sweep's manifest still ends up backend_kind: real because that is SessionArgs' own clap default"
  - "The two sweep-only hardcoded values (--runs 1, no --python-best-num-tokenizer) are intentional per the plan's <action> spec: the sweep computes BEST itself and runs once per --num-tokenizer candidate, independent of the wrapper's own --runs value, which only applies to every scenario that already knows BEST"
  - "Task 3's precondition (Phase 5's HTTP API present in this checkout) was re-verified independently, not trusted from the orchestrator's framing: grep -rn \"v1/chat/completions\" crates/rsg-server/src matched, cargo build -p rsg-server --bins succeeded, --help listed --host/--port and an --abort-timing option, and a /metrics route exists in crates/rsg-server/src/http/mod.rs. Met -> proceeded with no checkpoint."
  - "rsg-mock-stack's --rsg-server-arg values that themselves start with a hyphen (e.g. --port={port}) must be passed as --rsg-server-arg=--port={port} (the = form), not two separate argv tokens -- clap derive rejects a hyphen-prefixed value for a repeatable --long option otherwise, confirmed empirically before writing either bench_mac_devpass.sh or the mock_stack.rs fix"
  - "bench_mac_devpass.sh uses PORT=19191 for the shared s1/s2/s3 session port and a dynamically freed port (via a short-lived python socket bind-close) only for the standalone cancellation-test rsg-mock-stack instance, since that instance runs concurrently with nothing else needing the session port"
  - "The rsg-tokenizer EnvGuard race (loader.rs test module) was fixed with a static Mutex<()> held for each EnvGuard's full lifetime, not by adding --test-threads=1 to scripts/check_all.sh's shared cargo test --workspace step -- the narrower, file-local fix corrects the actual race without slowing every future phase's test gate"

patterns-established:
  - "A GPU-box wrapper script's preflight gate is a sequence of independent checks that each immediately record FAIL step preflight and exit 1 the moment one fails (mirroring scripts/gpu_phase2_profile.sh), never accumulating multiple preflight failures into one report -- a GPU box with a broken toolchain should fail fast, not run every check to completion first"
  - "A Mac dev-pass script proving harness mechanics before a GPU run writes every output under target/<tool>/devpass/<run-id>, never into the directory the real measured report owns (docs/benchmarks/), and asserts that boundary itself (git status --porcelain docs/benchmarks empty) rather than merely trusting the script's own file paths to be correct"
  - "A test-only EnvGuard (or any RAII helper) that mutates process-global state (env vars, global registries) needs a held-for-the-guard's-lifetime lock across the whole binary's test threads, not just around the mutation itself -- Rust's default test runner executes every #[test] fn in a binary concurrently, and two guards covering overlapping global state can otherwise interleave their save/mutate/restore cycles"

requirements-completed: [BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08]

coverage:
  - id: D1
    description: "rsg-mock-stack brings up mock-scheduler (bind backend, connect detok) and rsg-server (connect backend, bind detok) on matched ipc addresses, relays the stdout handshake line verbatim to rsg-server's stdin, forwards both children's stderr with prefixes, and stops both (SIGTERM, 5s grace, SIGKILL) on a signal or either child's exit, removing the ipc socket files"
    requirement: BENCH-02
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/mock_stack.rs#mock_stack_relays_handshake_and_stops"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/mock_stack.rs#mock_stack_exits_when_child_dies"
        status: pass
    human_judgment: false
  - id: D2
    description: "scripts/gpu_phase7_bench.sh sequences preflight, the --num-tokenizer sweep, s1, s2 (open loop), the optional vllm/sglang cross-checks, s3, throughput, the combined report, and check_upstream.py behind one command, with --dry-run printing the exact command sequence and a tested preflight-failure path"
    requirement: BENCH-02
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_bench_script.py#test_help"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_bench_script.py#test_unknown_arg"
        status: pass
      - kind: integration
        ref: "python/tests/test_gpu_bench_script.py#test_dry_run_sequence"
        status: pass
      - kind: integration
        ref: "python/tests/test_gpu_bench_script.py#test_dry_run_crosschecks_optional"
        status: pass
      - kind: integration
        ref: "python/tests/test_gpu_bench_script.py#test_extra_args_placement"
        status: pass
      - kind: integration
        ref: "python/tests/test_gpu_bench_script.py#test_preflight_fails_without_gpu"
        status: pass
    human_judgment: false
  - id: D3
    description: "D-03 Mac dev pass: scripts/bench_mac_devpass.sh drives rsg-server (backed by mock-scheduler) through s1/s2 (both modes)/s3, asserts completed/cancelled counts, the report's mock banner, every histograms entry decoding, and that a curl-aborted streaming request increments rsg-server's /metrics cancellation counter; followed by the phase gate (scripts/check_all.sh --offline)"
    requirement: BENCH-03
    verification:
      - kind: integration
        ref: "scripts/bench_mac_devpass.sh (full run: s1 alternation/completed/cancelled, both s2 curves, s3 means, report decode + banner, /metrics cancellation counter +3)"
        status: pass
      - kind: other
        ref: "scripts/check_all.sh --offline (full 7-step phase gate)"
        status: pass
      - kind: other
        ref: "cargo clippy --workspace --all-targets -- -D warnings"
        status: pass
    human_judgment: false
  - id: D4
    description: "The real GPU-box headline Python-vs-Rust comparison (BENCH-03..BENCH-08's actual measured numbers)"
    requirement: BENCH-06
    verification: []
    human_judgment: true
    rationale: "This is this plan's own end-of-phase human check (Task 2's <human-check>), explicitly deferred to the Linux CUDA GPU box after Phase 6 lands; it was never expected to run on this Mac dev machine and did not run here. BENCH-02..BENCH-08 are satisfied by proven harness mechanics (D1-D3); the measured comparison itself is still outstanding and gated on hardware."

duration: 75min
completed: 2026-10-07
status: complete
---

# Phase 07 Plan 10: GPU-Box Wrapper, Mac Launcher Adapter, and D-03 Mac Dev Pass Summary

**Built and tested `rsg-mock-stack` (the Mac launcher adapter for the real Rust frontend against `mock-scheduler`), `scripts/gpu_phase7_bench.sh` (the one-command GPU-box Python-vs-Rust wrapper), and `scripts/bench_mac_devpass.sh` (the D-03 Mac verification pass proving the full harness -- s1/s2/s3, manifests, report, and client-cancellation-reaches-the-server -- against the project's real Rust frontend); the full phase gate (`check_all.sh --offline`) and workspace clippy are green, completing Phase 07.**

## Performance
- **Duration:** 75min total (Tasks 1-2: ~25min in an earlier session; Task 3 (this continuation, including re-verifying the precondition and four unplanned fixes): ~50min)
- **Started:** 2026-10-07T06:42:40Z (approx, Tasks 1-2)
- **Completed:** 2026-10-07T19:54:00Z (Task 3, this session)
- **Tasks:** 3 of 3
- **Files modified:** 13 total across the plan (8 created, 5 modified); Task 3 alone touched 8 (2 newly created: `bench_mac_devpass.sh`, plus the already-existing `deferred-items.md`; 2 further-modified from Tasks 1-2: `rsg-mock-stack.rs`, `mock_stack.rs`; 3 newly modified: `loader.rs`, `test_baseline_profile.py`, `test_gpu_profile_script.py`; 1 planning doc: `WINDOWS.md`)

## Accomplishments
- `crates/rsg-bench/src/bin/rsg-mock-stack.rs`: spawns `mock-scheduler` (`--backend-role bind --detok-role connect`, passthrough delay/max-seq-len flags) and `rsg-server` (`--backend-role connect --detok-role bind --model --run-id`, plus each `--rsg-server-arg` with `{port}` substituted), reads `mock-scheduler`'s stdout handshake line within 20s, writes it verbatim to `rsg-server`'s stdin, forwards both children's stderr with `[rsg-server]`/`[mock-scheduler]` prefixes, prints the `children ...` and `backend ready; handshake sent to rsg-server` lines, and stops both children (SIGTERM, 5s grace, SIGKILL) on SIGINT/SIGTERM or either child's own exit, removing the two `ipc://` socket files. Task 3 fixed two bugs in it/its test, both only exercisable once Phase 5's real binaries existed in this checkout (see Deviations).
- `scripts/gpu_phase7_bench.sh`: preflight (`nvidia-smi -L`, `hyperfine >= 1.19`, the `minisgl`/`openai`/`transformers`/`rsglang` python imports, a release build) then `sweep-num-tokenizer` -> `s1` -> `s2 --mode open` -> the optional `vllm`/`sglang` cross-checks -> `s3` -> `throughput` -> `report` -> `scripts/check_upstream.py`, each step recording a PASS/FAIL line. `--dry-run` prints the exact `RUN: ` command sequence without building or running anything.
- `scripts/bench_mac_devpass.sh` (new, Task 3): builds `rsg-server`+`rsg-bench`, then drives both A/B arms through the identical `rsg-mock-stack` launch command (R-vs-R mechanics, RESEARCH Pitfall 3) via `s1` (16 agents, 50% cancel fraction, 2 runs), `s2 --mode open` (rates 20,40) and `s2 --mode closed` (concurrency 1,8, written to a separate `closed/` directory so `report` never sees two manifests for one scenario), and `s3` (hyperfine, 2 runs + 1 warmup). Asserts via `"$PYTHON" -c` JSON checks: s1's trial-arm alternation and non-zero completed/cancelled counts, both s2 manifests' ascending 2-point curves, s3's positive `means.e2e_ready_s` and non-null `means.frontend_tail_s`. Runs `rsg-bench report` over both manifest directories (doubling as the "every histograms entry decodes" check) and asserts the main report's first summary line carries both "NOT A FRONTEND COMPARISON" and "mock". Launches a standalone `rsg-mock-stack` instance on a free port and proves LIFE-02 from the harness side: 3 `curl -sN --max-time 0.2` client-aborted streaming `/v1/chat/completions` requests grow `/metrics`' `rsg_requests_cancelled_total` by >= 3 (measured: 0 -> 3). Asserts `docs/benchmarks` was never touched. Printed `devpass: OK` on every run (3 full runs during this session, all green).
- Re-verified Task 3's `<precondition>` independently rather than trusting the orchestrator's framing: `grep -rn "v1/chat/completions" crates/rsg-server/src` matched, `cargo build -p rsg-server --bins` succeeded, `--help` listed `--host`/`--port`/`--abort-timing`, and a `/metrics` route exists in `crates/rsg-server/src/http/mod.rs`. Precondition met -> proceeded with no checkpoint.
- Full phase gate green: `cargo test --workspace` (confirmed clean on 2 repeated full runs after the EnvGuard fix below), `.venv/bin/python -m pytest python/tests -q` (197 passed, 37 skipped, 0 failed), fixture-freshness x3, WIRE-02 decode, `check_upstream.py --offline` -> `check_all: OK`. `cargo clippy --workspace --all-targets -- -D warnings` clean.

## Task Commits
1. **Task 1: Tracer -- rsg-mock-stack brings up mock-scheduler and rsg-server with the handshake relayed, then stops both cleanly** - `44137d6` (feat) *(original hash; superseded by the `c23df8f` reconciliation commit that carried this content forward -- see STATE.md's reconciliation decision log)*
2. **Task 2: GPU-box wrapper scripts/gpu_phase7_bench.sh with dry-run tests** - `ee85b47` (feat) *(same note as above)*
3. **Task 3: D-03 Mac dev pass against rsg-server + mock-scheduler, and the phase gate** - six commits, this session:
   - `c702dc3` (fix) -- keep mock-scheduler's stdin piped+open; use a loadable test model
   - `9a5fb7d` (fix) -- stop the hyperfine-missing pytest simulations from finding a real one; venv bootstrap
   - `e48fefa` (feat) -- the D-03 dev pass script itself (`scripts/bench_mac_devpass.sh`)
   - `d56e8fb` (fix) -- thread `mock_stack.rs`'s `free_port()` into rsg-server's listen port
   - `a43fde0` (fix) -- serialize `EnvGuard` across rsg-tokenizer's parallel test threads
   - `4b0117b` (docs) -- resolve WINDOWS.md/deferred-items.md entries for the two fixes above

**Plan metadata:** pending (the commit immediately following this one, per the final_commit protocol)

## Files Created/Modified
- `crates/rsg-bench/src/bin/rsg-mock-stack.rs` - Mac launcher adapter: mock-scheduler + rsg-server, handshake relay, signal/child-exit teardown; Task 3 fixed mock-scheduler's stdin handling
- `crates/rsg-bench/tests/mock_stack.rs` - the two Task 1 integration tests; Task 3 fixed the test model id and threaded `free_port()` into rsg-server's actual listen port
- `crates/rsg-bench/Cargo.toml` - added tokio's `process` feature (needed by `tokio::process`)
- `scripts/gpu_phase7_bench.sh` - the one GPU-box command: preflight, sweep, s1/s2/crosscheck/s3/throughput, report, check_upstream.py
- `python/tests/test_gpu_bench_script.py` - the six Task 2 behavior tests
- `scripts/bench_mac_devpass.sh` - new: the D-03 Mac dev pass (Task 3)
- `crates/rsg-tokenizer/src/loader.rs` - new (Task 3): a static `Mutex<()>` serializing `EnvGuard` across parallel test threads, fixing a pre-existing race
- `python/tests/test_baseline_profile.py` - new (Task 3): excludes a real `hyperfine` elsewhere on PATH from the "hyperfine missing" simulation
- `python/tests/test_gpu_profile_script.py` - new (Task 3): same PATH fix for `test_hyperfine_ok`'s third case
- `.planning/phases/07-frontend-benchmarks/deferred-items.md` - Tasks 1-2's two pre-existing Phase-2 pytest failures, both now resolved by Task 3; plus a new (now also resolved) rsg-tokenizer EnvGuard race entry
- `.planning/WINDOWS.md` - entries 4 and 5 marked `fixed`

## Decisions Made
See `key-decisions` in frontmatter: no `process_group` override on rsg-mock-stack's children (T-07-24); the sweep step relying on clap's own `--backend-kind` default; the sweep's hardcoded `--runs 1`; Task 3's independent precondition re-verification; the `--rsg-server-arg=--value` (`=` form) requirement for hyphen-prefixed values; the dev pass's port strategy; and the file-local `Mutex` fix for the EnvGuard race over a workspace-wide `--test-threads=1`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `FAILED` flag never set on a preflight failure, so the summary printed "ALL PASS" / exit 0 after a `FAIL step preflight` line**
- **Found during:** Task 2, manual verification of the preflight-failure path (`PATH` with no `nvidia-smi`) before writing the pytest suite
- **Issue:** Each of the four preflight failure branches called `record ... FAIL ...` then jumped straight to `print_summary_and_exit`, but never set `FAILED=1` first, so the summary's `[ "$FAILED" = 0 ]` check still saw `0` and printed `ALL PASS` with exit 0 -- a wrapper that reports success despite a failed GPU preflight.
- **Fix:** Added `FAILED=1` immediately before each of the four `print_summary_and_exit` calls in the preflight section.
- **Files modified:** `scripts/gpu_phase7_bench.sh`
- **Verification:** Manually re-ran with a `PATH` lacking `nvidia-smi`; confirmed `SOME STEPS FAILED` / exit 1. Then covered by `test_preflight_fails_without_gpu`.
- **Commit:** `ee85b47`

**2. [Rule 1 - Bug] rsg-mock-stack spawned mock-scheduler with `Stdio::null()` stdin, which mock-scheduler treats as "parent went away" and exits before its handshake line**
- **Found during:** Task 3, Step 0 of resumption -- running `cargo test -p rsg-bench --test mock_stack` against the real (Phase 5-equipped) `mock-scheduler` binary for the first time ever in this checkout
- **Issue:** `mock-scheduler` expects its stdin piped and held open for its whole lifetime (treats EOF as the launcher going away), but rsg-mock-stack used `Stdio::null()` for it -- an immediately-EOF'd stdin -- so `mock-scheduler` exited (`EXIT_STDIN_EOF`) before ever printing its handshake line. Both `mock_stack.rs` tests failed with "mock-scheduler closed stdout before sending its handshake line" / "parent went away (stdin EOF)".
- **Fix:** Changed to `Stdio::piped()`, held the handle open until teardown (mirroring the existing `rsg_stdin` pattern already in the same file), and added `drop(mock_stdin)` at every teardown/early-return path.
- **Files modified:** `crates/rsg-bench/src/bin/rsg-mock-stack.rs`
- **Verification:** `cargo test -p rsg-bench --test mock_stack` passes; `cargo clippy -p rsg-bench --all-targets -- -D warnings` clean.
- **Commit:** `c702dc3`

**3. [Rule 1/3 - Bug/Blocking] `mock_stack.rs`'s synthetic `--model` value panics against Phase 5's real tokenizer loader**
- **Found during:** Task 3, immediately after fix #2 above, same first-real-run discovery
- **Issue:** `mock_stack.rs` passed `--model mock-stack-test-{n}` to rsg-mock-stack (and so to rsg-server). Phase 1/3's rsg-server skeleton never validated `--model`; Phase 5's real `HfCodec::load` does, and panicked with `repo_id "mock-stack-test-0" has no '/'` before any handshake work.
- **Fix:** Reused the already-cached `Qwen/Qwen3-0.6B` model Phase 4/5's own tests use (confirmed cached locally, no network dependency), removing the now-pointless `COUNTER`/`n` machinery.
- **Files modified:** `crates/rsg-bench/tests/mock_stack.rs`
- **Verification:** Both `mock_stack` tests pass.
- **Commit:** `c702dc3`

**4. [Rule 1 - Bug] Two Phase-2 pytest "hyperfine missing" simulations find a real hyperfine elsewhere on PATH**
- **Found during:** Task 3, running the full pytest suite ahead of the phase gate (pre-existing, logged by the earlier Tasks 1-2 session to `deferred-items.md`/`WINDOWS.md` entry 4 but not fixed there)
- **Issue:** `test_run_s3_hyperfine_missing_exits_2` and `test_hyperfine_ok`'s third case simulate "hyperfine is missing" by removing/never writing it into a `tmp_path/bin` stub dir, but both then composed `PATH` as `{bin_dir}:{inherited PATH}`, so this Mac's real `hyperfine` (installed at `~/.cargo/bin/hyperfine` per CLAUDE.md's own recommendation, during this phase's own earlier Task 2/07-08 work) was still found, and the "missing" branch never fired. This blocks Task 3's own acceptance criteria (`check_all.sh --offline` must print `check_all: OK`), making it in scope under the scope-boundary exception for issues that block the current task.
- **Fix:** Added a `_path_without_real_hyperfine` helper to each test file that excludes, from `PATH`, any directory other than the test's own stub `bin_dir` that itself contains a real `hyperfine` binary -- a targeted PATH filter, not a change to either preflight helper's own logic.
- **Files modified:** `python/tests/test_baseline_profile.py`, `python/tests/test_gpu_profile_script.py`
- **Verification:** Both tests pass individually; full `pytest python/tests -q` now reports 197 passed, 37 skipped, 0 failed (previously 4 failed, including two unrelated venv-dependency failures fixed alongside, below).
- **Commit:** `9a5fb7d`

**5. [Rule 3 - Blocking] This worktree's `.venv` was missing `fastapi`/`uvicorn`, needed by two Phase 5 tests**
- **Found during:** Task 3, same full-pytest-suite run as #4
- **Issue:** `test_gen_api_fixtures.py::test_committed_fixtures_are_fresh` and `test_python_frontend.py::test_tracer_python_frontend_serves_generate_against_mock` both spawn the real upstream Python frontend process, which now needs `fastapi`/`uvicorn` (Phase 5's own already-hash-pinned additions to `requirements-mac.txt`). This worktree's `.venv` predated Phase 5 landing via the reconciliation and had never been re-synced.
- **Fix:** Ran `scripts/bootstrap_mac_env.sh` (idempotent, already documented in its own header as "safe to rerun in any checkout or worktree") -- a venv-sync against the already-committed, hash-pinned lockfile, not a new/unvetted package install (excluded from the package-manager-install deviation exclusion: no new dependency choice was made here).
- **Files modified:** none (venv-only; no tracked files changed)
- **Verification:** Both previously-failing tests pass; full suite green.
- **Commit:** n/a (no tracked-file change; documented in `deferred-items.md`, commit `9a5fb7d`)

**6. [Rule 1 - Bug] `mock_stack.rs`'s `free_port()` was computed but never threaded into rsg-server's actual listen port**
- **Found during:** Task 3, running `cargo test --workspace` as step 1 of `scripts/check_all.sh --offline` (not reproducible via `cargo test -p rsg-bench --test mock_stack` alone, since that invocation happened not to collide)
- **Issue:** `Stack::spawn()` computed `free_port()` and passed it as rsg-mock-stack's own `--port` (used only for `{port}` substitution inside `--rsg-server-arg` values), but never actually supplied an `--rsg-server-arg` using it. Every spawned rsg-server therefore defaulted to port 1919, and `cargo test`'s default per-binary parallelism runs the file's two tests concurrently -- an intermittent "failed to bind 127.0.0.1:1919: Address already in use" that had nothing to protect against before Phase 5's rsg-server ever opened an HTTP listener.
- **Fix:** Passed `--rsg-server-arg=--port={port}` (confirmed the `=` form is required: clap derive rejects a hyphen-prefixed value for a repeatable `--long` option as a separate token).
- **Files modified:** `crates/rsg-bench/tests/mock_stack.rs`
- **Verification:** 5 repeated `cargo test -p rsg-bench --test mock_stack` runs, all green; `cargo clippy -p rsg-bench --all-targets -- -D warnings` clean.
- **Commit:** `d56e8fb`

**7. [Rule 1 - Bug] rsg-tokenizer's `EnvGuard`-using loader tests race under `cargo test --workspace`'s parallel test threads**
- **Found during:** Task 3, running `cargo test --workspace` (step 1 of `scripts/check_all.sh --offline`) after fixing #6; confirmed non-deterministic (1 fail / 2 pass across isolated reruns) then confirmed load-sensitive (2/2 failed under full-workspace concurrency, where a plain retry was not a viable path)
- **Issue:** `loader::tests::gated_access_unavailable_with_blank_token_file` intermittently got `Ok(_)` instead of the expected `GatedAccessUnavailable`, because `EnvGuard` mutates process-global `HF_TOKEN`/`HF_TOKEN_PATH`/`HF_HOME`/`HF_HUB_DISABLE_IMPLICIT_TOKEN` env vars with no cross-test lock, and two `EnvGuard`-using tests running concurrently (Rust's test runner default) can interleave their save/mutate/run/restore cycles. This is the identical hazard category Phase 4 already named in `crates/rsg-server/src/hf_codec.rs`'s own comment ("requires --test-threads=1 to be deterministic"), not introduced by 07-10, but blocking Task 3's own acceptance criterion (`check_all.sh --offline` must print `check_all: OK`), so in scope under the same exception as #4.
- **Fix:** Added a `static Mutex<()>` to `loader.rs`'s test module, held for each `EnvGuard`'s full lifetime, serializing the whole save/mutate/run/restore cycle -- narrower than forcing `--test-threads=1` on `scripts/check_all.sh`'s shared `cargo test --workspace` step, which would slow every future phase's test gate to fix a bug local to one test module.
- **Files modified:** `crates/rsg-tokenizer/src/loader.rs`
- **Verification:** 6 repeated `cargo test -p rsg-tokenizer --lib` runs, all green; 2 repeated full `cargo test --workspace` runs, both green; `cargo clippy -p rsg-tokenizer --all-targets -- -D warnings` clean.
- **Commit:** `a43fde0`

---

**Total deviations:** 7 auto-fixed (6 Rule 1 bugs, 1 Rule 3 blocking issue). **Impact:** All seven were either caught before any commit (preflight flag, Task 2) or were bugs/races only ever exercisable against the real Phase 5 binaries and full-workspace test load this session was the first to produce in this checkout -- none represent scope creep beyond what was needed to get Task 3's own acceptance criteria to pass honestly. No fix weakened a test's intent; each narrowed a bug to its true root cause (stdin contract, model-id validity, PATH composition, port threading, env-var serialization) rather than loosening an assertion.

## Issues Encountered

**Resolved (previously deferred by Tasks 1-2):** `scripts/check_all.sh --offline`'s pytest step previously stopped short of `check_all: OK` on two pre-existing, unrelated Phase-2 "hyperfine missing" test failures (logged to `deferred-items.md`/`WINDOWS.md` entry 4). Fixed this session (deviation #4 above); `WINDOWS.md` entry 4 is now `fixed`.

**New, also resolved this session:** `cargo test --workspace` (step 1 of the same gate) intermittently, and under full workspace load near-deterministically, failed one `rsg-tokenizer` test due to a pre-existing (Phase 4-era, already documented in `hf_codec.rs`'s own comment) `EnvGuard` parallel-test race. Fixed this session (deviation #7 above); logged and resolved as `WINDOWS.md` entry 5.

Both issues are now fully resolved; `scripts/check_all.sh --offline` prints `check_all: OK` reliably (confirmed on a clean run after both fixes, plus two repeated plain `cargo test --workspace` runs).

## User Setup Required

None. Task 3 needed no additional user setup beyond what Phase 5 itself requires in this checkout (its own `requirements-mac.txt` entries, already hash-pinned and installed via `scripts/bootstrap_mac_env.sh` as part of deviation #5 above).

## Next Phase Readiness

**Phase 07 is now fully complete on the Mac side.** All three tasks of this plan (07-10) are done, tested, and committed:
- Task 1: `rsg-mock-stack` (Mac launcher adapter, D-03 prerequisite)
- Task 2: `scripts/gpu_phase7_bench.sh` (one-command GPU-box wrapper, dry-run tested)
- Task 3: `scripts/bench_mac_devpass.sh` (D-03 Mac dev pass against the real Rust frontend + mock-scheduler) and the full phase gate

`BENCH-02` through `BENCH-08` are satisfied by proven harness mechanics (D1-D3 in `coverage` above): the load generator, both benchmark scenarios' client-side proof, the cold-start/memory measurement path, the A/B orchestrator with CIs and manifests, and the GC/memory-vs-latency reporting are all exercised end to end against the project's real Rust frontend on this Mac.

**The GPU-box headline Python-vs-Rust comparison did NOT run and was never expected to here.** Task 2's own verification explicitly defers the real measured run to a `<human-check>` on the Linux CUDA GPU box, after Phase 6 lands, as this phase's single manual-only verification (D4 above, `human_judgment: true`). `scripts/gpu_phase7_bench.sh` is ready and dry-run-tested for that eventual run. `docs/benchmarks/frontend-benchmarks.{json,md}` do not exist yet and were not written by any session of this plan (confirmed throughout: `git status --porcelain docs/benchmarks` empty).

**Plan 07-10 (and Phase 07 as a whole) is now `status: complete`.** 10/10 plans executed. `BENCH-02`..`BENCH-08` are marked complete in `REQUIREMENTS.md` by this plan's close-out (the shared-ID gate: 07-10 is the last plan declaring any of them).

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07 (all 3 tasks)*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/bin/rsg-mock-stack.rs
- FOUND: crates/rsg-bench/tests/mock_stack.rs
- FOUND: scripts/gpu_phase7_bench.sh
- FOUND: python/tests/test_gpu_bench_script.py
- FOUND: scripts/bench_mac_devpass.sh
- FOUND: crates/rsg-tokenizer/src/loader.rs
- FOUND: .planning/phases/07-frontend-benchmarks/deferred-items.md
- FOUND: c702dc3 (fix: mock-scheduler stdin + test model)
- FOUND: 9a5fb7d (fix: hyperfine PATH simulations + venv bootstrap)
- FOUND: e48fefa (feat: bench_mac_devpass.sh)
- FOUND: d56e8fb (fix: mock_stack.rs free_port threading)
- FOUND: a43fde0 (fix: EnvGuard mutex)
- FOUND: 4b0117b (docs: WINDOWS.md/deferred-items.md resolution)
- No unexpected file deletions in any Task 3 commit
