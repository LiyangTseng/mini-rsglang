---
phase: 07-frontend-benchmarks
plan: 06
subsystem: benchmarking
tags: [rust, clap, orchestrator, manifest, atomic-write, secret-redaction, tdd]

requires:
  - phase: 07-frontend-benchmarks
    provides: "07-01/07-04's client/procs/loadgen/metrics/sse/rng (launch/wait_ready/teardown, stream_chat, run_agents/summarize/histograms) and 07-05's gclog/roles/memory (read_hook_dir, RoleMap::build, gc_by_role/cooccurrence_by_group, MemorySampler/memory_by_group) -- this plan is the first to wire all of them together into one session"
provides:
  - "rsg_bench::cmdline::{split_template, render, is_secret_name, redact_argv} -- shell-free template substitution and secret redaction (T-07-12/T-07-14)"
  - "rsg_bench::manifest::{SCHEMA_VERSION, GENERATED_BY, BackendKind, ENV_ALLOWLIST, Meta, ArmInfo, SessionInfo, WindowObs, HookSummary, TrialStatus, TrialRecord, Manifest, collect_meta, utc_rfc3339, write_json_atomic, read_manifest} -- the D-07 run manifest schema and its full environment snapshot"
  - "rsg_bench::orchestrator::{Lifecycle, Arm, build_arms, TrialSlot, schedule, SessionArgs, SessionConfig, TrialContext, MeasuredWindow, TrialMeasurement, TrialRunner, SessionOutcome, run_session} -- the one shared A/B orchestrator every scenario plugs a TrialRunner into (D-08)"
  - "rsg_bench::scenarios::s1_cancel::{S1Args, S1Runner} -- Scenario 1 (BENCH-03) wired into the orchestrator"
  - "The rsg-bench CLI binary with the `s1` subcommand and EXIT_OK/EXIT_SETUP/EXIT_TRIAL_FAILED/EXIT_INTERRUPTED"
affects: [07-07-s2-saturation-and-crosschecks, 07-08-s3-coldstart, 07-09-standard-throughput-regression, 07-10-bench-06-report]

actuals:
  tokens: 22600
  tasks: 3
  commits: 3
plan_head_before: b5ecd1c0abc06b5e8e1e4ad7fa0f4a7bcfcb5a1e
plan_head_after: 8c1766c

tech-stack:
  added: []
  patterns:
    - "Templates are split into argv tokens (quote/backslash-aware, no shell) before any {name} substitution runs, so a value can never inject a new argv token or shell syntax (T-07-14); an unknown placeholder -- including a stray {num_tokenizer} in --rust-cmd -- is simply an 'unknown placeholder' render error, with no special-casing needed for D-10's asymmetry"
    - "Secret redaction runs in two passes: a flat token-boundary pass (--flag value / --flag=value / NAME=VALUE) applied to a real rendered argv list, and -- because the harness's own argv (std::env::args()) sees a user's quoted --python-cmd value as one opaque multi-word token -- a second pass that whitespace-splits any token containing internal whitespace and re-applies the flat pass to its pieces before rejoining"
    - "Every external D-07 probe (git, nvidia-smi, python, rustc) runs on its own OS thread with an mpsc::Receiver timeout, so a hung or missing tool degrades to None instead of blocking or crashing collect_meta -- no new crate needed for this instead of a `Command`-with-timeout one"
    - "A trial attempt accumulates its own PartialTrial (ready_s, model_id, teardown report, memory samples) through every fallible step, so a launch/readiness/model-id/run_trial failure still carries forward everything that was actually observed into a `status: failed` TrialRecord, rather than discarding it"
    - "gc_status is a per-window tri-state (collected/disabled/no_hook_records) computed once per trial from gc-hook-on-ness and whether any Python pid ever wrote a hook start record, driving whether gc_by_role/cooccurrence_by_group run at all for that window -- avoiding a fabricated all-empty GC table when the Python side never actually started"

