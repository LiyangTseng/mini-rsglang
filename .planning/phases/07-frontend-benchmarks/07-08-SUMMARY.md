---
phase: 07-frontend-benchmarks
plan: 08
subsystem: benchmarking
tags: [rust, hyperfine, cold-start, process-groups, tdd, orchestrator-lifecycle]

requires:
  - phase: 07-frontend-benchmarks
    provides: "07-06's shared orchestrator (TrialRunner, Lifecycle enum, run_session, SessionArgs/SessionConfig, Arm, D-07 manifest) and 07-01's procs.rs (ServerHandle, launch, wait_ready, teardown, check_signalable_pgid) -- this plan adds the RunnerManaged branch run_session never actually used before, plus procs.rs's detached-group teardown"
provides:
  - "rsg_bench::procs::{DetachedGroup, ServerHandle::detach, process_start_time, stop_detached} -- T-07-20 leader-identity-checked teardown of a server group that outlived its own ServerHandle"
  - "rsg_bench::cmdline::{shell_quote, shell_join} -- the shlex.quote/shlex.join equivalent (T-07-19)"
  - "rsg_bench::scenarios::s3_coldstart::{OnceArgs, coldstart_once, StopArgs, coldstart_stop, hyperfine_argv, HyperfineStats, parse_hyperfine_json, S3Args, S3Runner, ColdstartRecord} -- Scenario 3 (BENCH-05)"
  - "rsg-bench subcommands s3, coldstart-once, coldstart-stop"
  - "orchestrator::run_session's RunnerManaged branch (run_one_trial_runner_managed, run_one_trial_dispatch, build_trial_env) -- the Lifecycle enum 07-06 defined but never branched on"
affects: [07-09-standard-throughput-regression, 07-10-bench-06-report]

actuals:
  tokens: 19602
  tasks: 3
  commits: 5
plan_head_before: e1f41472c6e3bde24091c765b2f837a8e82d6a8f
plan_head_after: 97a7d1db78de569c6bba927167e0bd744953a4ca

tech-stack:
  added: []
  patterns:
    - "ColdstartRecord gets a hand-rolled Serialize/Deserialize instead of #[serde(tag = \"kind\")]: serde's internally-tagged-enum deserialization re-decodes the matched variant from a buffered Content value, and that buffered path does not correctly deserialize a non-string map key (roles: BTreeMap<i32, Role>) -- it round-trips fine standalone (TrialRecord.roles in manifest.rs already proves this), but silently fails inside a tagged enum (returns a parse Err that a naive filter_map(.ok()) swallows). Deserializing through serde_json::Value/serde_json::from_value instead uses the complete, non-buffered value deserializer and avoids the bug entirely -- discovered live during Task 1's own test run, not anticipated in the plan"
    - "build_trial_env extracts the gc-hook environment block (RSGLANG_PROFILE_DIR/_MODE/_INTERVAL_S plus PYTHONPATH, or the matching env_remove) that run_one_trial already built inline, so both the HarnessManaged and RunnerManaged lifecycles construct the exact same environment from the exact same SessionConfig (P1) -- a straight extraction, not a new design"
    - "run_one_trial_dispatch is one async fn that matches on TrialRunner::lifecycle() internally rather than branching at the call site, so run_session's per-slot tokio::select! sees a single concrete Future type without needing Box<dyn Future> -- the two lifecycle-specific trial runners stay fully separate functions"
    - "--marker-after-ms on bench-stub (07-01) is now relative to process start (the same clock --ready-delay-ms uses), not listener-bind time -- the prior ordering made the marker always land strictly after readiness, which cannot produce backend_ready_s < e2e_ready_s, a case this plan's own test oracle requires"

