---
phase: 07-frontend-benchmarks
plan: 10
subsystem: benchmarking
tags: [rust, bash, launcher, gpu-wrapper, D-03, BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08]

requires:
  - phase: 07-09
    provides: "rsg_bench::report::{Report, build_report, render_markdown, run_report} and rsg_bench::scenarios::standard_throughput, the full subcommand surface (s1, s2, sweep-num-tokenizer, crosscheck, s3, throughput, report) gpu_phase7_bench.sh drives"
  - phase: 07-01
    provides: "crates/rsg-bench crate layout and bin/clap conventions rsg-mock-stack follows"
provides:
  - "rsg-mock-stack: a Mac launcher adapter binary (crates/rsg-bench/src/bin/rsg-mock-stack.rs) that brings up mock-scheduler + rsg-server, relays the handshake, and tears both down on a signal or either child's exit (D-03, T-07-24)"
  - "scripts/gpu_phase7_bench.sh: the one GPU-box command that sequences preflight, sweep-num-tokenizer, s1, s2, the optional vllm/sglang cross-checks, s3, throughput, report and check_upstream.py, with a tested --dry-run"
affects: ["07-10 Task 3 (D-03 Mac dev pass + the phase gate) once Phase 5's HTTP API lands in this checkout", "the end-of-phase GPU-box human check that depends on this wrapper"]

actuals:
  tokens: 10740
  tasks: 2
  commits: 2
plan_head_before: 934a3f0c293e1e72d7ac6afc1c536e3f2d847fa9
plan_head_after: ee85b47a0c0e07de7bbd9a5e7f1ab0f41a1f33e1

tech-stack:
  added: []
  patterns:
    - "rsg-mock-stack uses tokio::process::Command (not std::process + a manual wait thread) for both children, so the SIGINT/SIGTERM-or-child-exit race is one tokio::select! rather than hand-rolled polling -- the only bin in rsg-bench so far to need async subprocess supervision of two concurrent children"
    - "gpu_phase7_bench.sh's --dry-run branch is hand-written (not synthesized from the real-run argv arrays via printf %q), so every printed RUN: line's wording is fully under the script author's control and trivially substring-matchable by pytest, at the cost of keeping two copies of each command's flag list in sync"

key-files:
  created:
    - crates/rsg-bench/src/bin/rsg-mock-stack.rs
    - crates/rsg-bench/tests/mock_stack.rs
    - scripts/gpu_phase7_bench.sh
    - python/tests/test_gpu_bench_script.py
    - .planning/phases/07-frontend-benchmarks/deferred-items.md
  modified:
    - crates/rsg-bench/Cargo.toml

key-decisions:
  - "rsg-mock-stack's two children are spawned with no process_group override, so they inherit rsg-mock-stack's own process group -- when the harness wraps rsg-mock-stack itself in a fresh group via rsg_bench::procs::launch, that same group already covers both children and no special-casing is needed in procs.rs (T-07-24)"
  - "gpu_phase7_bench.sh's cross-check env-var gate (VLLM_BIN / SGLANG_PYTHON) reuses clap's own --backend-kind default (Real) for the sweep step rather than passing --backend-kind real explicitly on that one line, since the <behavior> contract's test_dry_run_sequence requires the sweep RUN: line to NOT contain --backend-kind real -- the sweep's manifest still ends up backend_kind: real because that is SessionArgs' own clap default"
  - "The two sweep-only hardcoded values (--runs 1, no --python-best-num-tokenizer) are intentional per the plan's <action> spec: the sweep computes BEST itself and runs once per --num-tokenizer candidate, independent of the wrapper's own --runs value, which only applies to every scenario that already knows BEST"

patterns-established:
  - "A GPU-box wrapper script's preflight gate is a sequence of independent checks that each immediately record FAIL step preflight and exit 1 the moment one fails (mirroring scripts/gpu_phase2_profile.sh), never accumulating multiple preflight failures into one report -- a GPU box with a broken toolchain should fail fast, not run every check to completion first"

requirements-completed: []

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
    description: "D-03 Mac dev pass: scripts/bench_mac_devpass.sh drives rsg-server (backed by mock-scheduler) through s1/s2 (both modes)/s3, asserts completed/cancelled counts, the report's mock banner, and that a curl-aborted streaming request increments rsg-server's /metrics cancellation counter; followed by the phase gate (scripts/check_all.sh --offline)"
    requirement: BENCH-03
    verification: []
    human_judgment: true
    rationale: "Not built. Task 3's own <precondition> (Phase 5's HTTP API landed in this checkout) is unmet: `grep -rn \"v1/chat/completions\" crates/rsg-server/src` finds nothing, and `target/debug/rsg-server --help` lists no HTTP listen-port option. Per the executor's precondition-gate rule this halts before any Task 3 work, including the file write -- it is never auto-approved. A human (or a later continuation run once Phase 5 has landed in this checkout) must complete Task 3 and the phase gate before 07-10, and Phase 07 as a whole, can be marked done."
  - id: D4
    description: "The real GPU-box headline Python-vs-Rust comparison (BENCH-03..BENCH-08's actual measured numbers)"
    requirement: BENCH-06
    verification: []
    human_judgment: true
    rationale: "This is this plan's own end-of-phase human check (Task 2's <human-check>), explicitly deferred to the Linux CUDA GPU box after Phase 6 lands; it was never expected to run on this Mac dev machine and did not run here."

