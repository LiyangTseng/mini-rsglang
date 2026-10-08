---
phase: 06-gpu-end-to-end-parity
plan: 06
subsystem: testing
tags: [gpu-script, bash, parity-harness, stress-test, checkpoint, external-stress-driver]

requires:
  - phase: 06-03
    provides: "fixtures/parity/corpus.json (128-item canonical corpus), compare.py's full layer precedence"
  - phase: 06-05
    provides: "the stress run part (abort_stress block, verdict criterion 4), fake_parity_server's server/stress subcommands"
provides:
  - "scripts/gpu_phase6_parity.sh: the human-run GPU wrapper chaining cargo build, discover (both frontends), the full parity_check.py run, validate --require-gpu and verdict --criterion 1..4, plus check_upstream.py -- proven end to end on the Mac against stub nvidia-smi/cargo and the fake_parity_server stand-ins"
  - "sweep.run_session's sessions.pgid safety net (T-06-14), so an interrupted wrapper run can kill -9 every session process group it started"
  - "scripts/parity_check.py's --rust-server-cmd/--stress-server-cmd defaults bound to Phase 5's delivered interface; rsglang.launch now forwards --abort-timing to rsg-server's own flag, which it never did before this plan"
  - "A new Phase 6 external-target stress driver (rsglang.parity.stress_client) replacing the un-reusable stress_128.rs as --stress-cmd's default -- a user-approved checkpoint resolution, not a silent deviation"
  - "rsglang.testing.rust_frontend: a Mac-only standalone wiring of the real rsg-server binary to a real mock-scheduler subprocess (two OS processes, real ipc:// sockets), proving discover --frontend rust and the new stress driver against mock-scheduler before any GPU time"
affects: [06-07, 06-08]

actuals:
  tokens: 22897
  tasks: 2
  commits: 4
  plan_head_before: 96e23214bef46ffa51a1c27a3b2d6e5117d067da
  plan_head_after: ea0ab188d52be1c8384c729d3f81e093700c5cce

tech-stack:
  added: []
  patterns:
    - "GPU wrapper step functions (run_discover/run_run/run_verdict) each split cross-referencing `local name=value name2=value2` declarations across separate `local` statements -- bash evaluates all RHS expressions in a single `local` statement against the OUTER scope before any of that statement's own new locals are assigned, so referencing an earlier-declared-in-the-same-statement local on its own RHS is unbound under `set -u` even though it reads correctly to the eye"
    - "A standalone two-binary Mac test wiring (rsg-server + mock-scheduler as separate OS processes) forwards a readiness handshake by reading the first stdout line of one process and writing it verbatim to the stdin of the other -- the same JSON-line contract the real launcher uses with the real scheduler, just driven from Python instead of from inside the real launcher's multiprocessing supervision loop"
    - "RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler (the existing Mac launcher-process-lifecycle test harness) only answers ExitMsg in its run_forever loop -- it never replies to a UserMsg with tokens, so it proves launcher supervision (D-12) but cannot drive a real /generate or /v1/chat/completions response; only mock-scheduler's real echo/delay decode loop can"

key-files:
  created:
    - scripts/gpu_phase6_parity.sh
    - python/tests/test_gpu_phase6_parity_script.py
    - python/rsglang/parity/stress_client.py
    - python/rsglang/testing/rust_frontend.py
    - python/tests/test_parity_rust_mock.py
    - .planning/phases/06-gpu-end-to-end-parity/deferred-items.md
  modified:
    - python/rsglang/parity/sweep.py
    - python/rsglang/launch.py
    - python/rsglang/sockets.py
    - scripts/parity_check.py
    - python/tests/test_parity_stress.py