key-files:
  created:
    - crates/rsg-bench/src/scenarios/s3_coldstart.rs
    - crates/rsg-bench/tests/coldstart.rs
    - crates/rsg-bench/tests/hyperfine_parse.rs
    - crates/rsg-bench/tests/s3_coldstart_e2e.rs
    - crates/rsg-bench/tests/fixtures/hyperfine_export.json
  modified:
    - crates/rsg-bench/src/procs.rs
    - crates/rsg-bench/src/cmdline.rs
    - crates/rsg-bench/src/orchestrator.rs
    - crates/rsg-bench/src/main.rs
    - crates/rsg-bench/src/scenarios/mod.rs
    - crates/rsg-bench/src/memory.rs
    - crates/rsg-bench/src/bin/bench-stub.rs

key-decisions:
  - "ColdstartRecord's Serialize/Deserialize are hand-rolled (serde_json::Value round trip) instead of the originally-planned #[serde(tag = \"kind\")] derive, because the derive silently fails to deserialize the Mem variant's roles: BTreeMap<i32, Role> field when nested inside an internally-tagged enum -- confirmed live (parse error 'invalid type: string \"67312\", expected i32') before switching approaches"
  - "hyperfine_parse.rs's parse_phase2_shape compares the fixture's 17-significant-digit floats with an epsilon (1e-9), not bit-exact equality: serde_json's default (non-float_roundtrip) float parser is not always bit-identical to Rust's own literal parser at that precision -- changing serde_json's cargo feature workspace-wide was judged out of this plan's scope for a sub-nanosecond timing difference"
  - "hyperfine 1.20.0 was installed via cargo install --locked (not skipped) so both s3_coldstart_e2e.rs tests, including the hyperfine-gated one, actually ran and passed on this Mac -- the install was pre-vetted in 07-RESEARCH's Package Legitimacy Audit and the plan's own T-07-SC threat-model disposition, not a fresh ad hoc decision"
  - "run_one_trial_runner_managed reports gc_status: \"runner_managed\" (a value outside the WindowObs doc comment's collected/disabled/no_hook_records enumeration) for any window a RunnerManaged TrialRunner reports, since the orchestrator genuinely never attempted GC collection for it -- S3Runner itself always returns zero windows, so this value is reserved for a future RunnerManaged scenario that does report windows"

patterns-established:
  - "TDD discipline: Task 2 (hyperfine_argv/parse_hyperfine_json/shell_quote/shell_join, pure functions) and Task 3's orchestrator RunnerManaged branch both used genuine RED-GREEN cycles -- intentionally-wrong stubs (shell_quote returns the token unchanged; shell_join joins with ',' instead of a space; hyperfine_argv always returns an empty vec; parse_hyperfine_json always returns a zeroed stats struct; orchestrator::run_session left unconditionally HarnessManaged) were committed first, confirmed to fail the new tests on real assertions, then fixed to GREEN in a separate commit. Task 3's S3Args/S3Runner/rsg-bench-s3-subcommand wiring were implemented in their final correct form alongside the RED commit, since they are integration glue over Task 1/2's already-tested primitives, not the behavior under test -- documented explicitly rather than disguised as RED"

requirements-completed: []  # BENCH-05 is shared with 07-05 (complete) and 07-10 (not yet run); requirements.ready-ids reports 0/1 ready -- stays open in REQUIREMENTS.md per the shared-ID gate

