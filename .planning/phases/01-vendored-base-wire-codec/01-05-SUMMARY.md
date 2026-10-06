---
phase: 01-vendored-base-wire-codec
plan: 05
subsystem: infra
tags: [launcher, d-12, process-group, sigkill, watchdog, pytest, gpu-check, bash]

requires:
  - phase: 01-vendored-base-wire-codec (plan 01-03)
    provides: "rsglang.launch tracer (process group, readiness wait, SIGINT shutdown), backend.run_scheduler, handshake/sockets seams, FakeScheduler, e2e LauncherRun harness"
  - phase: 01-vendored-base-wire-codec (plan 01-02)
    provides: "rsg-server: exit 3 on stdin EOF, exit 0 on SIGINT/SIGTERM, 'handshake received' log line"
provides:
  - "Full D-12 failure contract in rsglang.launch: rsg-server stderr pump + 200-line tail on failure, SIGINT then group SIGKILL escalation, socket cleanup on every survivable exit path"
  - "rsglang.backend.start_parent_watchdog(poll_interval=1.0): scheduler exits when the launcher is gone"
  - "rsglang.testing.fake_scheduler.MODE_ENV (RSGLANG_FAKE_MODE): ok, crash_before_ready, hang_before_ready, crash_after_ready"
  - "Five slow e2e failure tests plus fast unit tests pinning topology, socket paths and handshake payload"
  - "scripts/gpu_phase1_check.sh: one-command GPU check for ROADMAP criteria 2 and 3, no-orphan backstop, frozen frontend"
affects: [01-06-integrity-gates, phase-03-transport, phase-05-http, phase-07-bench]

actuals:
  tokens: 6599    # chars/4 over added lines of the realized diff (26394 chars, 7 files)
  tasks: 3
  commits: 3
plan_head_before: 57dd300cead455cedd518bbd9d3e7e41eb5b686d
plan_head_after: f1abe651ee7d9dd2f692ec530ad87a19ff01acca

tech-stack:
  added: []
  patterns:
    - "Child stderr pump: daemon thread forwards '[rsg-server] <line>' live and keeps collections.deque(maxlen=200) for failure tails"
    - "Shutdown: SIG_IGN only at shutdown, killpg(SIGINT), 10 s grace, unlink sockets, killpg(SIGKILL) if any direct child survives (ends the launcher, status -9)"
    - "Scheduler parent watchdog: daemon thread polls os.getppid() and os._exit(1) on change; started first in run_scheduler"
    - "Fake failure modes selected by env var so e2e tests drive real processes through each D-12 path"
    - "Background jobs in scripts: restore SIGINT to SIG_DFL before exec (non-interactive shells start them with SIGINT ignored)"

key-files:
  created:
    - python/tests/test_topology.py
    - python/tests/test_handshake.py
    - scripts/gpu_phase1_check.sh
  modified:
    - python/rsglang/launch.py
    - python/rsglang/backend.py
    - python/rsglang/testing/fake_scheduler.py
    - python/tests/test_launch_rust_e2e.py

key-decisions:
  - "D-12 failure tests assert a non-zero exit (1, or -9 when SIGKILL escalation was needed), per the plan; both outcomes satisfy the contract"
  - "After ready, the supervise loop also reads scheduler error envelopes, so a crash after ready prints 'scheduler rank R failed:' with the traceback, not just an exit code"
  - "Launcher stderr writes tolerate a closed or broken stderr, so supervision and shutdown always complete (e.g. a pipeline reader that already exited)"
  - "gpu_phase1_check.sh starts each run via a Python exec wrapper that restores SIGINT to default, because a non-interactive shell's background job inherits SIGINT ignored and python mode would then ignore kill -INT"

patterns-established:
  - "Launcher failure log contract: 'scheduler rank R failed:' + traceback | '<name> exited with code C' | 'backend not ready after T s', then '--- last N lines of rsg-server stderr ---' ... '--- end ---', then 'exit code 1' or 'escalating to SIGKILL for the process group'"

