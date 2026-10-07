---
phase: 06-gpu-end-to-end-parity
plan: 05
subsystem: testing
tags: [zmq, msgpack, pytest, parity-harness, backend-probe]

# Dependency graph
requires:
  - phase: 06-02
    provides: scripts/gpu_phase6_watch.sh health watcher, tap infrastructure
  - phase: 06-03
    provides: compare.annotate_sequence, the 128-item parity corpus
  - phase: 06-04
    provides: sweep.run_session, sidecar schema, endpoints/sequential/concurrent run parts, verdict criteria 1-3
provides:
  - "stress run part: runs Phase 5's unmodified 128-agent stress tool against a fresh tapped session per abort timing, under scripts/gpu_phase6_watch.sh, with a canary request after settle"
  - "abort_analysis.analyze/failure_mode: classifies every abort (pending/pending_chunked/prefill_window/decode/not_found) and reduces a run to one failure_mode (crash > wedge > corrupted_requests > double_free > none)"
  - "probe.run_window_probe: injects UserMsg/AbortBackendMsg directly onto the scheduler's backend PULL socket with upstream's own encoder, at uids >= 2**40, across a configurable delay sweep -- exercises the abort-during-prefill window independent of frontend disconnect-detection latency"
  - "abort_analysis.analyze_probe: restricts the same classification to probe uids only, grouped by delay"
  - "abort_stress sidecar block (runs, probe, reproduced, conclusive) and verdict criterion 4"
  - "fake_parity_server knobs for every failure mode (crash/wedge/integrity/double-free/perturbation) and a --bind-backend + stress subcommand for GPU-free coverage"
affects: [06-06, 06-07, 06-08]

actuals:
  tokens: 24953
  tasks: 3
  commits: 3
  plan_head_before: 98198e67fe06cd3a6ace396ca0e605251214f896
  plan_head_after: 7569f2d3a25ef1e356eb5e39f5cf7200538c3412

tech-stack:
  added: []
  patterns:
    - "SessionContext/pass_context on sweep.run_session lets a workload reach the session's log path, launcher pid and tap dir without changing existing callers"
    - "failure_mode precedence (crash > wedge > corrupted_requests > double_free > none) turns ambiguous liveness signals into one evidence-backed outcome per run"
    - "Backend-level probe with its own uid namespace (>= 2**40) and upstream's own wire encoder proves a transport-level hypothesis independent of the frontend under test"

key-files:
  created:
    - python/rsglang/parity/abort_analysis.py
    - python/rsglang/parity/probe.py
    - python/tests/test_parity_stress.py
  modified:
    - python/rsglang/parity/stress.py
    - python/rsglang/parity/sweep.py
    - python/rsglang/parity/sidecar.py
    - scripts/parity_check.py
    - python/rsglang/testing/fake_parity_server.py

key-decisions:
  - "Gated the sequential comparison loop in parity_check.py on \"sequential\" in --parts (Rule 1 fix, Task 1): it previously ran unconditionally regardless of --parts, so `--parts stress` alone still launched real python/rust servers via the default server-cmd templates and crashed with ModuleNotFoundError: uvicorn"
  - "Probe uids start at 2**40 and the probe runs only after the stress command finishes, only inside the \"immediate\" timing's Rust-frontend session -- never against --frontend python, whose detokenizer has no state for foreign uids (T-06-12, T-06-13)"
  - "reproduced and conclusive each fold in both the stress run's own tap evidence and the probe's findings, so a bug caught only by the deliberate probe (and never by the frontend's own disconnect timing) still surfaces"

patterns-established:
  - "Pattern: fake-server knobs that force one failure mode at a time (--crash-after-requests, --hang-after-requests, --log-integrity-error, --double-free-on-abort, --perturb-input-when) keep the whole stress/probe contract testable on a GPU-free Mac"

requirements-completed: [PAR-02]

coverage:
  - id: D1
    description: "Stress part runs Phase 5's unmodified stress tool against a fresh tapped session per abort timing, under the watcher, with a post-settle canary"
    requirement: PAR-02
    verification:
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_tracer_stress_part_both_timings"
        status: pass
    human_judgment: false
  - id: D2
    description: "abort_analysis classifies every abort into one of 5 classes and reduces a run to one of 6 failure modes by fixed precedence"
    requirement: PAR-02
    verification:
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_analyze_classification"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_double_free_reproduced"
        status: pass
    human_judgment: false
  - id: D3
    description: "Crash, wedge and idle-time integrity failures are each recorded as a distinct, evidence-backed outcome without the run itself hanging; annotate_sequence wired into the sequential part; abort_stress required for GPU validation"
    requirement: PAR-02
    verification:
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_crash_failure_mode"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_wedge_failure_mode"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_integrity_error_recorded"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_annotate_wired"
        status: pass
    human_judgment: false
  - id: D4
    description: "Backend window probe sends UserMsg/AbortBackendMsg directly over upstream's own wire encoder at a delay sweep, independent of frontend disconnect-detection latency; probe-refined reproduced/conclusive; verdict criterion 4"
    requirement: PAR-02
    verification:
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_probe_wire_delivery"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_analyze_probe"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_stress_with_probe_against_fake"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_probe_skipped"
        status: pass
      - kind: unit
        ref: "python/tests/test_parity_stress.py#test_verdict_c4_synthetic"
        status: pass
    human_judgment: true
    rationale: "The real-GPU question this plan exists to answer -- does the abort-during-prefill double free actually reproduce against the real scheduler, and under which trigger -- can only be settled by running `scripts/parity_check.py run --parts stress` against the real backend on the GPU machine (plan 06-06 wires the real --stress-cmd/--stress-server-cmd). All automated coverage here proves the harness's logic against a fake backend; it cannot itself confirm the real-world finding."