coverage:
  - id: D1
    description: "coldstart-once launches a server, polls GET /v1/models for readiness while watching the server's own log for a backend-ready marker, appends a ready JSONL record (e2e_ready_s, backend_ready_s, frontend_tail_s), then detaches; coldstart-stop samples per-role-group memory at ready, appends a mem record, then tears the detached group down via procs::stop_detached's T-07-20 leader-identity check (killpg only when the pid equal to the recorded pgid still has the recorded start time; otherwise individual pids matching pgid+start-time are signalled)"
    requirement: BENCH-05
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/coldstart.rs#coldstart_once_then_stop_with_stub"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/coldstart.rs#marker_absent_gives_null_backend"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/coldstart.rs#once_self_heals_stale_group"
        status: pass
    human_judgment: false
  - id: D2
    description: "cmdline::{shell_quote, shell_join} port Python's shlex.quote/shlex.join exactly (T-07-19), verified against a real sh -c round trip including a ';rm -rf /' injection token; s3_coldstart::{hyperfine_argv, parse_hyperfine_json} port Phase 2's own functions field-by-field (RESEARCH Pattern 2) with explicit presence/type checks and path-naming errors, never a panic"
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/hyperfine_parse.rs#shell_quote_round_trip"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/hyperfine_parse.rs#hyperfine_argv_shape"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/hyperfine_parse.rs#parse_phase2_shape"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/hyperfine_parse.rs#parse_missing_results_errors"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/hyperfine_parse.rs#parse_null_stddev_ok"
        status: pass
    human_judgment: false
  - id: D3
    description: "rsg-bench s3 runs Scenario 3 as a hyperfine-backed RunnerManaged trial in the shared A/B orchestrator (D-08): hyperfine times coldstart-once end-to-end with --conclude running coldstart-stop, warm-up runs are excluded by run index, and the result reports hyperfine stats, per-kept-run frontend_tail_s/memory_at_ready/gc_boot, and trial-level means separately from end-to-end startup. orchestrator::run_session's new RunnerManaged branch (build_trial_env, run_one_trial_runner_managed, run_one_trial_dispatch) skips harness launch/teardown/memory-sampling entirely for such a runner, while still rendering argv and building the gc-hook env identically to a harness-managed trial (P1)"
    requirement: BENCH-05
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/s3_coldstart_e2e.rs#runner_managed_skips_harness_launch"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/s3_coldstart_e2e.rs#s3_session_with_hyperfine_and_stub_arms"
        status: pass
    human_judgment: false

duration: 90min
completed: 2026-10-06
status: complete
---

# Phase 07 Plan 08: Scenario 3 Cold Start, Hyperfine Wrapping and the Orchestrator's RunnerManaged Lifecycle Summary

**Scenario 3 (BENCH-05) added as a hyperfine-backed runner-managed trial: end-to-end cold start timed by hyperfine, the frontend's own critical-path tail measured separately from backend-ready log markers, and per-role-group memory at ready -- plus the orchestrator's `RunnerManaged` lifecycle branch that `run_session` had defined since 07-06 but never actually used until this plan's runner needed it.**

## Performance
- **Duration:** ~90min
- **Started:** 2026-10-06
- **Completed:** 2026-10-06
- **Tasks:** 3
- **Files modified:** 12 (5 created, 7 modified)

## Accomplishments
- `procs::{DetachedGroup, ServerHandle::detach, process_start_time, stop_detached}` (T-07-20): a server group can now outlive the `ServerHandle` that launched it, and be safely torn down from a *separate* process invocation later -- `stop_detached` verifies the pid equal to the recorded pgid still has the recorded start time before trusting `killpg`; otherwise it signals only pids individually matching pgid+start-time, never a `killpg` on a possibly-recycled pgid number.
- `s3_coldstart::{OnceArgs, coldstart_once, StopArgs, coldstart_stop}`: one cold-start attempt, self-healing a stale group first, launching the server, polling `GET /v1/models` for readiness while watching the log for `--backend-ready-marker`, recording `e2e_ready_s`/`backend_ready_s`/`frontend_tail_s`, then detaching. `coldstart_stop` samples whole-tree memory at ready (per-role-group via the existing `RoleMap`/`memory_at`), records it, then tears the group down.
- `cmdline::{shell_quote, shell_join}` and `s3_coldstart::{hyperfine_argv, HyperfineStats, parse_hyperfine_json}`: Phase 2's `hyperfine_argv`/`parse_hyperfine_json` ported field-by-field (RESEARCH Pattern 2), with POSIX shell quoting proven against a real `sh` including an injection token.
- `s3_coldstart::{S3Args, S3Runner}` and `rsg-bench s3`: a `Lifecycle::RunnerManaged` trial that wraps `hyperfine` around `coldstart-once`/`coldstart-stop`, excludes warm-up runs by index, computes boot-window GC per kept run when `--gc-hook on`, and reports `hyperfine`/`runs`/`means` separately from the frontend's own tail.
- `orchestrator::run_session`'s `RunnerManaged` branch: `build_trial_env` (extracted, shared with the harness-managed path for P1 parity), `run_one_trial_runner_managed` (creates the trial dir, renders argv, builds env, checks the port, calls `runner.run_trial` directly -- no `ServerHandle`, no `MemorySampler`, no hook read), and `run_one_trial_dispatch` (one `async fn` matching on `lifecycle` so `tokio::select!` needs no boxing).
- All three tasks' tests pass, including the hyperfine-gated `s3_session_with_hyperfine_and_stub_arms` (hyperfine 1.20.0 installed and run on this Mac, not skipped); `cargo test -p rsg-bench` (default, that one test ignored) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` are both clean; `cargo test --workspace` and `cargo build --workspace` are unaffected.