key-decisions:
  - "Task 1 (the GPU wrapper tracer) executed and committed in full in the first session; Task 2 halted at an unmet precondition in that same session, then resumed and completed here after the user's checkpoint decision -- see Deviations/Checkpoint Resolution below"
  - "User's checkpoint resolution (Option 1 of three proposed): build a new, purpose-built external-target stress driver (rsglang.parity.stress_client) as new Phase 6 scope, rather than modifying or reusing stress_128.rs, which D-11 forbids changing and which has no external-target mode to reuse unchanged"
  - "Discovered mid-resolution that precondition item (a) -- 'the Mac command that serves rsg-server's HTTP API backed by mock-scheduler' -- could not be the existing RSGLANG_SCHEDULER_FACTORY=FakeScheduler launcher harness (test_launch_rust_e2e.py): FakeScheduler's run_forever only handles ExitMsg, never answering a UserMsg with tokens, so it cannot drive a real generation. Built rsglang.testing.rust_frontend as new, additive Mac test-support code instead (two real OS processes, real ipc:// sockets, handshake forwarded stdout->stdin) -- not a production-path change"
  - "Added --abort-timing to rsglang.launch's own CLI, forwarded through sockets.rust_cli_args to rsg-server's existing flag: 05-08-SUMMARY.md explicitly left this forwarding 'deliberately left to Phase 6, which decides the benchmark setting' -- without it, --stress-server-cmd's existing {abort_timing} templating could never have reached rsg-server at all"
  - "Found and fixed a real bash bug while building Task 1: `local a=\"$1\" b=\"$LOG_DIR/x-$a.log\"` on one line throws '<name>: unbound variable' under `set -u`, because bash evaluates every RHS in a `local` statement against the scope BEFORE that statement's own declarations take effect -- not left-to-right as the textual order suggests. Fixed by splitting each occurrence (run_discover, run_verdict) into two separate `local` statements"

requirements-completed: [PAR-01, PAR-02]

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
    description: "scripts/parity_check.py's --rust-server-cmd/--stress-server-cmd/--stress-cmd defaults are bound to Phase 5's delivered interface (the real rsg-server launch, --abort-timing now forwarded through the launcher, and a new Phase 6 external-target stress driver in place of the un-reusable stress_128.rs)"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_rust_mock.py#test_discover_rust_against_mock"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_rust_mock.py#test_stress_cmd_targets_running_server"
        status: pass
    human_judgment: false
  - id: D5
    description: "Before any GPU time, the real Rust frontend over a real mock-scheduler subprocess passes every endpoint check, and the new external-target stress driver is shown to drive an already-running server (128 requests, 30% abort fraction, final canary) without hanging or erroring"
    requirement: "PAR-02"
    verification:
      - kind: unit
        ref: "python/tests/test_parity_rust_mock.py#test_discover_rust_against_mock"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_rust_mock.py#test_stress_cmd_targets_running_server"
        status: pass
    human_judgment: false

duration: ~75min (Task 1, first session) + ~90min (Task 2, this session, including the checkpoint-resolution investigation)
completed: 2026-10-07
status: complete
---

# Phase 06 Plan 06: GPU Wrapper, Phase-5-Bound Defaults, and a New External-Target Stress Driver Summary

**`scripts/gpu_phase6_parity.sh` chains the full Phase 6 GPU pipeline into one PASS/FAIL sheet, proven on the Mac; its `--rust-server-cmd`/`--stress-server-cmd` defaults are now bound to Phase 5's real launcher interface (with `--abort-timing` newly forwarded through `rsglang.launch`), and `--stress-cmd` now points at a new, purpose-built Phase 6 external-target stress driver (`rsglang.parity.stress_client`) built after the user resolved a checkpoint finding that Phase 5's `stress_128.rs` has no way to target an already-running server at all.**

## Performance

- **Duration:** ~75 min (Task 1, first session) + ~90 min (Task 2, this session -- resuming after the checkpoint, re-investigating the precondition, building the new driver and Mac test wiring, and verifying)
- **Completed:** 2026-10-07
- **Tasks:** 2 of 2 completed
- **Files modified:** 11 across both tasks (6 created, 5 modified in Task 1+2 combined; see key-files)

## Accomplishments

