---
phase: 06-gpu-end-to-end-parity
plan: 06
subsystem: testing
tags: [gpu-script, bash, parity-harness, stress-test, checkpoint]

requires:
  - phase: 06-03
    provides: "fixtures/parity/corpus.json (128-item canonical corpus), compare.py's full layer precedence"
  - phase: 06-05
    provides: "the stress run part (abort_stress block, verdict criterion 4), fake_parity_server's server/stress subcommands"
provides:
  - "scripts/gpu_phase6_parity.sh: the human-run GPU wrapper chaining cargo build, discover (both frontends), the full parity_check.py run, validate --require-gpu and verdict --criterion 1..4, plus check_upstream.py -- proven end to end on the Mac against stub nvidia-smi/cargo and the fake_parity_server stand-ins"
  - "sweep.run_session's sessions.pgid safety net (T-06-14), so an interrupted wrapper run can kill -9 every session process group it started"
affects: [06-07, 06-08]

actuals:
  tokens: 5050
  tasks: 1
  commits: 1
  plan_head_before: 96e23214bef46ffa51a1c27a3b2d6e5117d067da
  plan_head_after: 295ab9d

tech-stack:
  added: []
  patterns:
    - "GPU wrapper step functions (run_discover/run_run/run_verdict) each split cross-referencing `local name=value name2=value2` declarations across separate `local` statements -- bash evaluates all RHS expressions in a single `local` statement against the OUTER scope before any of that statement's own new locals are assigned, so referencing an earlier-declared-in-the-same-statement local on its own RHS is unbound under `set -u` even though it reads correctly to the eye"

key-files:
  created:
    - scripts/gpu_phase6_parity.sh
    - python/tests/test_gpu_phase6_parity_script.py
    - .planning/phases/06-gpu-end-to-end-parity/deferred-items.md
  modified:
    - python/rsglang/parity/sweep.py

key-decisions:
  - "Task 1 (the GPU wrapper tracer) executed and committed in full; Task 2 (binding the wrapper's defaults to Phase 5's delivered stress-test interface) halted at its unmet precondition -- see Deviations/Checkpoint below. This SUMMARY documents a partial, intentionally-halted plan (status: halted), not a completed one"
  - "Found and fixed a real bash bug while building Task 1: `local a=\"$1\" b=\"$LOG_DIR/x-$a.log\"` on one line throws '<name>: unbound variable' under `set -u`, because bash evaluates every RHS in a `local` statement against the scope BEFORE that statement's own declarations take effect -- not left-to-right as the textual order suggests. Fixed by splitting each occurrence (run_discover, run_verdict) into two separate `local` statements"

requirements-completed: []

coverage:
  - id: D1
    description: "scripts/gpu_phase6_parity.sh runs, in order, a release build, discover for both frontends, the full parity_check.py run, validate --require-gpu, verdict --criterion 1..4 and check_upstream.py, printing one PASS/FAIL line per step and ALL PASS only when every step passed"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_tracer_mac_dry_run"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_help_anywhere"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_unknown_arg_exits_2"
        status: pass
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_preflight_missing_tool_fails"
        status: pass
    human_judgment: false
  - id: D2
    description: "On the Mac, with stub nvidia-smi/cargo and the fake parity server, the wrapper drives the whole pipeline end to end; every step passes except validate --require-gpu, which fails only because the run is not on Linux"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_tracer_mac_dry_run"
        status: pass
    human_judgment: false
  - id: D3
    description: "An interrupted wrapper run (Ctrl-C or unexpected error) leaves no session process group behind, via the sessions.pgid safety net and the EXIT trap"
    requirement: "PAR-01"
    verification:
      - kind: unit
        ref: "python/tests/test_gpu_phase6_parity_script.py#test_tracer_mac_dry_run"
        status: pass
    human_judgment: false
  - id: D4
    description: "Bind scripts/parity_check.py's --rust-server-cmd/--stress-server-cmd/--stress-cmd defaults to Phase 5's delivered interface, and prove the real Rust frontend over mock-scheduler plus Phase 5's stress tool against an already-running server, before any GPU time is spent"
    requirement: "PAR-02"
    verification: []
    human_judgment: true
    rationale: "Not attempted. Task 2's precondition is unmet: Phase 5's 128-agent stress tool (crates/rsg-server/tests/stress_128.rs) has no way to target an already-running server by base URL, and D-11 forbids changing the tool in this phase. See Deviations/Checkpoint below -- a human decision is required before this deliverable can proceed."