## Task Commits
1. **Task 1: Tracer -- coldstart-once/coldstart-stop time readiness, the backend-ready marker and detached-group teardown** - `474f175` (feat)
2. **Task 2a: RED -- failing tests for hyperfine argv/export parsing and POSIX shell quoting** - `2718076` (test)
2. **Task 2b: GREEN -- implement shell_quote/shell_join and hyperfine_argv/parse_hyperfine_json** - `ab7c8fe` (feat)
3. **Task 3a: RED -- failing test for the orchestrator's RunnerManaged lifecycle branch, plus rsg-bench s3** - `525ccd5` (test)
3. **Task 3b: GREEN -- add the RunnerManaged branch to orchestrator::run_session** - `97a7d1d` (feat)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/src/scenarios/s3_coldstart.rs` - `ColdstartRecord`, `OnceArgs`/`coldstart_once`, `StopArgs`/`coldstart_stop`, `hyperfine_argv`, `HyperfineStats`/`parse_hyperfine_json`, `S3Args`/`S3Runner`
- `crates/rsg-bench/src/procs.rs` - `DetachedGroup`, `ServerHandle::detach`, `process_start_time`, `stop_detached`, `group_members_matching`, `signal_matching_members`
- `crates/rsg-bench/src/cmdline.rs` - `shell_quote`, `shell_join`
- `crates/rsg-bench/src/orchestrator.rs` - `build_trial_env`, `run_one_trial_runner_managed`, `run_one_trial_dispatch`, `run_session`'s per-slot dispatch
- `crates/rsg-bench/src/main.rs` - `s3`, `coldstart-once` (hidden), `coldstart-stop` (hidden) subcommands
- `crates/rsg-bench/src/scenarios/mod.rs` - added `pub mod s3_coldstart;`
- `crates/rsg-bench/src/memory.rs` - `MemPoint` gained `Deserialize`
- `crates/rsg-bench/src/bin/bench-stub.rs` - `--marker-after-ms` now relative to process start, not listener-bind time
- `crates/rsg-bench/tests/coldstart.rs` - 3 tests: once/stop round trip, null-marker, self-heal
- `crates/rsg-bench/tests/hyperfine_parse.rs` - 5 tests: fixture parse, missing-results errors, null stddev, shell-quote round trip, argv shape
- `crates/rsg-bench/tests/s3_coldstart_e2e.rs` - 2 tests: runner-managed-skips-harness-launch, hyperfine-gated full session
- `crates/rsg-bench/tests/fixtures/hyperfine_export.json` - Phase 2's own `baseline-profile.json` numbers, reused as the parse fixture

## Decisions Made
See `key-decisions` in the frontmatter above.

## TDD Gate Compliance

`workflow.tdd_mode` is off for this project run, so the orchestrator-level RED-commit hard gate did not apply, but the full RED-GREEN-REFACTOR discipline was followed for both TDD tasks.

**Task 2 (`hyperfine_argv`/`parse_hyperfine_json`/`shell_quote`/`shell_join`):** `tests/hyperfine_parse.rs` and its fixture were written first, against intentionally-wrong stubs: `shell_quote` returned the token unchanged (never quoting), `shell_join` joined with `,` instead of a space, `hyperfine_argv` always returned an empty `Vec`, and `parse_hyperfine_json` always returned a zeroed, empty-`times` stats struct without ever checking for a missing/empty `results`. `cargo test -p rsg-bench --test hyperfine_parse` confirmed a genuine RED: all 5 tests failed on real assertions (wrong quoting, a broken `sh` round trip from comma-joining, an empty argv, wrong numeric values, a missing expected error) -- never a compile error. That RED state was committed (`2718076`). The correct implementations were then written; 4 of 5 tests passed immediately, with `parse_phase2_shape` initially failing on an unrelated floating-point precision mismatch (documented as a deviation below) before reaching GREEN (`ab7c8fe`). No refactor commit was needed.

**Task 3 (orchestrator `RunnerManaged` branch):** `tests/s3_coldstart_e2e.rs` was written first, with `S3Args`/`S3Runner`/the `rsg-bench s3` subcommand wired up in their final, correct form (they are integration glue over Task 1/2's already-complete and tested primitives, not the behavior under test here) alongside an *unmodified* `orchestrator::run_session` that still unconditionally harness-launched every trial. Running the suite confirmed a genuine RED: `runner_managed_skips_harness_launch`'s fake `RunnerManaged` runner (over a nonexistent-binary arm template) was actually harness-launched as a real server, so its trial came back `Failed` instead of `Ok` (`failed_trials` `1 != 0`) -- a real assertion failure, not a compile error. The hyperfine-gated test stayed `#[ignore]`d during this RED pass (hyperfine was not yet installed) and was not part of the RED/GREEN cycle's own evidence. That RED state was committed (`525ccd5`). `build_trial_env`/`run_one_trial_runner_managed`/`run_one_trial_dispatch` were then implemented; both tests passed (GREEN, `97a7d1d`), with `run_one_trial_runner_managed` needing one `#[allow(clippy::too_many_arguments)]` fix caught by the same gate run (documented below). No refactor commit was needed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `bench-stub`'s `--marker-after-ms` was relative to listener-bind time, not process start**
- **Found during:** Task 1, writing `coldstart_once_then_stop_with_stub`
- **Issue:** `bench-stub.rs` (07-01) spawned the marker-emission task *after* the listener bound, so with `--ready-delay-ms 300 --marker-after-ms 100` the marker fired at `300 + 100 = 400ms`, strictly after the ~300ms readiness point. This plan's own test oracle requires `backend_ready_s` ≤ `e2e_ready_s` for exactly this configuration, which the prior ordering could never produce.
- **Fix:** Moved the marker-emission `tokio::spawn` before the `--ready-delay-ms` sleep, so both flags are now relative to the same clock (process start).
- **Files modified:** `crates/rsg-bench/src/bin/bench-stub.rs`
- **Verification:** `crates/rsg-bench/tests/coldstart.rs#coldstart_once_then_stop_with_stub` passes with `backend_ready_s` ≈ 0.32s and `e2e_ready_s` ≈ 0.52s for `--ready-delay-ms 300 --marker-after-ms 100`. No other test in the suite referenced either flag before this plan.
- **Commit:** `474f175`