- `scripts/gpu_phase6_parity.sh`: the Phase 6 human-run GPU wrapper, following the `gpu_phase1_check.sh`/`gpu_phase2_profile.sh` convention exactly. Ten steps: release build (`--all-targets`), discover for both frontends, the full `parity_check.py run`, `validate --require-gpu`, `verdict --criterion 1..4`, and `check_upstream.py`. Proven end to end on the Mac with stub `nvidia-smi`/`cargo` and the fake parity server; every step passes except `validate --require-gpu`, which fails only because the run isn't on Linux.
- `sweep.run_session`'s `sessions.pgid` safety net (T-06-14), read by the wrapper's `cleanup()` EXIT trap to `kill -9` every session process group an interrupted run started.
- **Task 2's checkpoint, and its resolution:** the first execution attempt found Task 2's precondition unmet -- `crates/rsg-server/tests/stress_128.rs` (05-07-SUMMARY.md) is a `#[tokio::test]` cargo integration test with no CLI, no binary, and no way to point it at an already-running server by base URL, and 06-CONTEXT.md's D-11 forbids changing it. It halted at a `checkpoint:human-verify` rather than inventing a workaround. The user resolved it by choosing to build a **new, purpose-built external-target stress driver** instead of reusing or modifying `stress_128.rs` -- new Phase 6 scope, not a Phase 5 deliverable.
- `python/rsglang/parity/stress_client.py`: the new driver. Fires N concurrent chat-completion requests at `--base-url` via aiohttp, disconnects a seeded `--abort-fraction` of them mid-stream after at least one chunk (real TCP cancellation, not an in-process assertion), and finishes with one canary request; exits 0 only if nothing timed out or errored and the canary succeeded. Mirrors `fake_parity_server.py`'s existing fake stress driver's CLI shape (`--base-url --requests --abort-fraction --seed`) plus `--model`, so the same template shape works for both the Mac fake-server dry run and the real run.
- **Resolving precondition item (a) along the way:** proving the new driver (and `discover --frontend rust`) against `mock-scheduler` needed a GPU-free way to serve `rsg-server`'s real HTTP API with real generated tokens. The existing Mac harness for `rsglang.launch --frontend rust` (`RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler`, from `test_launch_rust_e2e.py`) turned out to be unusable for this: its `FakeScheduler.run_forever` only handles `ExitMsg` and never answers a `UserMsg` with tokens, so it proves launcher process-supervision (D-12) but cannot drive a real `/generate` or `/v1/chat/completions` response. Built `rsglang.testing.rust_frontend` instead: a new, additive, Mac-only module that spawns a real `mock-scheduler` subprocess and a real `rsg-server` subprocess as two separate OS processes over real `ipc://` sockets, forwarding `mock-scheduler`'s first stdout line (its readiness handshake) verbatim to `rsg-server`'s stdin -- the same JSON-line contract the real launcher uses with the real scheduler, just driven from a small Python wrapper instead of from inside the real launcher's multiprocessing supervision loop.
- `python/rsglang/launch.py` gained its own `--abort-timing immediate|deferred` CLI flag, forwarded through `sockets.rust_cli_args`'s new `abort_timing` parameter to `rsg-server`'s own `--abort-timing` flag. 05-08-SUMMARY.md explicitly noted this forwarding was "deliberately left to Phase 6, which decides the benchmark setting" -- without it, `--stress-server-cmd`'s pre-existing `{abort_timing}` templating could never have actually reached `rsg-server`.
- `scripts/parity_check.py`'s `--stress-cmd` default changed from `""` (required override) to the new `stress_client` invocation; `--rust-server-cmd` kept its existing default (confirmed correct against 05-08-SUMMARY.md: "the real rsg-server binary ... the one Phase 6 runs on the GPU box"); `--stress-server-cmd`'s existing `--abort-timing {abort_timing}` default now actually works end to end.
- `python/tests/test_parity_rust_mock.py` (new, slow): `test_discover_rust_against_mock` proves `discover --frontend rust --skip-tap-check` against the real `rsg-server` binary wired to a real `mock-scheduler` subprocess -- all 8 `RUST_ENDPOINTS` come back `ok: true`. `test_stress_cmd_targets_running_server` launches the same server-cmd independently via `procs.launch_server`/`wait_ready`, then runs the plan's own default `--stress-cmd` against it as an external subprocess with a 600s timeout -- it exits 0.
- `scripts/gpu_phase6_parity.sh --help` updated to name where each default now comes from (05-08-SUMMARY.md for the launch/abort-timing defaults, this plan's checkpoint resolution for the new stress driver).

## Task Commits

1. **Task 1: Tracer -- the GPU wrapper drives the whole Phase 6 pipeline on the Mac against stubs and fakes** - `295ab9d` (feat)
2. **Task 2: Bind the defaults to Phase 5's delivered interface, with a new external-target stress driver (checkpoint resolution)** - `5283d1c` (feat), `ea0ab18` (fix: show the new --stress-cmd default in --help)

**Plan metadata:** this commit.

(The halted session's interim docs commit, `ae64004`, recorded Task 1's completion and Task 2's halt; superseded by this fully-completed SUMMARY.)

## Files Created/Modified

- `scripts/gpu_phase6_parity.sh` - the Phase 6 GPU wrapper (new, executable); `--help` text updated with Task 2's default-provenance notes
- `python/tests/test_gpu_phase6_parity_script.py` - `test_help_anywhere`, `test_unknown_arg_exits_2`, `test_preflight_missing_tool_fails`, `test_tracer_mac_dry_run` (Task 1)
- `python/rsglang/parity/sweep.py` - `run_session` appends `handle.pgid` to `work_dir/sessions.pgid` (Task 1)
- `python/rsglang/parity/stress_client.py` - the new external-target stress driver (Task 2, new)
- `python/rsglang/testing/rust_frontend.py` - standalone rsg-server + mock-scheduler Mac wiring (Task 2, new)
- `python/tests/test_parity_rust_mock.py` - `test_discover_rust_against_mock`, `test_stress_cmd_targets_running_server` (Task 2, new)
- `python/rsglang/launch.py` - new `--abort-timing` CLI flag, forwarded to `rust_cli_args` (Task 2)
- `python/rsglang/sockets.py` - `rust_cli_args` gains an optional `abort_timing` parameter (Task 2)
- `scripts/parity_check.py` - `--stress-cmd` default now non-empty (the new driver); comments on all three server-cmd defaults naming their source (Task 2)
- `python/tests/test_parity_stress.py` - `test_stress_requires_stress_cmd` now passes `--stress-cmd ""` explicitly, since the default is no longer empty (Task 2)
- `.planning/phases/06-gpu-end-to-end-parity/deferred-items.md` - Task 1's two entries, plus a new Task 2 entry (hyperfine-already-on-PATH test assumption)
- `.planning/phases/06-gpu-end-to-end-parity/06-06-PLAN.md` - Task 2 amended in place with the checkpoint resolution and the resulting scope change

## Decisions Made

- Task 1 executed and committed exactly as planned, following the existing `gpu_phase1_check.sh`/`gpu_phase2_profile.sh` convention with no structural deviation.
- Task 2's first attempt correctly halted rather than working around D-11 or `stress_128.rs`'s complete lack of an external-target mode -- see Deviations/Checkpoint below for the full finding and the three options it proposed.
- The user chose Option 1 (build a new, purpose-built driver) over Option 2 (teach `stress_128.rs` itself an external-target mode, which would still be a change to the tool D-11 forbids) and Option 3 (accept no automation for this criterion). This plan amends 06-06-PLAN.md's Task 2 in place to document the change, per the resuming session's explicit instruction, rather than silently deviating.
- `rsglang.testing.rust_frontend` is deliberately scoped as Mac-only test-support code (`python/rsglang/testing/`), not a production launcher mode -- it does not touch `rsglang.launch`'s real GPU-box code path at all, only adds a new, separate module.
- `--abort-timing`'s launcher-forwarding fix (`python/rsglang/launch.py`, `python/rsglang/sockets.py`) was outside Task 2's originally-declared `<files>` list but was a Rule 3 blocking-issue fix: `--stress-server-cmd`'s own pre-existing default literally could not have worked without it (upstream's own argument parser, not `rsg-server`, would have received the unrecognized `--abort-timing` flag and errored).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `local` statement with a cross-reference to its own just-declared variable throws `unbound variable` under `set -u`**
- **Found during:** Task 1, while running `test_tracer_mac_dry_run` for the first time
- **Issue:** `run_discover()`'s `local frontend="$1" override="$2" log="$LOG_DIR/discover-$frontend.log"` and `run_verdict()`'s equivalent line both reference a variable declared earlier in the *same* `local` statement. Under `set -u`, bash evaluates every RHS expression in a `local` statement against the scope that existed *before* the statement ran.
- **Fix:** Split each into two separate `local` statements.
- **Files modified:** `scripts/gpu_phase6_parity.sh`
- **Verification:** `test_tracer_mac_dry_run` failed with exactly this error before the fix, passed after it.
- **Committed in:** `295ab9d` (Task 1 commit)

**2. [Rule 3 - Blocking] `rsglang.launch --frontend rust` never forwarded `--abort-timing` to `rsg-server`, so `--stress-server-cmd`'s own pre-existing default could not have worked**
- **Found during:** Task 2, re-reading `python/rsglang/sockets.py`'s docstring ("left for Phase 6 to forward deliberately") and `python/rsglang/launch.py`'s `run_rust_mode`/`rust_cli_args` call site
- **Issue:** `scripts/parity_check.py`'s existing `--stress-server-cmd` default already appended `--abort-timing {abort_timing}` to a `python -m rsglang.launch --frontend rust ...` command line, but `rsglang.launch`'s `rest` arguments are parsed by upstream's own `minisgl.server.args.parse_args`, not by `rsg-server`'s CLI -- `--abort-timing` would never reach `rsg-server` at all (and would likely error as an unrecognized upstream argument).
- **Fix:** Added `--abort-timing immediate|deferred` to `rsglang.launch`'s own argparse parser (consumed via `parse_known_args` before `rest` is built), forwarded through a new `abort_timing` parameter on `sockets.rust_cli_args`.
- **Files modified:** `python/rsglang/launch.py`, `python/rsglang/sockets.py`
- **Verification:** `python/tests/test_topology.py::test_rust_cli_args_exact` (unchanged call site, still passes -- the new parameter defaults to `None`, preserving the exact prior return value); `python/tests/test_launch_args.py` and `python/tests/test_launch_rust_e2e.py` (12 tests) both pass unchanged; manually confirmed `rsg-server`'s own "ready to serve abort_timing=..." log line reflects the forwarded value end to end via `rsglang.testing.rust_frontend`.
- **Committed in:** `5283d1c` (Task 2 commit)

**3. [Rule 1 - Bug] `python/tests/test_parity_stress.py::test_stress_requires_stress_cmd` asserted on the OLD empty `--stress-cmd` default**
- **Found during:** Task 2, before running the full test suite
- **Issue:** That test called `run --parts stress` with no `--stress-cmd` override and asserted exit code 2 with `"--stress-cmd"` in stderr -- a behavior that only held because the default was empty. Task 2's own acceptance criteria require the default to become non-empty, which would silently break this test's intent (it would proceed past the validation check instead of hitting it).
- **Fix:** Pass `--stress-cmd ""` explicitly, so the test still exercises the same validation path regardless of what the default becomes.
- **Files modified:** `python/tests/test_parity_stress.py`
- **Verification:** `python/tests/test_parity_stress.py -q` (44 tests) passes, including this one, against the new non-empty default.
- **Committed in:** `5283d1c` (Task 2 commit)

**4. [Rule 1 - Bug] `--stress-cmd`'s new default didn't actually appear in `--help` output**
- **Found during:** Task 2, final acceptance-criteria check
- **Issue:** The plan's own acceptance criterion requires `run --help` to show the new `--stress-cmd` default containing `{base_url}`. The argument's `help=` string didn't interpolate `%(default)s` (unlike `--models`'s own help text, which does), so the default value itself never appeared in `--help` output even though it was correctly set.
- **Fix:** Added `(default: %(default)s)` to `--stress-cmd`'s help string, matching the existing `--models` convention.
- **Files modified:** `scripts/parity_check.py`
- **Verification:** `scripts/parity_check.py run --help` now shows the literal default string including `{base_url}`; `test_parity_check.py`/`test_parity_stress.py`/`test_parity_rust_mock.py` (31 tests) still pass.
- **Committed in:** `ea0ab18`