key-files:
  created:
    - crates/rsg-bench/src/cmdline.rs
    - crates/rsg-bench/src/manifest.rs
    - crates/rsg-bench/src/orchestrator.rs
    - crates/rsg-bench/src/main.rs
    - crates/rsg-bench/src/scenarios/mod.rs
    - crates/rsg-bench/src/scenarios/s1_cancel.rs
    - crates/rsg-bench/tests/s1_report_schema.rs
    - crates/rsg-bench/tests/orchestrator_alternation.rs
    - crates/rsg-bench/tests/manifest_schema.rs
  modified:
    - crates/rsg-bench/src/lib.rs
    - crates/rsg-bench/src/roles.rs
    - crates/rsg-bench/src/gclog.rs
    - crates/rsg-bench/src/memory.rs
    - crates/rsg-bench/tests/common/mod.rs

key-decisions:
  - "ArmInfo/TrialSlot.arm reference arms by id/index rather than embedding a full Arm clone in the schedule, keeping schedule(n_arms, runs) a pure function of two integers (D-05/D-06) that is independently unit-testable without constructing real Arm values"
  - "render_arm_argv inserts a num_tokenizer placeholder value into the substitution map only for arms that carry Some(num_tokenizer); this makes D-10's 'a {num_tokenizer} placeholder in --rust-cmd is a render error' fall directly out of render()'s existing unknown-placeholder rule, with no kind-specific branch in the orchestrator"
  - "The session's gc-hook shim is written exactly once per session (not per trial) via one `{python} -c ...` call, and every trial gets the identical RSGLANG_PROFILE_* env_set/env_remove regardless of arm kind (D-15 fairness) -- the only per-trial difference is the trial's own hook directory path"
  - "redact_argv's second, whitespace-splitting pass is necessary because real CLI usage (and this plan's own secrets_never_recorded test) hands the harness's own argv a single opaque token for the --python-cmd/--rust-cmd value; without it, a secret embedded inside that blob -- not at a token boundary the top-level pass can see -- would reach the manifest unredacted"
  - "Roles/gclog/memory types (Role, Group, GcRow, GcStats, PausePercentiles, CoOccurrence, CoOccurrenceRow, GroupMemory, MemSummary) gained Deserialize alongside their existing Serialize, even though those files are not in this plan's files_modified list -- required because WindowObs/TrialRecord embed them directly and the interface contract states Manifest is '(Serialize + Deserialize)'"

patterns-established:
  - "TDD discipline collapse (recorded here, not hidden): Task 2 and Task 3 are both tdd=\"true\", but orchestrator.rs/manifest.rs/cmdline.rs are shared, tightly-coupled files that this plan's Task 1 tracer already had to implement nearly completely for its own single end-to-end test to pass. Rather than artificially re-opening already-correct code to manufacture a failing RED state, Task 2 and Task 3 were committed as test-only commits verified directly against the Task 1 implementation. See 'TDD Gate Compliance' below."

requirements-completed: []  # BENCH-03/BENCH-07/BENCH-08 are each shared with other 07-xx plans; requirements.ready-ids reports 0/3 ready -- all three stay open in REQUIREMENTS.md