**2. [Rule 1 - Bug] `ColdstartRecord`'s planned `#[serde(tag = "kind")]` derive silently failed to deserialize `roles: BTreeMap<i32, Role>`**
- **Found during:** Task 1, running `coldstart_once_then_stop_with_stub` for the first time
- **Issue:** The `Mem` variant's `roles: BTreeMap<i32, Role>` field, nested inside a `#[serde(tag = "kind")]`-derived enum, failed to deserialize with `invalid type: string "67312", expected i32` even though the JSON key is a normal JSON object key (always a string) and the exact same field type round-trips fine as a plain struct field elsewhere (`TrialRecord.roles` in `manifest.rs`). serde's internally-tagged-enum deserialization re-decodes the matched variant from a buffered `Content` value rather than the original deserializer, and that buffered path does not correctly handle this non-string map key. The failure was silent in the record-reading code's own `filter_map(|l| serde_json::from_str(l.trim()).ok())`, which is why `coldstart-stop`'s mem record appeared "missing" even though the subprocess had written it successfully.
- **Fix:** Replaced the `#[serde(tag = "kind")]` derive with hand-rolled `Serialize`/`Deserialize` impls that serialize to a plain map with an explicit `"kind"` key, and deserialize by first decoding to `serde_json::Value`, reading `"kind"`, then `serde_json::from_value::<ReadyFields|MemFields>` on the whole value (ignoring the extra `"kind"` key, since serde ignores unknown fields by default). This uses the complete value deserializer instead of the buffered `Content` one.
- **Files modified:** `crates/rsg-bench/src/scenarios/s3_coldstart.rs`
- **Verification:** All three `coldstart.rs` tests pass; the record-file round trip was manually traced (raw file contents inspected, parse error observed, then confirmed resolved) before committing.
- **Commit:** `474f175`