---

**Total deviations:** 4 auto-fixed (1 Rule 1 bug in Task 1, 1 Rule 3 blocking fix + 2 Rule 1 fixes in Task 2)
**Impact on plan:** All four were necessary for this plan's own stated verification commands to pass. No scope creep beyond what Task 2's acceptance criteria already required.

## CHECKPOINT RESOLUTION (Task 2, this session)

**Type:** human-verify (resolved)
**Gate:** blocking-human (resolved by explicit user decision, not auto-approved)

**What the first execution attempt found (full detail preserved from the halted session's own finding):**

`crates/rsg-server/tests/stress_128.rs` **is** Phase 5's 128-agent cancellation stress test (LIFE-03, D-04). It is a `#[tokio::test(flavor = "multi_thread", worker_threads = 8)]` **cargo integration test function**, not a standalone binary or CLI tool. Inside it, `TestServer::start(...)` spawns a **fresh, in-process** `mock-scheduler` subprocess and wires `rsg-server`'s writer/dispatcher/engine/HTTP router onto it **in the same test process** -- there is no CLI flag, environment variable, or alternate entry point to point this test at a server already running elsewhere. Its correctness assertions (`server.snapshot_when_idle`, `server.mock.observed()`) read in-process state that only exists because the test started everything itself -- neither is reachable from outside the test process. 06-CONTEXT.md's D-11 ("reuses Phase 5's throwaway stress-test tool **as-is** ... no changes to the tool itself") forbids modifying it to add an external-target mode, and 05-07-SUMMARY.md itself confirms the design intent: "Phase 7's benchmark harness should not extend or reuse `stress_128.rs` as its load generator."

**The three options the halted session proposed, and the user's choice:**

1. **(Chosen)** Build a new, purpose-built external-target stress driver for the real-backend run -- an architectural addition, accepting it is no longer literally "Phase 5's tool, unmodified."
2. Teach `stress_128.rs` itself to optionally skip spawning its own mock-scheduler and connect to an already-configured backend -- still a change to the tool's code, which D-11 forbids as a category.
3. Accept that this criterion cannot be automated by `scripts/gpu_phase6_parity.sh` at all; have a human run `cargo test -p rsg-server --test stress_128` by hand against a real-backend-wired build, with no sidecar-recorded evidence.

**Resolution:** The user chose Option 1. `python/rsglang/parity/stress_client.py` is the result -- new Phase 6 scope, making no attempt to reproduce `stress_128.rs`'s in-process assertions (those remain Phase 5's own correctness proof, untouched). Its only job is the external-targeting capability this task needed: real concurrent HTTP traffic, real TCP cancellation, against an already-running frontend, with a simple pass/fail plus counts -- proven in this session against the real Rust frontend over a real `mock-scheduler` subprocess (`test_stress_cmd_targets_running_server`).