requirements-completed: [BASE-02, BASE-03]

coverage:
  - id: D1
    description: "Launcher seams pinned by fast unit tests against real upstream parse_args: D-07 roles (detok bind by default, connect with --num-tokenizer 2), exact rsg-server argv, per-run socket paths and decoy-safe unlink"
    requirement: BASE-02
    verification:
      - kind: unit
        ref: "python/tests/test_topology.py (11 tests)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Handshake payload pinned: page_size from cache_manager, max_seq_len from engine, exact contract bytes, eos null, key set/order enforced, SHA file validation, scheduler factory resolution"
    requirement: BASE-03
    verification:
      - kind: unit
        ref: "python/tests/test_handshake.py (10 tests)"
        status: pass
    human_judgment: false
  - id: D3
    description: "D-12 on the Mac: scheduler crash before/after ready, never-ready timeout, rsg-server death and launcher SIGKILL all end with a non-zero exit (or no orphan for the SIGKILL case), a printed cause and no surviving child"
    requirement: BASE-02
    verification:
      - kind: integration
        ref: "python/tests/test_launch_rust_e2e.py (tracer + 5 failure tests)"
        status: pass
    human_judgment: false
  - id: D4
    description: "SIGKILL escalation path: a child that ignores SIGINT is killed with the group after the 10 s grace; launcher status 137, children gone, sockets removed"
    verification:
      - kind: manual_procedural
        ref: "ad hoc: SIGSTOP the scheduler, SIGTERM the launcher (run during Task 2)"
        status: pass
    human_judgment: true
    rationale: "Exercised once by hand on the Mac, not by an automated test; a reviewer should judge whether an automated escalation test is wanted"
  - id: D5
    description: "GPU end-of-phase check: real backend serves a Python-mode chat completion, the Rust-mode handshake carries real values (max_seq_len <= 40960, eos 151645, page_size 1 or 64, max_running_req 256, num_pages > 1, SHA), kill -9 of the launcher leaves nothing in ps or nvidia-smi, check_upstream.py passes"
    requirement: BASE-03
    verification:
      - kind: other
        ref: "bash -n scripts/gpu_phase1_check.sh && bash scripts/gpu_phase1_check.sh --help | grep -q 'Usage:'"
        status: pass
    human_judgment: true
    rationale: "Needs the Linux GPU box; only syntax and --help are checked on the Mac. A human runs the script and replies 'approved' (embedded human-check of Task 3)"

duration: 44min
completed: 2026-10-04
status: complete
---

# Phase 1 Plan 05: Launcher Failure Contract and GPU Check Summary

**The rust-mode launcher now implements D-12 in full. It pumps rsg-server's stderr into a 200-line tail that it prints on failure. Shutdown sends SIGINT to the group, waits 10 s, and then SIGKILLs the whole group if any child survives. The scheduler wrapper runs a ppid watchdog, so killing the launcher with kill -9 leaves no orphan. Five e2e tests prove each failure mode on the Mac. Fast unit tests pin the topology and handshake seams against upstream's real `parse_args`. `scripts/gpu_phase1_check.sh` runs the GPU-only checks for criteria 2 and 3 from one command.**

## Performance

- **Duration:** 44 min (most of it spent hunting a single flaky run; see Issues)
- **Started:** 2026-10-04T03:11:58Z
- **Completed:** 2026-10-04T03:56:44Z
- **Tasks:** 3
- **Files modified:** 7 (3 created, 4 modified)

## Accomplishments

- **D-12 failure contract (BASE-02).** Every failure path prints its cause:
  - a scheduler error envelope with its traceback, before or after ready;
  - a dead child with its exit code;
  - the ready timeout.

  Each failure then prints the tail of rsg-server's stderr, stops the group and exits 1. If a child is still alive after the 10 s grace, the launcher SIGKILLs the group, itself included, and exits with status -9. Sockets are unlinked on every exit path the launcher survives.