duration: 25min
completed: 2026-10-07
status: blocked
---

# Phase 07 Plan 10: GPU-Box Wrapper and Mac Launcher Adapter (Tasks 1-2 complete, Task 3 blocked) Summary

**Built and tested `rsg-mock-stack` (the Mac launcher adapter for the real Rust frontend against `mock-scheduler`) and `scripts/gpu_phase7_bench.sh` (the one-command GPU-box Python-vs-Rust wrapper, dry-run tested); Task 3's D-03 Mac dev pass and the phase gate are blocked because this checkout's `crates/rsg-server` does not yet have Phase 5's HTTP API.**

## Performance
- **Duration:** ~25min
- **Started:** 2026-10-07T06:42:40Z (approx, from STATE.md's prior session timestamp)
- **Completed:** 2026-10-07T07:05:28Z
- **Tasks:** 2 of 3 (Task 3 blocked before any work started)
- **Files modified:** 6 (5 created, 1 modified)

## Accomplishments
- `crates/rsg-bench/src/bin/rsg-mock-stack.rs`: spawns `mock-scheduler` (`--backend-role bind --detok-role connect`, passthrough delay/max-seq-len flags) and `rsg-server` (`--backend-role connect --detok-role bind --model --run-id`, plus each `--rsg-server-arg` with `{port}` substituted), reads `mock-scheduler`'s stdout handshake line within 20s, writes it verbatim to `rsg-server`'s stdin, forwards both children's stderr with `[rsg-server]`/`[mock-scheduler]` prefixes, prints the `children ...` and `backend ready; handshake sent to rsg-server` lines, and stops both children (SIGTERM, 5s grace, SIGKILL) on SIGINT/SIGTERM or either child's own exit, removing the two `ipc://` socket files. The two children inherit rsg-mock-stack's own process group, so the harness's own `killpg` teardown already covers them with no changes to `procs.rs` (T-07-24).
- `crates/rsg-bench/tests/mock_stack.rs`: `mock_stack_relays_handshake_and_stops` (handshake relay, clean SIGTERM stop, both child pids and ipc socket files gone) and `mock_stack_exits_when_child_dies` (SIGKILL `mock-scheduler` -> rsg-mock-stack exits 1, `rsg-server` also gone), both against the real `target/debug/rsg-server`/`mock-scheduler` binaries.
- `scripts/gpu_phase7_bench.sh`: preflight (`nvidia-smi -L`, `hyperfine >= 1.19`, the `minisgl`/`openai`/`transformers`/`rsglang` python imports, a release build -- any failure records `FAIL step preflight` and exits 1 immediately) then `sweep-num-tokenizer` (parses `best_num_tokenizer=` from its own stdout) -> `s1` -> `s2 --mode open` -> the optional `vllm`/`sglang` cross-checks (only when `VLLM_BIN`/`SGLANG_PYTHON` are set) -> `s3` -> `throughput` -> `report` -> `scripts/check_upstream.py`, each step recording a PASS/FAIL line. `BACKEND_EXTRA` applies to both launch templates; `RUST_CMD_EXTRA` only to the Rust one. `--dry-run` prints the exact `RUN: ` command sequence without building or running anything, verified to finish in well under 5s and to leave `docs/benchmarks` untouched.
- `python/tests/test_gpu_bench_script.py`: the six Mac-runnable behavior tests from the plan's `<behavior>` block -- `test_help`, `test_unknown_arg`, `test_dry_run_sequence` (command order, per-scenario flags, timing, git-clean assertion), `test_dry_run_crosschecks_optional`, `test_extra_args_placement`, `test_preflight_fails_without_gpu`.
- `.planning/phases/07-frontend-benchmarks/deferred-items.md`: logs two pre-existing, unrelated Phase 2 pytest failures found while running the phase gate (see Issues Encountered).
- `cargo test --workspace` passed in full (including the two new `mock_stack` tests), and `cargo clippy -p rsg-bench --all-targets -- -D warnings` is clean.

## Task Commits
1. **Task 1: Tracer -- rsg-mock-stack brings up mock-scheduler and rsg-server with the handshake relayed, then stops both cleanly** - `44137d6` (feat)
2. **Task 2: GPU-box wrapper scripts/gpu_phase7_bench.sh with dry-run tests** - `ee85b47` (feat)

**Task 3: D-03 Mac dev pass against rsg-server + mock-scheduler, and the phase gate** - NOT STARTED (precondition unmet; see Deviations / Next Phase Readiness)

**Plan metadata:** pending (this commit, docs: pause plan -- blocked, not complete)

## Files Created/Modified
- `crates/rsg-bench/src/bin/rsg-mock-stack.rs` - Mac launcher adapter: mock-scheduler + rsg-server, handshake relay, signal/child-exit teardown
- `crates/rsg-bench/tests/mock_stack.rs` - the two Task 1 integration tests, against the real rsg-server/mock-scheduler binaries
- `crates/rsg-bench/Cargo.toml` - added tokio's `process` feature (needed by `tokio::process`)
- `scripts/gpu_phase7_bench.sh` - the one GPU-box command: preflight, sweep, s1/s2/crosscheck/s3/throughput, report, check_upstream.py
- `python/tests/test_gpu_bench_script.py` - the six Task 2 behavior tests
- `.planning/phases/07-frontend-benchmarks/deferred-items.md` - two pre-existing, unrelated Phase 2 pytest failures found while running the phase gate

## Decisions Made
See `key-decisions` in frontmatter: no `process_group` override on rsg-mock-stack's children (T-07-24); the sweep step relying on clap's own `--backend-kind` default instead of passing it explicitly, to satisfy the plan's own dry-run behavior contract; the sweep's hardcoded `--runs 1` and absent `--python-best-num-tokenizer` being intentional, not oversights.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `FAILED` flag never set on a preflight failure, so the summary printed "ALL PASS" / exit 0 after a `FAIL step preflight` line**
- **Found during:** Task 2, manual verification of the preflight-failure path (`PATH` with no `nvidia-smi`) before writing the pytest suite
- **Issue:** Each of the four preflight failure branches called `record ... FAIL ...` then jumped straight to `print_summary_and_exit`, but never set `FAILED=1` first, so the summary's `[ "$FAILED" = 0 ]` check still saw `0` and printed `ALL PASS` with exit 0 -- a wrapper that reports success despite a failed GPU preflight.
- **Fix:** Added `FAILED=1` immediately before each of the four `print_summary_and_exit` calls in the preflight section.
- **Files modified:** `scripts/gpu_phase7_bench.sh`
- **Verification:** Manually re-ran with a `PATH` lacking `nvidia-smi`; confirmed `SOME STEPS FAILED` / exit 1. Then covered by `test_preflight_fails_without_gpu`.
- **Commit:** `ee85b47`

**Total deviations:** 1 auto-fixed (Rule 1 bug, caught before commit). **Impact:** Caught during manual verification before the pytest suite was written; no behavior-incorrect code was committed.

### Not Auto-Fixed: Task 3 Precondition Unmet (halt, not a deviation)

Task 3 carries an explicit `<precondition>`: "Phase 5 has landed in this checkout: `grep -rn \"v1/chat/completions\" crates/rsg-server/src` matches and `target/debug/rsg-server --help` (after `cargo build -p rsg-server --bins`) lists an HTTP listen-port option and a /metrics endpoint exists in crates/rsg-server/src". Checked at the start of Task 3, before any file was read or written for it:

```
grep -rn "v1/chat/completions" crates/rsg-server/src   -> no matches (exit 1)
target/debug/rsg-server --help                          -> lists only the Phase 1/3 skeleton
                                                            flags (--backend-addr/-role,
                                                            --detok-addr/-role, --model,
                                                            --run-id); no HTTP listen-port
                                                            option, no /metrics
```

The precondition is unmet. Per the executor's precondition-gate rule, this halts Task 3 entirely -- no partial commit, and the halt is never auto-approved even in the project's `auto_advance`-style modes. The plan's own `07-10-PLAN.md` objective section anticipated this exact scenario: "Phase 5 is being built in a parallel worktree, and ROADMAP already orders Phase 7 after Phases 5-6."

This is not a deviation from the plan (nothing was fixed or worked around) -- it is the documented halt-and-report path, surfaced below as a checkpoint.

## Issues Encountered

Running `scripts/check_all.sh --offline` (attempted after Tasks 1-2, to confirm they introduce no regressions ahead of Task 3) surfaced two **pre-existing, unrelated** `pytest python/tests` failures, both from Phase 2 and neither touching any file this plan modified:
- `test_baseline_profile.py::test_run_s3_hyperfine_missing_exits_2`
- `test_gpu_profile_script.py::test_hyperfine_ok`

Root cause (see `deferred-items.md` for detail): this Mac now has a real `hyperfine` reachable on `PATH` outside each test's stub directory, which these two "simulate hyperfine missing" tests did not anticipate. `cargo test --workspace` (step 1/5 of the phase gate) passed in full, including the new `mock_stack` tests; the pytest step (2/5) is where `check_all.sh` currently stops short of `check_all: OK`, for reasons unrelated to 07-10. Logged to `deferred-items.md` and the `.planning/WINDOWS.md` ledger (kind: deviation), not fixed (out of scope).

## User Setup Required

None yet for Tasks 1-2. Task 3, once unblocked, needs no additional user setup beyond what Phase 5 itself requires in this checkout.

## CHECKPOINT: Task 3 blocked on an unmet precondition

**Type:** human-verify
**Gate:** blocking-human
**Blocked by:** Precondition not met: "Phase 5 has landed in this checkout: `grep -rn \"v1/chat/completions\" crates/rsg-server/src` matches and `target/debug/rsg-server --help` ... lists an HTTP listen-port option and a /metrics endpoint exists in crates/rsg-server/src"

### What this means

Tasks 1 and 2 of this plan are complete, tested, and committed (`44137d6`, `ee85b47`). Task 3 -- the D-03 Mac dev pass (`scripts/bench_mac_devpass.sh`) and the Phase 07 gate -- cannot be written or run in this checkout because `crates/rsg-server` here is still the Phase 1/3 skeleton (stdin handshake only; no HTTP server, no `/metrics`). The plan's own dependency note anticipated this: Phase 5 is being built in a parallel worktree.

### How to resume

1. Merge/land Phase 5's HTTP API into this checkout (or re-run this plan from a checkout where Phase 5 has already landed).
2. Re-verify the precondition: `grep -rn "v1/chat/completions" crates/rsg-server/src` should match, and `cargo build -p rsg-server --bins && target/debug/rsg-server --help` should list an HTTP listen-port option.
3. Resume 07-10 at Task 3 (a continuation agent, or `/gsd-execute-phase` against this plan again) to write `scripts/bench_mac_devpass.sh`, run it, run `scripts/check_all.sh --offline`, and complete the plan's close-out (SUMMARY status -> complete, STATE.md/ROADMAP.md advance, requirements mark-complete for the shared BENCH-02..BENCH-08 IDs once every plan declaring them has finished).

### Awaiting

A human decision: either land Phase 5 into this checkout and re-dispatch 07-10's Task 3, or explicitly accept Phase 07 remaining incomplete until that happens. This plan's execution halts here; it is not resumed automatically.

## Next Phase Readiness

**Phase 07 is NOT ready to close.** Tasks 1 and 2's deliverables (`rsg-mock-stack`, `scripts/gpu_phase7_bench.sh`) are complete, tested on the Mac, and committed. Task 3 (the D-03 Mac dev pass and the phase gate) is blocked on Phase 5 landing in this checkout, per its own `<precondition>`. Because `requirements: [BENCH-02, BENCH-03, BENCH-04, BENCH-05, BENCH-06, BENCH-07, BENCH-08]` on this plan overlaps with 07-09's own `requirements-completed: [BENCH-06, BENCH-07, BENCH-08]`, the shared-ID gate correctly keeps all of BENCH-02..BENCH-08 at `Pending` in `REQUIREMENTS.md` until this plan (07-10) actually finishes -- nothing was marked complete in this run.

**The GPU-box headline Python-vs-Rust comparison did not run.** It was never expected to: Task 2's own verification explicitly defers the real run to a `<human-check>` on the Linux CUDA GPU box, after Phase 6 lands, as the plan's "single manual-only verification" for this phase. `scripts/gpu_phase7_bench.sh` is ready and dry-run-tested for that eventual run, but no GPU-measured numbers exist yet, and `docs/benchmarks/frontend-benchmarks.{json,md}` were not written by this session (confirmed: `git status --porcelain docs/benchmarks` is empty).

**STATE.md/ROADMAP.md/REQUIREMENTS.md are intentionally left at "Plan 10 in progress / blocked"** rather than advanced to "phase complete" -- see the state-update commit for exact wording. Re-running this plan's close-out steps after Task 3 lands will advance them correctly.

---
*Phase: 07-frontend-benchmarks*
*Completed: 2026-10-07 (Tasks 1-2 only; Task 3 blocked)*

## Self-Check: PASSED

- FOUND: crates/rsg-bench/src/bin/rsg-mock-stack.rs
- FOUND: crates/rsg-bench/tests/mock_stack.rs
- FOUND: scripts/gpu_phase7_bench.sh
- FOUND: python/tests/test_gpu_bench_script.py
- FOUND: .planning/phases/07-frontend-benchmarks/deferred-items.md
- FOUND: 44137d6 (feat: Task 1 rsg-mock-stack)
- FOUND: ee85b47 (feat: Task 2 gpu_phase7_bench.sh)
- No unexpected file deletions in either task commit