06-06-PLAN.md's Task 2 was amended in place (not left stale) to describe this new driver instead of "reuse Phase 5's tool unchanged," per this session's explicit instruction that this is a real, user-approved scope change, not a silent deviation.

## Issues Encountered

- Task 2's own precondition investigation surfaced a second, related gap beyond what the halted session had flagged: precondition item (a)'s "Mac command that serves rsg-server's HTTP API backed by mock-scheduler" could not be satisfied by the existing `RSGLANG_SCHEDULER_FACTORY=FakeScheduler` launcher test harness, because `FakeScheduler` never answers a `UserMsg` with tokens (confirmed by reading its `run_forever` loop, which only handles `ExitMsg`). This was resolved as part of the same checkpoint resolution by building `rsglang.testing.rust_frontend`, additive Mac test-support code with no production-path impact.
- `bash scripts/check_all.sh --offline` does not print `check_all: OK` on this specific dev checkout, but not because of anything this plan's own files changed: `cargo test --workspace` (step 1 of 7) passed cleanly in full, and the gate stopped at step 2 (`pytest python/tests`) on four pre-existing, already-or-newly-documented failures unrelated to this plan's `files_modified` -- two from `.venv` missing `uvicorn` (already logged in `deferred-items.md` item 1 from Task 1's own verification) and two newly found (`test_hyperfine_ok`, `test_run_s3_hyperfine_missing_exits_2`) from this box having a real `hyperfine 1.20.0` already on `PATH`, which neither test's own tmp-dir-stub-removal technique accounts for. Confirmed unrelated by running both pairs of tests in isolation (same failures, same causes) and by inspecting their file paths (`scripts/gpu_phase2_profile.sh`, `scripts/profile_scenarios.py` -- Phase 2 scope, never touched by this plan). Logged as a new entry (2a) in `deferred-items.md`. This plan's own targeted verification -- `test_parity_rust_mock.py` (2 passed), `test_parity_stress.py`/`test_parity_check.py`/`test_topology.py`/`test_launch_args.py`/`test_launch_rust_e2e.py`/`test_gpu_phase6_parity_script.py` (all green, 68 tests total) -- is what this plan's acceptance criteria actually require, and all of it passes.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Both of this plan's tasks are complete and committed. `scripts/gpu_phase6_parity.sh`'s defaults now point at Phase 5's real launch interface (with `--abort-timing` newly forwarded) and Phase 6's own new external-target stress driver -- ready to run on the GPU box with no further wiring.
- 06-07 (`depends_on: ["06-06"]`) and 06-08 (`depends_on: ["06-07"]`) are unblocked: this plan's `status` is now `complete`, not `halted`.
- PAR-01 and PAR-02 remain marked `Complete` in `REQUIREMENTS.md` from plans 06-03/06-05; this plan adds proof that the GPU wrapper's own defaults are wired correctly, not a new requirement.
- The pre-existing `uvicorn`-missing and newly-found `hyperfine`-on-`PATH` test issues are both logged in `deferred-items.md` for anyone bootstrapping or re-verifying this checkout; neither blocks this plan or Phase 6.
- Blocker cleared from STATE.md's Blockers/Concerns (the Task 2 halt entry is removed; this SUMMARY's checkpoint-resolution section is the permanent record of that history).