**3. [Rule 1 - Bug] `hyperfine_parse.rs`'s `parse_phase2_shape` failed on bit-exact float equality**
- **Found during:** Task 2 GREEN, first run of `cargo test --test hyperfine_parse` against the correct implementation
- **Issue:** `serde_json::from_str::<f64>("14.934000985119999")` (the fixture, parsed by the harness's own code) and the Rust literal `14.934000985119999` (in the test) are not bit-identical: `serde_json`'s default float parser (the `float_roundtrip` cargo feature is not enabled) is not always correctly rounded at 17 significant digits, differing from Rust's own literal parser by ~1e-12 relative.
- **Fix:** Changed the five numeric assertions (and the `times_s` vector) in `parse_phase2_shape` to an epsilon comparison (`1e-9` absolute), rather than enabling `serde_json`'s `float_roundtrip` feature workspace-wide (out of this plan's scope for a sub-nanosecond-equivalent timing difference that no benchmark consumer could observe).
- **Files modified:** `crates/rsg-bench/tests/hyperfine_parse.rs`
- **Verification:** `parse_phase2_shape` passes; the other four tests in the file (which don't carry 17-significant-digit literals) were unaffected.
- **Commit:** `ab7c8fe`

**4. [Rule 3 - Blocking issue] `MemPoint` needed `Deserialize`, not just `Serialize`**
- **Found during:** Task 1, designing `ColdstartRecord::Mem`'s `groups`/`tree` fields
- **Issue:** `memory::MemPoint` (07-07-era, used for "memory at one instant") only derived `Serialize`. `ColdstartRecord` needs to deserialize it back from the JSONL record file inside `S3Runner::run_trial`.
- **Fix:** Added `Deserialize` alongside the existing `Serialize` derive. No field types or serialization shape changed -- the same pattern 07-07's `OutcomeCounts`/`Percentiles`/`LatencySummary` already established.
- **Files modified:** `crates/rsg-bench/src/memory.rs`
- **Verification:** `cargo build -p rsg-bench` and the full test suite pass.
- **Commit:** `474f175`

**5. [Rule 1 - Bug] `run_one_trial_runner_managed` tripped `clippy::too_many_arguments`**
- **Found during:** Task 3 GREEN, running `cargo clippy --all-targets -- -D warnings`
- **Issue:** The function's 9 parameters (mirroring `run_one_trial`'s own 9, which already carries `#[allow(clippy::too_many_arguments)]`) tripped the same lint.
- **Fix:** Added the matching `#[allow(clippy::too_many_arguments)]`.
- **Files modified:** `crates/rsg-bench/src/orchestrator.rs`
- **Verification:** `cargo clippy -p rsg-bench --all-targets -- -D warnings` clean.
- **Commit:** `97a7d1d`