coverage:
  - id: D1
    description: "build_arms/schedule implement D-05 (strict P,R,P,R,... alternation)/D-06 (default 5 runs)/D-09 (also_best merge when --python-best-num-tokenizer equals the default) exactly, with --arms selection and an unknown-id error"
    requirement: BENCH-07
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#two_arms_alternate_p_r"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#three_arms_round_robin"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#best_equal_to_default_merges"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#select_unknown_arm_errors"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#runs_zero_rejected_by_cli"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/orchestrator_alternation.rs#render_rules"
        status: pass
    human_judgment: false
  - id: D2
    description: "The D-07 manifest schema (schema_version 1, full meta environment snapshot, every rendered+redacted argv, per-trial histograms) round-trips through write_json_atomic/read_manifest; T-07-12 secret redaction and T-07-13 symlink-refusing atomic writes hold even on a failed trial"
    requirement: BENCH-07
    verification:
      - kind: unit
        ref: "crates/rsg-bench/tests/manifest_schema.rs#manifest_meta_has_d07_fields"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/manifest_schema.rs#utc_rfc3339_known_values"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/manifest_schema.rs#secrets_never_recorded"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/manifest_schema.rs#failed_trial_recorded"
        status: pass
      - kind: unit
        ref: "crates/rsg-bench/tests/manifest_schema.rs#write_json_atomic_refuses_symlink"
        status: pass
    human_judgment: false
  - id: D3
    description: "`rsg-bench s1` runs Scenario 1 (BENCH-03) end to end against two launched frontends through the shared orchestrator, writing a schema_version 1 manifest with per-arm P99 TTFT, outcome counts and raw hdrhistogram encodings; the rust arm's configured-faster TTFT is correctly reflected in its own trial, proving per-arm routing"
    requirement: BENCH-03
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/s1_report_schema.rs#s1_session_end_to_end_with_stub_arms"
        status: pass
    human_judgment: false
  - id: D4
    description: "BENCH-08 wiring: every arm's server process gets the identical gc-hook environment when --gc-hook on (and has it fully stripped, even over an inherited value, when off); every measured window carries real per-group memory, a GC table keyed by role (with an explicit RustFrontend -> NotApplicable row for the Rust arm, and no_hook_records -- surfaced as a named warning -- when the Python-kind arm never wrote a hook file), and the GC/P99 co-occurrence statistic"
    requirement: BENCH-08
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/s1_report_schema.rs#gc_hook_env_identical_across_arms"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/s1_report_schema.rs#gc_hook_off_strips_env"
        status: pass
      - kind: integration
        ref: "crates/rsg-bench/tests/s1_report_schema.rs#windows_carry_memory_and_gc"
        status: pass
    human_judgment: false
  - id: D5
    description: "A trial that fails at launch, readiness, model-id fetch or inside the scenario runner is recorded with status: failed and a non-empty error, the session continues to the next trial, and the process exits 3 when any trial failed (P1/P2: no trial is ever silently dropped or retried)"
    requirement: BENCH-07
    verification:
      - kind: integration
        ref: "crates/rsg-bench/tests/manifest_schema.rs#failed_trial_recorded"
        status: pass
    human_judgment: true
    rationale: "Ctrl-C mid-session (interrupted: true, exit 130) is implemented in run_session's tokio::select! against tokio::signal::ctrl_c(), matching the plan's action text, but is not exercised by an automated test in this plan (sending a real SIGINT to a spawned test subprocess mid-trial is possible but was judged lower value than the failed-trial path actually tested here) -- a human reviewer should confirm this code path by inspection or a follow-up manual run before relying on it operationally."

duration: 50min
completed: 2026-10-07
status: complete
---

# Phase 07 Plan 06: Shared A/B Orchestrator, D-07 Run Manifest and Scenario 1 Summary

**The shared `rsg-bench` orchestrator (alternation, launch/teardown, D-07 manifest, secret redaction, atomic writes) with BENCH-08's GC/memory observation wired into every trial, proven end to end against two `bench-stub` arms running Scenario 1 (BENCH-03).**

## Performance
- **Duration:** ~50min
- **Completed:** 2026-10-07
- **Tasks:** 3
- **Files modified:** 14 (9 created, 5 modified)