duration: ~20min active (Tasks 1-2, single session) + resumed session for Task 3 (interrupted mid-run, finished and verified without rework)
completed: 2026-10-07
status: complete
---

# Phase 06 Plan 05: GPU-Backed Stress Test, Backend Window Probe and Verdict 4 Summary

**Re-ran Phase 5's unmodified 128-agent cancellation stress test around a process-health watcher and a scheduler-level tap, added a direct backend probe using upstream's own wire encoder, and turned "did the abort-during-prefill double free reproduce, how, and was the window ever hit" into recorded, evidence-backed sidecar fields and a verdict-4 check.**

## Performance

- **Duration:** ~20 min (Tasks 1-2) + a separate resumed session for Task 3, which was interrupted mid-verification and resumed from its existing work-in-progress rather than restarted
- **Completed:** 2026-10-07
- **Tasks:** 3
- **Files modified:** 8 (3 new: abort_analysis.py, probe.py, test_parity_stress.py; 5 modified: stress.py, sweep.py, sidecar.py, parity_check.py, fake_parity_server.py)

## Accomplishments

- `run --parts stress` runs Phase 5's stress tool, unmodified, against a fresh tapped session for each configured abort timing, under `scripts/gpu_phase6_watch.sh`, with a settle period and a post-settle canary request
- `abort_analysis.analyze` classifies every abort (pending, pending_chunked, prefill_window, decode, not_found) from the scheduler's own tap records and reduces a run to exactly one `failure_mode` (crash > wedge > corrupted_requests > double_free > none), so a crashed/wedged/corrupted run is never mistaken for "no bug"
- `probe.run_window_probe` sends `UserMsg` then `AbortBackendMsg` straight onto the scheduler's backend PULL socket using upstream's own `BaseBackendMsg` encoder, at uids `>= 2**40` across a configurable delay sweep -- deliberately exercising the abort-during-prefill window, which the frontend's own disconnect-detection latency might never land on its own
- `abort_stress.reproduced` and `.conclusive` now fold in both the stress run's tap evidence and the probe's findings; `verdict --criterion 4` checks both timings ran to completion, the deferred run didn't crash/wedge/fail-to-setup, `reproduced`/`conclusive` are recorded booleans, and the probe ran at least one trial
- `fake_parity_server.py` gained one knob per failure mode (`--crash-after-requests`, `--hang-after-requests`, `--log-integrity-error`, `--double-free-on-abort`, `--perturb-input-when`) plus `--bind-backend` and a `stress` subcommand, so the entire contract above is provable on a GPU-free Mac
- `annotate_sequence` (06-03's contract) is now wired into the sequential run part, and `require_gpu` validation demands a complete `abort_stress` block including a successful probe

## Task Commits

Each task was committed atomically:

1. **Task 1: Tracer -- stress part runs under the watcher with tap evidence** - `aef9f2e` (feat)
2. **Task 2: Crash/wedge/integrity failure modes, annotate_sequence wired, abort_stress required for GPU validation** - `45e28c1` (feat)
3. **Task 3: Backend window probe, probe-refined reproduced/conclusive, verdict 4** - `7569f2d` (feat)

_Note: Task 3 carried `tdd="true"`; its tests (listed in the plan's `<behavior>` block) and implementation landed together in one commit, consistent with how the work-in-progress was structured when the interrupted session was resumed -- all 5 behavior tests pass._

## Files Created/Modified

- `python/rsglang/parity/stress.py` - `run_stress_part`, `StressSetupError`, per-timing workload (scheduler-pid poll, watcher lifecycle, stress command, probe hook, settle, canary, teardown)
- `python/rsglang/parity/abort_analysis.py` - `analyze`, `analyze_probe`, `failure_mode`, abort-class and failure-mode constants
- `python/rsglang/parity/probe.py` - `run_window_probe` over upstream's `ZmqPushQueue`/`BaseBackendMsg` encoder
- `python/rsglang/parity/sweep.py` - `SessionContext(log_path, launcher_pid, tap_dir)` and `pass_context` keyword on `run_session` (backward compatible)
- `python/rsglang/parity/sidecar.py` - `abort_stress` block validation, including the probe's `status == "ok"` requirement under `require_gpu`
- `scripts/parity_check.py` - `stress` run part, `--stress-server-cmd`/`--stress-cmd`/`--abort-timings`/`--stress-timeout`/`--settle-s`/`--canary-timeout-s`/`--watch-interval-s`/`--probe-delays-ms`/`--probe-repeats` flags, verdict criterion 4, `annotate_sequence` wired into the sequential part, default `--parts` now `endpoints,sequential,concurrent,stress`
- `python/rsglang/testing/fake_parity_server.py` - `--emit-scheduler-child`, `--abort-timing`, `--double-free-on-abort`, `--crash-after-requests`, `--hang-after-requests`, `--log-integrity-error`, `--perturb-input-when`, `--bind-backend`, and a `stress` subcommand (fake 128-agent cancellation driver)
- `python/tests/test_parity_stress.py` - new test file; stress-part tracer, failure-mode classification, crash/wedge/integrity/annotate tests, probe wire-delivery/analysis/skip tests, verdict-4 synthetic tests

## Decisions Made

- Probe uids start at `1 << 40` and the probe is restricted to the "immediate" timing's Rust-frontend session, after the stress command and before settle/canary -- never against `--frontend python` (T-06-12, T-06-13 mitigations)
- `reproduced` combines the immediate run's own `failure_mode` with the probe's `double_free_total + collisions_total`; `conclusive` combines the immediate run's `aborts_by_class.prefill_window` with the probe's `prefill_window_hits` -- so a bug the frontend's own timing never triggers, but the probe does, still counts, and a probe that never lands in the window is distinguishable from a probe that was never run
- `scripts/parity_check.py`'s sequential comparison loop is gated on `"sequential" in parts` (Rule 1 fix landed in Task 1): previously it ran unconditionally, so `--parts stress` alone still tried to launch real python/rust servers via the default (GPU-only) server-cmd templates and crashed with `ModuleNotFoundError: uvicorn` on the Mac

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Sequential comparison loop ran unconditionally regardless of `--parts`**
- **Found during:** Task 1 (stress part tracer)
- **Issue:** `parity_check.py`'s sequential comparison loop had no `"sequential" in parts` guard, so `--parts stress` alone still launched the real `rsglang.launch` python/rust servers via the default server-cmd templates (GPU-only) and failed with `ModuleNotFoundError: uvicorn` on the Mac dev box
- **Fix:** Gated the loop on `"sequential" in parts`
- **Files modified:** `scripts/parity_check.py`
- **Verification:** `test_tracer_stress_part_both_timings` and all other `--parts stress`-only tests pass without touching the sequential path
- **Committed in:** `aef9f2e` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary for `--parts stress` to be usable standalone, which every test in this plan relies on. No scope creep.

## Issues Encountered

Task 3 (the backend window probe) was interrupted mid-verification in a prior session with substantial work-in-progress already in the working tree (`probe.py` complete and untracked; `stress.py`, `sidecar.py`, `fake_parity_server.py`, `scripts/parity_check.py` and the test file all modified). On resume, the work-in-progress was inspected in full against the plan's Task 3 `<action>`/`<behavior>` spec before touching anything: `probe.run_window_probe`, `abort_analysis.analyze_probe`, `stress.py`'s probe wiring, the fake server's `--bind-backend`, and `verdict --criterion 4` all matched the spec exactly, including the uid namespace (`1 << 40`), the probe-only-in-"immediate" restriction, and the `reproduced`/`conclusive` refinement. All 5 of Task 3's behavior tests, plus the full `test_parity_stress.py` + `test_parity_check.py` + `test_parity_tap.py` + `test_gpu_phase6_watch.py` suite (44 tests), passed without any fix-forward needed. The work was committed as-is.

## User Setup Required

None - no external service configuration required. (The real-GPU run of `--parts stress` itself is plan 06-06's concern, which wires the real `--stress-cmd`/`--stress-server-cmd` defaults from Phase 5's documented tool invocation.)

## Next Phase Readiness

- The `abort_stress` contract (runs, probe, reproduced, conclusive) and verdict criterion 4 are complete and fully exercised against the fake backend; 06-06 only needs to supply the real `--stress-cmd`/`--stress-server-cmd` values and run it on the GPU box
- `abort_stress.reproduced`/`.conclusive` are evidence-backed booleans ready for 06-08's D-09 triage -- "did it reproduce, how, and was the window ever exercised" are each answered from recorded fields rather than inferred from liveness alone
- No blockers. The only open question this plan cannot itself resolve, by design, is whether the suspected double free reproduces against the *real* GPU scheduler -- that requires the GPU run plan 06-06 sets up

---
*Phase: 06-gpu-end-to-end-parity*
*Completed: 2026-10-07*

## Self-Check: PASSED

All claimed files found on disk (abort_analysis.py, probe.py, test_parity_stress.py, stress.py, sweep.py, sidecar.py, parity_check.py, fake_parity_server.py). All claimed commits found in git log (aef9f2e, 45e28c1, 7569f2d).