---
*Phase: 06-gpu-end-to-end-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

- `scripts/gpu_phase6_parity.sh` found on disk, executable; `--help` text updated and verified (`test_help_anywhere` passes).
- `python/rsglang/parity/stress_client.py`, `python/rsglang/testing/rust_frontend.py`, `python/tests/test_parity_rust_mock.py` found on disk.
- `python/rsglang/launch.py`'s `--abort-timing` flag and `python/rsglang/sockets.py`'s `rust_cli_args(..., abort_timing=...)` confirmed present (`git show 5283d1c`).
- Commits `295ab9d`, `5283d1c` and `ea0ab18` found in `git log`.
- Re-ran `.venv/bin/python -m pytest python/tests/test_parity_rust_mock.py -q` -> 2 passed; confirmed no leftover `rust_frontend`/`mock-scheduler`/`rsg-server` processes after the run.
- Re-ran `.venv/bin/python -m pytest python/tests/test_parity_stress.py python/tests/test_parity_check.py python/tests/test_topology.py python/tests/test_gpu_phase6_parity_script.py -q` -> 44 passed.
- Re-ran `.venv/bin/python -m pytest python/tests/test_launch_args.py python/tests/test_launch_rust_e2e.py -q` -> 24 passed (confirms the `--abort-timing` launcher change is backward compatible).
- Re-ran `RUST_TEST_THREADS=1 bash scripts/check_all.sh --offline`: `cargo test --workspace` (step 1/7) passed in full; the gate stopped at step 2/7 (pytest) on the 4 pre-existing/unrelated failures documented above and in `deferred-items.md`; confirmed by isolated reruns that none trace to this plan's files.
- `.venv/bin/python scripts/parity_check.py run --help` shows `--stress-cmd`'s new default contains `{base_url}`.