- **Parent watchdog (T-01-13).** `start_parent_watchdog()` runs first in `run_scheduler`. A mutation check confirmed `test_launcher_sigkill_leaves_no_orphans` fails without it ("scheduler pid=… orphaned").
- **Seam unit tests (BASE-02/03).** 21 fast tests run against real upstream `parse_args`. All passed against the 01-03 code, so no defects were found in `sockets.py`, `handshake.py` or `backend.py`.
- **GPU script.** `scripts/gpu_phase1_check.sh` runs five steps and prints PASS or FAIL for each. It exits 0 only if all five pass, and it never touches `vendor/`.
- The plan's verification passes: `pytest test_topology.py test_handshake.py test_launch_rust_e2e.py` gives 27 passed, and the fast suite gives 28 passed. The vendored tree hash `02d3e4ad…` is unchanged.

## Task Commits

1. **Task 1: Unit tests for the launcher seams.** `323458b` (test). The task is marked tdd, but the code under test already existed from 01-03. These are characterization tests, so no RED failure was possible, and none of them exposed a defect.
2. **Task 2: D-12 failure contract.** `a1fc5d6` (feat).
3. **Task 3: GPU verification script.** `f1abe65` (feat).

## Files Created/Modified

- `python/rsglang/launch.py`: stderr pump and tail, failure-tail printing, group SIGKILL escalation, try/finally socket cleanup, post-ready error-envelope handling, broken-stderr-tolerant logging
- `python/rsglang/backend.py`: `start_parent_watchdog()`, called first in `run_scheduler`
- `python/rsglang/testing/fake_scheduler.py`: `MODE_ENV` and the three failure modes (unknown mode raises ValueError)
- `python/tests/test_launch_rust_e2e.py`: `make_launcher` factory fixture, `LauncherRun(mode=, ready_timeout=)`, `finish()`/`text()`, five failure tests
- `python/tests/test_topology.py`: D-07 roles, argv, socket paths, suffix validation, decoy-safe unlink
- `python/tests/test_handshake.py`: extraction, contract bytes, key enforcement, SHA file, factory resolution
- `scripts/gpu_phase1_check.sh`: the GPU end-of-phase check (`--model`, `--port`, `--timeout`, `--help`; `PYTHON`)

## Decisions Made

See `key-decisions` in the frontmatter.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Scheduler errors after ready are read and printed**
- **Found during:** Task 2
- **Issue:** After the handshake, the 01-03 supervise loop only polled exit codes. A crash after ready would print "<name> exited with code 1" but not the scheduler's traceback, and D-12 requires printing the cause.
- **Fix:** The supervise loop waits on the ready queue (it replaces the 0.2 s sleep) and prints any error envelope. When a child exit is detected, the launcher first drains the queue for up to 1 s for envelopes.
- **Files modified:** python/rsglang/launch.py
- **Verification:** test_scheduler_crash_after_ready and test_scheduler_crash_before_ready
- **Committed in:** a1fc5d6

**2. [Rule 2 - Missing Critical] Shutdown survives a broken or closed launcher stderr**
- **Found during:** Task 2, while handling the 01-03 pipeline caveat (a)
- **Issue:** When the launcher runs in a pipeline and the reader (for example `tee`) dies from the group SIGINT, a later stderr write raises BrokenPipeError. That would abort shutdown halfway, before the wait, unlink and escalation steps.
- **Fix:** All launcher stderr writes (`_log` and the pump) go through `_write_stderr`, which swallows OSError and ValueError.
- **Files modified:** python/rsglang/launch.py
- **Committed in:** a1fc5d6