**Total deviations:** 5 auto-fixed (3 Rule 1 bugs, 1 Rule 1 test-precision fix, 1 Rule 3 blocking issue). **Impact:** None change the plan's scope or design; the bench-stub and `ColdstartRecord` fixes were necessary for the plan's own stated test oracle to be satisfiable at all, and the float-precision and clippy fixes were needed for a clean, passing gate.

## Issues Encountered

**`hyperfine` was not installed on this Mac dev machine.** Per the plan's own `<verify>` step and T-07-SC's pre-approved disposition (RESEARCH Package Legitimacy Audit: sharkdp/hyperfine, crates.io, 8 years old), `cargo install hyperfine --version 1.20.0 --locked` was run to install it, so `s3_coldstart_e2e.rs`'s hyperfine-gated test could actually run (not just compile) before this plan was marked complete. `hyperfine --version` reports `1.20.0`. This is documented here rather than under "User Setup Required" because the install was already pre-vetted during planning, not a fresh decision made during execution.

No other issues beyond the deviations above. One pre-existing, unrelated test flake was observed and is **not** attributed to this plan: `loadgen_cancel.rs#loadgen_counts_match_server` failed once with "port already in use" when run as part of the full `cargo test -p rsg-bench` suite (a `free_port()` race across test binaries sharing the ephemeral port range), and passed cleanly when re-run in isolation (`cargo test -p rsg-bench --test loadgen_cancel`). This file was not touched by this plan.

## User Setup Required
None beyond the `hyperfine` install already covered above (and already performed during this execution, not left as a manual step).

## Next Phase Readiness
`rsg_bench::scenarios::s3_coldstart` is complete and tested against `bench-stub` arms on the Mac, including the real hyperfine-timed path. `BENCH-05` stays open in `REQUIREMENTS.md` per the shared-ID gate (`07-10`, the GPU-box wrapper/report plan, also declares it and has not yet run). The 07-09/07-10 report plans can read `result.hyperfine`/`result.runs`/`result.means` directly from the manifest via the `s3_coldstart` scenario name this plan's interface contract fixes. `orchestrator::run_session`'s `RunnerManaged` branch is now proven end-to-end and available to any future scenario that needs to own its own server lifecycle. No blockers.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-06*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/scenarios/s3_coldstart.rs
- FOUND: crates/rsg-bench/tests/coldstart.rs
- FOUND: crates/rsg-bench/tests/hyperfine_parse.rs
- FOUND: crates/rsg-bench/tests/s3_coldstart_e2e.rs
- FOUND: crates/rsg-bench/tests/fixtures/hyperfine_export.json
- FOUND: crates/rsg-bench/src/procs.rs
- FOUND: crates/rsg-bench/src/cmdline.rs
- FOUND: crates/rsg-bench/src/orchestrator.rs
- FOUND: crates/rsg-bench/src/main.rs
- FOUND: crates/rsg-bench/src/scenarios/mod.rs
- FOUND: crates/rsg-bench/src/memory.rs
- FOUND: crates/rsg-bench/src/bin/bench-stub.rs
- FOUND: 474f175 (feat: tracer -- coldstart-once/coldstart-stop)
- FOUND: 2718076 (test: RED -- hyperfine/shell-quote tests)
- FOUND: ab7c8fe (feat: GREEN -- hyperfine_argv/parse_hyperfine_json/shell_quote/shell_join)
- FOUND: 525ccd5 (test: RED -- RunnerManaged lifecycle test, plus rsg-bench s3)
- FOUND: 97a7d1d (feat: GREEN -- RunnerManaged branch in orchestrator::run_session)
- No unexpected file deletions in any of the five task commits