duration: ~75min (Task 1 complete; Task 2 halted before any implementation work began)
completed: 2026-10-07
status: halted
---

# Phase 6 Plan 6: GPU Wrapper Tracer Complete, Task 2 Halted on an Unmet Precondition Summary

**`scripts/gpu_phase6_parity.sh` chains cargo build, discover, run, validate and all four verdicts into one human-run PASS/FAIL sheet, proven on the Mac against stub tools and the fake parity server -- but Task 2 (binding its defaults to Phase 5's real stress-test interface) halted because that tool has no way to run against an external server at all.**

## Performance

- **Duration:** ~75 min active work (research + implementation + debugging + verification for Task 1; Task 2 was a precondition check only, no implementation)
- **Completed:** 2026-10-07
- **Tasks:** 1 of 2 completed (Task 1 done and committed; Task 2 halted before any code was written)
- **Files modified:** 3 (2 created, 1 modified)

## Accomplishments

- `scripts/gpu_phase6_parity.sh`: the Phase 6 human-run GPU wrapper, following the `gpu_phase1_check.sh`/`gpu_phase2_profile.sh` convention exactly -- `usage()`, an unknown-argument-exits-2 loop, `ROOT`/`PYTHON`/`PYTHONPATH` setup, a fresh `mktemp -d` log directory, `record()`/`RESULTS`, a `cleanup()` EXIT trap, a source guard, and a preflight check for `nvidia-smi`/`cargo`. Ten steps: release build (`--all-targets`, so Phase 5's stress tool is compiled too), discover for both frontends, the full `parity_check.py run`, `validate --require-gpu`, `verdict --criterion 1..4`, and `check_upstream.py`. Steps 5-9 record `FAIL "skipped: run wrote no sidecar"` when the run step wrote nothing.
- `sweep.run_session` now appends each launched session's process-group id to `work_dir/sessions.pgid` right after `launch_server` returns (T-06-14); the wrapper's `cleanup()` reads every `$LOG_DIR/*/sessions.pgid` file on EXIT and `kill -9`s each recorded group as a safety net, on top of `parity_check.py`'s own per-session teardown.
- Proved end to end on the Mac (`test_tracer_mac_dry_run`): with stub `nvidia-smi` (answering both `--query-gpu` and `--query-compute-apps`) and stub `cargo` (exits 0) on `PATH`, and every `--*-server-cmd`/`--stress-cmd` pointed at `rsglang.testing.fake_parity_server`'s `server`/`stress` subcommands, the wrapper runs its full ten-step pipeline against the real 128-item canonical corpus and both fake-server flavors, across all of `endpoints`/`sequential`/`concurrent`/`stress`. Every step passes (`PASS` for 1-4, 6-10) except step 5 (`validate --require-gpu`), which fails for exactly the reason it should: `sidecar.validate_sidecar`'s `require_gpu` check requires `meta.platform` to start with `"linux"`, and this Mac reports `darwin`. After the run exits, no `fake_parity_server`/`fake-parity-scheduler` process remains alive.
- Found and fixed a genuine bash bug while writing the wrapper (Rule 1): `local frontend="$1" override="$2" log="$LOG_DIR/discover-$frontend.log"` throws `frontend: unbound variable` under `set -u`. All the RHS expressions in a single `local` statement are evaluated against the *outer* scope before any of that statement's own new locals take effect -- referencing an earlier name from the *same* `local` statement is not the left-to-right assignment the text suggests. Fixed in both `run_discover` and `run_verdict` by splitting each into two separate `local` statements.

## Task Commits

1. **Task 1: Tracer -- the GPU wrapper drives the whole Phase 6 pipeline on the Mac against stubs and fakes** - `295ab9d` (feat)

Task 2 produced no commits (halted before any implementation).

**Plan metadata:** this commit.

## Files Created/Modified

- `scripts/gpu_phase6_parity.sh` - the Phase 6 GPU wrapper (new, executable)
- `python/tests/test_gpu_phase6_parity_script.py` - `test_help_anywhere`, `test_unknown_arg_exits_2`, `test_preflight_missing_tool_fails`, `test_tracer_mac_dry_run` (new)
- `python/rsglang/parity/sweep.py` - `run_session` appends `handle.pgid` to `work_dir/sessions.pgid`
- `.planning/phases/06-gpu-end-to-end-parity/deferred-items.md` - logs two out-of-scope discoveries found while verifying Task 1 (a `.venv` missing `uvicorn`, and a re-confirmation of the pre-existing Phase 4 `rsg-tokenizer` test-parallelism flake); neither is fixed here (new)

## Decisions Made

- Task 1 executed and committed exactly as planned, following the existing `gpu_phase1_check.sh`/`gpu_phase2_profile.sh` convention with no structural deviation.
- Task 2 did not begin any implementation. Its `<precondition>` was evaluated first, per the plan's own instruction ("If Phase 5's stress tool has no way to target an already-running server, stop. The precondition is unmet, and D-11 forbids changing the tool in this phase. Report the gap."), and found unmet -- see Deviations below for the full finding.
- `scripts/gpu_phase6_parity.sh`'s `--help` text and `scripts/parity_check.py`'s `--stress-cmd`/`--stress-server-cmd`/`--rust-server-cmd` defaults were deliberately left untouched (still empty / still the pre-existing Python-launcher defaults). Touching them was Task 2's job, and Task 2 is blocked.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `local` statement with a cross-reference to its own just-declared variable throws `unbound variable` under `set -u`**
- **Found during:** Task 1, while running `test_tracer_mac_dry_run` for the first time
- **Issue:** `run_discover()`'s `local frontend="$1" override="$2" log="$LOG_DIR/discover-$frontend.log"` and `run_verdict()`'s equivalent line both reference a variable (`$frontend`, `$n`) declared earlier in the *same* `local` statement, in that statement's own later assignment. Under `set -u`, bash evaluates every RHS expression in a `local` statement against the scope that existed *before* the statement ran, so the just-declared local isn't visible yet to a later assignment in the same statement -- producing `<name>: unbound variable` even though the code reads as straightforward left-to-right assignment.
- **Fix:** Split each into two separate `local` statements, so the first statement's assignment is fully in effect before the second statement's RHS is evaluated.
- **Files modified:** `scripts/gpu_phase6_parity.sh`
- **Verification:** `test_tracer_mac_dry_run` failed with exactly this error before the fix, and passes (all 4 tests in the file, including this one) after it.
- **Committed in:** `295ab9d` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary for the wrapper to run past its first `discover` step at all. No scope creep -- the fix is confined to the two functions that had the bug.

## CHECKPOINT: Task 2 halted on an unmet precondition

**Type:** human-verify
**Gate:** blocking-human

**What Task 2's `<precondition>` required:** that Phase 5's SUMMARY files document (a) the Mac command that serves `rsg-server`'s HTTP API backed by `mock-scheduler`, (b) how a `--frontend rust` launch selects `--abort-timing`, and (c) **the 128-agent stress test's command plus the option that points it at an already-running server's base URL**.

**What I found, read directly from the Phase 5 code now merged into this branch:**

- `crates/rsg-server/tests/stress_128.rs` **is** Phase 5's 128-agent cancellation stress test (LIFE-03, D-04). It is a `#[tokio::test(flavor = "multi_thread", worker_threads = 8)]` **cargo integration test function**, not a standalone binary or CLI tool.
- Inside that test, `TestServer::start(...)` (`crates/rsg-server/tests/common/test_server.rs`) spawns a **fresh, in-process** `mock-scheduler` subprocess, wires rsg-server's writer/dispatcher/engine/HTTP router onto it **in the same test process**, and binds an OS-assigned port -- all inside the test function itself. There is no CLI flag, environment variable, or alternate entry point to point this test at a server that is already running elsewhere.
- The test's own correctness assertions depend on **in-process state that only exists because the test started everything itself**: `server.snapshot_when_idle(...)` reads the engine's registry directly (`self.engine.registry().snapshot()`), and `server.mock.observed()` reads the mock scheduler's in-process observation log for submit/abort ordering. Neither is reachable from outside the test process, let alone over a wire protocol to an externally-running server.
- `crates/rsg-server/src/bin/` contains exactly two binaries: `rsg-server` (the frontend) and `mock-scheduler` (the Mac backend stand-in). There is no third binary for a stress client.
- `python/rsglang/launch.py --frontend rust` spawns the **real upstream scheduler** (GPU-only); there is no `--mock`/Mac-compatible mode that serves `rsg-server`'s HTTP API against `mock-scheduler` as a ready, documented CLI command either (precondition item (a) is also effectively unmet as a standalone invocation, though less central to the blocker).
- 05-07-SUMMARY.md itself confirms the design intent: *"The stress test is explicitly a throwaway, Phase-5-only correctness check (D-04): Phase 7's benchmark harness should not extend or reuse `stress_128.rs` as its load generator..."* -- it was never built to be pointed at an external process.

**Why I did not build a workaround:** the task's own instructions, and 05-CONTEXT.md's D-11 ("Criterion 4's 128-request cancellation stress test against the real backend **reuses Phase 5's throwaway stress-test tool as-is** ... no changes to the tool itself"), both forbid modifying the stress tool in this phase. Giving `stress_128.rs` an external-target mode would mean either (a) rewriting it as a standalone binary that drives HTTP requests without the in-process registry/mock-observation assertions it currently relies on for correctness (a different, less-proven tool, not "as-is"), or (b) adding a new CLI surface to the test binary that doesn't exist today -- both are the kind of structural change Rule 4 reserves for an explicit human decision, and D-11 says not to make it at all in this phase.

**The gap this leaves:** `scripts/parity_check.py`'s `--stress-cmd` has no real default today (still `""`, required when `--parts` includes `stress`); the GPU wrapper's step 4 (`parity_check.py run`) will fail with "`--stress-cmd` must be set when `--parts` includes stress" on the actual GPU box unless a human supplies `--stress-cmd`/`--stress-server-cmd` overrides by hand. Criterion 4 (ROADMAP Phase 6, PAR-02's abort-during-prefill reproduction question) cannot be exercised against the real backend through the mechanism this plan assumed existed.

**Options for resolving this (not decided here -- genuinely a human/re-plan call):**
1. Re-plan Task 2/D-11 to build a **new**, purpose-built external-target stress driver for the real-backend run (an architectural addition, likely its own plan) -- e.g. a thin HTTP-only client mirroring `stress_128.rs`'s request-mix logic but without the in-process assertions, accepting that it is no longer literally "Phase 5's tool, unmodified."
2. Run `stress_128.rs` itself against the real backend by pointing **its own** `TestServer::start`/mock-scheduler wiring at the real scheduler's `ipc://` addresses instead of spawning a fresh mock -- i.e., teach the *existing* cargo test to optionally skip spawning its own mock-scheduler and connect to an already-configured backend. This keeps the test's own in-process assertions (registry snapshot, submit/abort ordering) meaningful even on the real backend, but is still a change to the tool's code, which D-11 currently forbids as a category; whether this specific kind of change counts as "reusing the tool as-is" (same assertions, same request mix, only the transport target changes) is itself the re-plan decision.
3. Accept that PAR-02 criterion 4's "stress test against the real backend" cannot be automated by `scripts/gpu_phase6_parity.sh` and instead have a human run `cargo test -p rsg-server --test stress_128 -- --nocapture` **by hand** directly against a real-backend-wired build (not through `parity_check.py`'s session/tap machinery at all), accepting weaker automation and no sidecar-recorded evidence for this one criterion.

**Resume signal:** A human (or `/gsd-plan-phase` re-run for this plan) picks one of the above, or another approach, and either amends 06-06-PLAN.md's Task 2 or inserts a new plan before it. Once Task 2's precondition is satisfiable, re-execute this plan to pick it up (Task 1's commit and this SUMMARY stay; Task 2 starts fresh).

## Issues Encountered

Task 2 could not proceed past its precondition check. This is the designed, expected outcome the task's own instructions call out explicitly ("This is a legitimate, expected halt outcome for this task, not a failure to work around"), not an implementation problem.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Task 1's GPU wrapper is complete, committed, and fully proven on the Mac. It is reusable as-is once Task 2's blocker is resolved -- no part of it needs to change for that resolution.
- 06-07 (`depends_on: ["06-06"]`) and 06-08 (`depends_on: ["06-07"]`) are correctly blocked by this plan's `status: halted` until a human resolves the Task 2 gap and this plan is re-summarized as `complete`.
- PAR-01 and PAR-02 remain marked `Complete` in `REQUIREMENTS.md` from plans 06-03/06-05 respectively; this halt does not reopen either of those -- it blocks only this plan's own Task 2 deliverable (binding the GPU wrapper's stress defaults to Phase 5's real interface) and everything downstream of it.
- Blocker logged to STATE.md's Blockers/Concerns.

---
*Phase: 06-gpu-end-to-end-parity*
*Completed: 2026-10-07 (partial -- Task 1 only; Task 2 halted)*

## Self-Check: PASSED

- `scripts/gpu_phase6_parity.sh` found on disk, executable.
- `python/tests/test_gpu_phase6_parity_script.py` found on disk.
- `python/rsglang/parity/sweep.py`'s `sessions.pgid` append confirmed present (`git show 295ab9d`).
- Commit `295ab9d` found in `git log`.
- Re-ran Task 1's full `<verify>`: `.venv/bin/python -m pytest python/tests/test_gpu_phase6_parity_script.py -q` -> 4 passed; `bash -n scripts/gpu_phase6_parity.sh` -> exit 0.
- Re-ran Task 1's `<acceptance_criteria>`: pytest 4 passed; `--help` exit 0; `grep -c -- "--criterion"` -> 5; `grep -c "sessions.pgid"` -> 2 in the script, 1 in sweep.py.
- Plan-level `<verification>`'s second half (`bash scripts/check_all.sh --offline`) was run; it fails at step 1 on a pre-existing, already-documented flake unrelated to this plan's changes (`cargo test -p rsg-tokenizer`'s `loader::tests::gated_access_unavailable_with_blank_token_file` under default parallel test threads -- see STATE.md's Phase 4 tech-debt blocker). Confirmed not a regression: `cargo test -p rsg-tokenizer --lib -- --test-threads=1` -> 17 passed, 0 failed. The plan-level verification's first half (`pytest ... test_parity_rust_mock.py`) cannot run because that file is Task 2's undelivered output.
- Additionally ran the full `.venv/bin/python -m pytest python/tests -q` (not required by the plan, but run for thoroughness given this plan touches a shared helper, `sweep.run_session`): 240 passed, 37 skipped, 2 failed. Both failures (`test_gen_api_fixtures.py::test_committed_fixtures_are_fresh`, `test_python_frontend.py::test_tracer_python_frontend_serves_generate_against_mock`) trace to the same pre-existing, unrelated cause -- `uvicorn` missing from this checkout's `.venv` (confirmed directly: `import uvicorn` raises `ModuleNotFoundError`). Neither failing test, nor `uvicorn`, is touched by this plan. Logged to the new `.planning/phases/06-gpu-end-to-end-parity/deferred-items.md`, not fixed here.