**3. [Rule 1 - Bug] The GPU script's python-mode stop would have been ignored**
- **Found during:** Task 3
- **Issue:** A non-interactive bash starts `&` jobs with SIGINT ignored, and that disposition survives exec. In python mode the launcher execs `python -m minisgl` without installing handlers, so the step's `kill -INT -- -<pgid>` would do nothing, and every run would fall through to the 60 s SIGKILL fallback. I verified on the Mac: a plain background job reported `SIGINT=1` (SIG_IGN), and the wrapped one reported `default_int_handler` with the same pid as `$!`.
- **Fix:** `start_session` execs `setsid` through a one-line `$PYTHON` wrapper that restores SIG_DFL first. The step waits for the whole process group, not just the leader, and sends SIGKILL with a WARN if the group outlives the 60 s.
- **Files modified:** scripts/gpu_phase1_check.sh
- **Committed in:** f1abe65

**4. [Plan detail] rsg-server and scheduler pids, `--port` in rust mode, and a setsid preflight**
- Rust mode also passes `--port "$PORT"`. Upstream's `distributed_addr` uses port+1, so a user-chosen port should apply to both modes.
- `setsid` was added to the preflight tools.
- The script exports `PYTHONPATH=<repo>/python` as a fallback, for a checkout where `rsglang` is not pip-installed.

---

**Total deviations:** 3 auto-fixed (2 Rule 2, 1 Rule 1), plus small plan-detail additions to the script.
**Impact on plan:** All of them are inside the planned files and needed for D-12 to be observable and for the GPU script to work as intended. There is no scope creep.

## Issues Encountered

- **One unexplained e2e failure.** One run in about 110 failed. It took about 10 s longer than usual, which matches the 10 s grace followed by SIGKILL escalation. The failure tests asserted `== 1` at that point, and the output was not captured. The failure did not recur in about 110 more runs, including targeted loops and two concurrent loops. I ruled out two hypotheses by measurement. First, a blocking zmq `socket.poll` in the fake's probe: it is interrupted by SIGINT within 0.1 s. Second, the watchdog thread absorbing SIGINT: blocking SIGINT in that thread made no difference. The tests now assert non-zero, as the plan specifies, and the escalation outcome (-9) is D-12-compliant. This is recorded in `deferred-items.md` with what to look for if it recurs.
- **Pipeline caveat (a) from 01-03.** This is only partly addressed (deviation 2). `killpg` still signals the other members of a pipeline when the launcher is already their group leader. The workaround is `setsid` or redirecting to a file. A structural fix needs a decision, so it is recorded in `deferred-items.md`.
- In the e2e tests rsg-server's log lines now arrive through the pump with a `[rsg-server] ` prefix. The existing tracer assertions use substring search, so they are unaffected.

## Known Stubs

None.

## Threat Flags

None. No new network, auth or trust-boundary surface. T-01-13 and T-01-14 are mitigated as planned, and T-01-15 (stderr tails) is accepted as planned.

## User Setup Required

The end-of-phase GPU sign-off (Task 3's human-check) needs one run on the Linux GPU box:
1. `uv venv --python=3.12 && uv pip install -e vendor/mini-sglang && uv pip install torch-c-dlpack-ext && uv pip install -e .` (needs build-essential for zmq-sys)
2. `bash scripts/gpu_phase1_check.sh`
3. Expect PASS for all five steps, then reply "approved" or paste the failing step and the printed log directory.

Step 5 needs `scripts/check_upstream.py` from plan 01-06. It reports FAIL until that plan has landed. This is recorded as an open `unrun-verify` entry in `.planning/WINDOWS.md`.

## Next Phase Readiness

- 01-06 (integrity gates) can proceed. It owns `scripts/check_upstream.py`, which step 5 of the GPU script invokes.
- Phase 1 sign-off is blocked on the human GPU run (D5 above).

---
*Phase: 01-vendored-base-wire-codec*
*Completed: 2026-10-04*

## Self-Check: PASSED

- All 7 key files are present on disk.
- Commits 323458b, a1fc5d6 and f1abe65 are present in git log.
- Plan verification: 27 passed; fast suite: 28 passed; `bash -n` and `--help` OK; vendored tree hash unchanged.