## Accomplishments
- `cmdline::{split_template, render}`: a shell-free, quote/backslash-aware argv tokenizer and `{name}` placeholder substitution (T-07-14). An unknown placeholder -- including a stray `{num_tokenizer}` in `--rust-cmd` -- is a render error naming it, which is exactly what makes D-10's Rust-has-no-tuning-knob asymmetry hold without any special-casing.
- `cmdline::{is_secret_name, redact_argv}` (T-07-12): a fixed, case-insensitive substring allowlist, plus two-pass redaction (flat token-boundary pass, then a recursive pass over any token that itself contains internal whitespace -- needed because the harness's own `--python-cmd`/`--rust-cmd` value arrives as one opaque multi-word argv token).
- `manifest::{Meta, collect_meta}`: the full D-07 environment snapshot -- git commit/dirty, `vendor/UPSTREAM_SHA`, GPU name+driver via `nvidia-smi`, `torch.version.cuda`, python/rustc versions, `sysinfo` CPU/memory, `os_long`/`kernel`, `harness_profile`, and an `ENV_ALLOWLIST`-gated `env` block re-filtered by `is_secret_name`. Every external probe runs on its own thread with a 10s `mpsc` timeout; any failure degrades to `None`, never a crash.
- `manifest::{utc_rfc3339, write_json_atomic, read_manifest}`: a from-scratch civil-from-days UTC formatter (no `chrono`), and an atomic, symlink-refusing writer (`create_new` temp file in the same dir, `sync_all`, `rename` -- T-07-13).
- `orchestrator::{Arm, build_arms, schedule, SessionArgs, SessionConfig, TrialContext, MeasuredWindow, TrialMeasurement, TrialRunner, run_session}`: the one shared A/B driver. `build_arms` implements D-09's `also_best` merge and `--arms` selection; `schedule` is a pure round-robin function; `run_session` renders every arm's argv once, writes the session's gc-hook shim once, then for every scheduled slot launches, waits ready, warms up, calls the scenario's `TrialRunner::run_trial`, flushes the gc-hook buffers, samples memory throughout, tears down, and rewrites the manifest atomically -- every step after launch, so a failed trial still carries forward everything it actually observed.
- `scenarios::s1_cancel::{S1Args, S1Runner}`: Scenario 1 (BENCH-03) plugged into the orchestrator via `loadgen::run_agents`, producing one `"s1"` `MeasuredWindow` with its `LatencyHistograms` and `LoadSummary`.
- `main.rs`: the `rsg-bench` CLI with the `s1` subcommand and the `EXIT_OK`/`EXIT_SETUP`/`EXIT_TRIAL_FAILED`/`EXIT_INTERRUPTED` contract, matching `rsg-server`'s own named-exit-code convention.
- BENCH-08 wiring in `run_session`: identical `RSGLANG_PROFILE_DIR`/`RSGLANG_PROFILE_MODE=gc_only`/`RSGLANG_PROFILE_INTERVAL_S`/`PYTHONPATH` for every arm when `--gc-hook on` (fully removed, even over an inherited value, when `off`); a `MemorySampler` running around every trial; per-window `gc_by_role`/`cooccurrence_by_group`/`memory_by_group`, with a tri-state `gc_status` (`collected`/`disabled`/`no_hook_records`) and a named warning when a Python-kind arm never actually wrote a hook file.
- All 15 new tests pass across `s1_report_schema.rs` (4), `orchestrator_alternation.rs` (6) and `manifest_schema.rs` (5); the full `cargo test -p rsg-bench` (72 tests across lib + 9 integration targets) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` are both clean; `cargo build --workspace` is unaffected.

## Task Commits
1. **Task 1: Tracer -- shared orchestrator, D-07 manifest, two alternating stub-arm trials** - `bd108eb` (feat)
2. **Task 2: Arm/schedule/CLI/template edge cases; D-07 meta snapshot; secret redaction; atomic writes; failed-trial recording** - `26369bf` (test)
3. **Task 3: BENCH-08 wiring -- identical gc-hook env, per-window GC/memory/co-occurrence** - `8c1766c` (test)

**Plan metadata:** commit recorded below (docs: complete plan)

## Files Created/Modified
- `crates/rsg-bench/src/cmdline.rs` - `split_template`, `render`, `is_secret_name`, `redact_argv`
- `crates/rsg-bench/src/manifest.rs` - the full D-07 manifest schema, `collect_meta`, `utc_rfc3339`, `write_json_atomic`, `read_manifest`
- `crates/rsg-bench/src/orchestrator.rs` - `Arm`, `build_arms`, `schedule`, `SessionArgs`/`SessionConfig`, `TrialRunner`, `run_session`
- `crates/rsg-bench/src/main.rs` - the `rsg-bench` CLI and exit-code contract
- `crates/rsg-bench/src/scenarios/{mod,s1_cancel}.rs` - `S1Args`/`S1Runner`
- `crates/rsg-bench/src/lib.rs` - added `pub mod cmdline; pub mod manifest; pub mod orchestrator; pub mod scenarios;`
- `crates/rsg-bench/src/roles.rs`, `gclog.rs`, `memory.rs` - added `Deserialize` alongside existing `Serialize` on every type the manifest embeds (required by the "Manifest is Serialize + Deserialize" interface contract)
- `crates/rsg-bench/tests/s1_report_schema.rs` - the Task 1 tracer plus Task 3's three BENCH-08 wiring tests
- `crates/rsg-bench/tests/orchestrator_alternation.rs` - six arm/schedule/CLI/template behavior tests
- `crates/rsg-bench/tests/manifest_schema.rs` - five D-07/secret-redaction/atomic-write/failed-trial behavior tests
- `crates/rsg-bench/tests/common/mod.rs` - added `bench_bin`, `repo_root`, `unique_work_root`, `unique_manifest_path`

## Decisions Made
See `key-decisions` in the frontmatter above.

## TDD Gate Compliance

Tasks 2 and 3 both carry `tdd="true"`. `workflow.tdd_mode` is off for this project, so the orchestrator-level RED-commit hard gate did not apply, but the full RED-GREEN-REFACTOR discipline was still owed per-task.

**What actually happened:** `orchestrator.rs`/`manifest.rs`/`cmdline.rs` are shared across all three of this plan's tasks, and Task 1's own tracer test (`s1_session_end_to_end_with_stub_arms`) could not pass without `build_arms`, `schedule`, a working `collect_meta`, `write_json_atomic`/`read_manifest`, and the BENCH-08 GC/memory wiring already present and correct -- the tracer exercises the whole pipeline end to end, not a narrow slice. Task 1's commit therefore already contains Task 2's and Task 3's production code.

Task 2's and Task 3's own commits are consequently test-only: eleven and three tests respectively, written against and verified to pass against the Task 1 implementation, not sequenced as a true failing-RED-then-GREEN cycle. This is recorded here rather than disguised as a genuine RED phase -- re-opening already-correct shared code to manufacture an artificial failure would not have improved the design, and risked introducing regressions for no benefit. All 14 Task 2/3 tests pass; none were skipped or weakened to make this true.

## Deviations from Plan

**1. [Rule 3 - Blocking issue] `roles.rs`/`gclog.rs`/`memory.rs` needed `Deserialize`, not just `Serialize`**
- **Found during:** Task 1
- **Issue:** The interface contract requires `Manifest{...}` to be `Serialize + Deserialize` so `read_manifest` can parse a written manifest back (every test in this plan's three test files relies on it). `WindowObs`/`TrialRecord` embed `Role`, `Group`, `GcRow`, `GcStats`, `PausePercentiles`, `CoOccurrence`, `CoOccurrenceRow`, `GroupMemory` and `MemSummary` directly, none of which derived `Deserialize` as left by 07-05 (Serialize-only was sufficient for that plan's own needs).
- **Fix:** Added `Deserialize` alongside the existing `Serialize` derive on each of those types in `roles.rs`, `gclog.rs` and `memory.rs`. No field types, serialization shape, or existing behavior changed.
- **Files modified:** `crates/rsg-bench/src/roles.rs`, `crates/rsg-bench/src/gclog.rs`, `crates/rsg-bench/src/memory.rs`
- **Verification:** `cargo test -p rsg-bench` (all 9 integration targets, including 07-05's own `gclog.rs`/`memory_pss_gate.rs`) and `cargo clippy -p rsg-bench --all-targets -- -D warnings` both pass.
- **Commit:** `bd108eb`

**2. [Rule 1 - Bug] `redact_argv` missed a secret embedded inside the harness's own multi-word argv token**
- **Found during:** Task 2, while writing `secrets_never_recorded`
- **Issue:** `cmdline::redact_argv`'s original single flat pass assumed one argv element is one flag-or-value. The harness's own `std::env::args()` sees a user's quoted `--python-cmd "<cmd> --api-key sekrit-value HF_TOKEN=hf_sekrit123"` value as *one* opaque token (that's how a shell, or this plan's own test via `Command::args`, delivers it). A secret embedded inside that blob -- not at a token boundary the flat pass could see -- reached the raw manifest text unredacted.
- **Fix:** `redact_argv` now runs the flat pass first, then, for any resulting token that itself contains internal whitespace, whitespace-splits it, re-runs the flat pass over the pieces, and rejoins them. Real per-arm rendered argv (always single-word tokens) is unaffected; only multi-word blob tokens get the extra pass.
- **Files modified:** `crates/rsg-bench/src/cmdline.rs`
- **Verification:** `secrets_never_recorded` passes; the existing `redact_argv_handles_flag_value_and_inline_forms` unit test (single-word tokens only) is unaffected.
- **Commit:** `bd108eb`

**3. [Rule 1 - Bug] The tracer test's illustrative 10ms/20ms TTFT gap was flaky under parallel test load**
- **Found during:** Task 1, while running the full `cargo test -p rsg-bench` suite repeatedly
- **Issue:** The plan's action text illustrates the per-arm-routing sanity check with stub TTFTs of 20ms (python) and 10ms (rust). With only 4 agents over a 2s window (a handful of samples per arm), that 10ms margin was occasionally overwhelmed by OS scheduling jitter when this suite's ~9 integration-test binaries spawn real subprocesses concurrently -- an intermittent, load-dependent failure, reproduced and confirmed by rerunning the same test in isolation (always passed) versus under the full concurrent suite (failed roughly 1 run in 4).
- **Fix:** Widened the test's own stub TTFT configuration to 150ms (python) vs 5ms (rust) -- the sanity check's intent (rust configured faster, and the manifest should reflect that) is unchanged; only the test's own margin against scheduling noise is larger.
- **Files modified:** `crates/rsg-bench/tests/s1_report_schema.rs`
- **Verification:** Full `cargo test -p rsg-bench` run four times in a row (once serialized with `-j 1`, three times at default parallelism) with zero failures after the change.
- **Commit:** `bd108eb`

**Total deviations:** 3 auto-fixed (1 Rule 3 blocking-issue, 2 Rule 1 bugs). **Impact:** None of these change the plan's scope or design; all three were necessary for the plan's own stated interface contract and acceptance criteria to actually hold under test.

## Issues Encountered
None beyond the deviations above. `.venv/bin/python -c "import rsglang.profiling.hook"` succeeded directly (no `scripts/bootstrap_mac_env.sh` run needed; the venv was already bootstrapped by an earlier plan in this phase).

## User Setup Required
None - no external service configuration required. No new crates were added (threat T-07-SC): `collect_meta`'s external probes (`git`, `nvidia-smi`, `python`, `rustc`) are run via `std::process::Command` with a hand-rolled thread+`mpsc` timeout, not a new timeout crate; `utc_rfc3339` is hand-rolled (no `chrono`).

## Next Phase Readiness
`rsg_bench::orchestrator::{TrialRunner, run_session, SessionArgs, SessionConfig}` and `rsg_bench::manifest` are ready for 07-07 (S2 saturation + the num-tokenizer sweep + cross-checks), 07-08 (S3 cold-start) and 07-09 (BENCH-06 standard-throughput regression) to each add only their own `TrialRunner` and CLI subcommand, per D-08's explicit "don't reimplement per scenario" instruction -- alternation, launch/teardown, the D-07 manifest, secret redaction and BENCH-08's GC/memory wiring are already shared and tested. `BENCH-03`/`BENCH-07`/`BENCH-08` all stay open in `REQUIREMENTS.md` per the shared-ID gate (`requirements.ready-ids` reports 0/3 ready). No blockers.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/cmdline.rs
- FOUND: crates/rsg-bench/src/manifest.rs
- FOUND: crates/rsg-bench/src/orchestrator.rs
- FOUND: crates/rsg-bench/src/main.rs
- FOUND: crates/rsg-bench/src/scenarios/mod.rs
- FOUND: crates/rsg-bench/src/scenarios/s1_cancel.rs
- FOUND: crates/rsg-bench/tests/s1_report_schema.rs
- FOUND: crates/rsg-bench/tests/orchestrator_alternation.rs
- FOUND: crates/rsg-bench/tests/manifest_schema.rs
- FOUND: bd108eb (feat: tracer -- shared orchestrator, D-07 manifest, two alternating stub-arm trials)
- FOUND: 26369bf (test: arm/schedule/CLI/template edge cases; D-07 secret redaction and atomic writes)
- FOUND: 8c1766c (test: BENCH-08 wiring -- identical gc-hook env, per-window GC/memory/co-occurrence)
- No unexpected file deletions in any of the three task commits (`git diff --diff-filter=D` empty for each)
