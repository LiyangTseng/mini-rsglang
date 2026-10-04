---
status: testing
phase: 01-vendored-base-wire-codec
source: [01-VERIFICATION.md]
started: 2026-10-04T04:55:00Z
updated: 2026-10-04T06:30:48Z
---

## Current Test

number: 6
name: Run the Linux-only parent-death watchdog test
expected: |
  On any Linux machine (no GPU needed): `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill -q` passes (PR_GET_PDEATHSIG == SIGKILL)
awaiting: user response

## Tests

### 1. Run `bash scripts/gpu_phase1_check.sh` on the Linux GPU box
expected: ALL PASS on steps 1-5 plus the early-kill step 4b added by 01-08 (covers the GPU halves of ROADMAP criteria 2 and 3; WINDOWS.md entry 1)
result: blocked
blocked_by: physical-device
reason: "手上沒有linux gpu怎麼辦? 我現在在mac 我應該要先commit 然後讓其他人在Linux machine上面測試？"

### 2. Decide the disposition of code-review finding CR-01 (group SIGINT / Ctrl-C makes the rust-mode launcher exit 1 with a failure report)
expected: Either fix now (re-check stop_requested right after each ready_queue.get and before scanning children, plus an e2e test that SIGINTs the launcher's process group and asserts exit 0), or mark it deferred in 01-REVIEW-DISPOSITION.md with a target phase
result: pass
reported: "修"
resolution: "Fixed by 01-07 (gap G-01-2); re-verified 2026-10-03 in 01-VERIFICATION.md, including a mutation check where the pre-fix code fails the new tests"

### 3. Decide the disposition of WR-02 (parent watchdog records getppid() only after the scheduler child has booted)
expected: Either pass the launcher pid explicitly (plus PR_SET_PDEATHSIG on Linux), or accept/defer it in 01-REVIEW-DISPOSITION.md
result: pass
reported: "修"
resolution: "Fixed by 01-08 (gap G-01-3); re-verified 2026-10-03 in 01-VERIFICATION.md, including a mutation check where the pre-fix code fails the new tests"

### 4. Review the judgment-tier prohibition from 01-03: rust mode runs the byte-identical upstream Scheduler and the handshake is not produced by patching vendored code
expected: Agree with the verifier's non-authoritative verdict that it holds (see the Prohibitions table in 01-VERIFICATION.md)
result: pass

### 5. Confirm the 01-01 process truth: no Python package was installed before you approved the PyPI names and pins
expected: Confirmed. The session record shows the 01-01 package gate was approved ("approve (use ... uv ...)") before the lock was installed into the project .venv
result: pass

### 6. Run the Linux-only parent-death watchdog test
expected: On any Linux machine (no GPU needed), `.venv/bin/python -m pytest python/tests/test_parent_watchdog.py::test_linux_arms_pdeathsig_sigkill -q` passes. It is skipped on macOS, so the prctl(PR_SET_PDEATHSIG) branch from 01-08 is unverified until then.
result: [pending]

### 7. Triage the open code-review warnings in 01-REVIEW-DISPOSITION.md
expected: Record fixed / deferred / skipped for WR-06..WR-10 (new incremental review) and the earlier WR-01, WR-03, WR-04, WR-05. Settle WR-07 (the GPU-orphan check can false-PASS) and WR-08 (the start_session race can false-FAIL) before trusting gpu_phase1_check.sh steps 4/4b.
result: [pending]

## Summary

total: 7
passed: 4
issues: 0
pending: 2
skipped: 0
blocked: 1

## Gaps

- gap_id: G-01-2
  truth: "Group SIGINT (Ctrl-C) to the rust-mode launcher's process group exits 0 without a failure report: stop_requested is re-checked right after each ready_queue.get and before scanning children, and an e2e test SIGINTs the launcher's process group and asserts exit 0 (CR-01 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-07-PLAN.md
  reason: "User reported: 修 (fix CR-01 now rather than defer)"
  severity: major
  test: 2
  root_cause: "Both rust-mode launcher loops in python/rsglang/launch.py check stop_requested only at the top of each iteration. The SIGINT/SIGTERM handler only sets a flag and PEP 475 retries the interrupted ready_queue.get to its full 0.5 s / 0.2 s timeout, so on a group SIGINT the children always react first (rsg-server exits 0 = its contractual signal code, or -2 if still booting; a pre-ready scheduler posts a KeyboardInterrupt error envelope) and the loop body classifies that as a failure -> shutdown(1) -> exit 1. Three branches: children() scan (L238-242 pre-ready, L271-275 post-ready), error envelope (L249-251, L268-270), handshake BrokenPipe (L256-258, code-reading only). Reproduced 6/6 post-ready, 3/3 pre-ready booting, 5/5 hang_before_ready; scratch patch with only a post-get stop re-check -> 15/15 exit 0."
  artifacts:
    - path: "python/rsglang/launch.py"
      issue: "ready-wait loop (L232-258) and supervise loop (L261-275) judge child exits / error envelopes / handshake BrokenPipe before re-checking stop_requested"
    - path: "python/tests/test_launch_rust_e2e.py"
      issue: "only stop test (L172-173) signals the launcher pid, never the process group"
  missing:
    - "In both loops: msg = None on queue.Empty, then `if stop_requested: return shutdown(0)` before handling the message or scanning children"
    - "Re-check stop_requested at each child-state-driven shutdown(1) decision (children scan `if code is not None`, error envelope, handshake BrokenPipeError) to close the residual microsecond window"
    - "Do NOT change shutdown()'s killpg(SIGINT) (L193) in this fix: the pid-only SIGTERM test depends on it; WR-05 stays a separate, unregressed issue"
    - "e2e tests in test_launch_rust_e2e.py using LauncherRun/make_launcher: os.killpg(run.proc.pid, SIGINT) (a) after 'backend ready; handshake sent' and (b) pre-ready with RSGLANG_FAKE_MODE=hang_before_ready (envelope branch); signal only after 'spawned scheduler rank=0' is logged; assert exit 0, no 'exited with code'/'failed'/stderr-tail lines, children gone, sockets removed"
  debug_session: .planning/debug/cr01-group-sigint-exit-1.md

- gap_id: G-01-3
  truth: "The parent-death watchdog cannot miss a launcher that dies early: the launcher pid is passed explicitly to the scheduler child (not read via getppid() after boot), plus PR_SET_PDEATHSIG on Linux, so kill -9 of the launcher never leaves an orphaned scheduler (WR-02 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-08-PLAN.md
  reason: "User reported: 修 (fix WR-02 now rather than defer)"
  severity: major
  test: 3
  root_cause: "python/rsglang/backend.py:61 start_parent_watchdog() takes its reference pid from os.getppid() when run_scheduler starts, not from the launcher pid. Under the forced spawn start method (launch.py:156) run_scheduler only starts after the child boots a fresh interpreter, re-imports rsglang.launch as __mp_main__ and unpickles ServerArgs (imports torch, minisgl.utils.hf, minisgl.server.launch): ~2.2-2.6 s on the Mac, longer with CUDA torch. A launcher SIGKILLed (pid only) in that window has already reparented the child, so the watchdog records ppid 1/subreaper as 'the parent' and never fires; the scheduler runs forever (on GPU: loads weights, holds the GPU). Launcher pid is never passed (launch.py:164) and no PR_SET_PDEATHSIG / spawn-pipe sentinel is used. Reproduced 5/5 in-window orphans vs 4/4 clean post-window kills. rsg-server is NOT affected (stdin EOF is persistent). Existing e2e test (test_launch_rust_e2e.py:228-240) and gpu_phase1_check.sh step 4 both kill only after the handshake, so they pass with the bug present."
  artifacts:
    - path: "python/rsglang/backend.py"
      issue: "L54-75: start_parent_watchdog() captures os.getppid() late (L61, compared at L66); neither it nor run_scheduler takes the launcher pid; no PR_SET_PDEATHSIG"
    - path: "python/rsglang/launch.py"
      issue: "L162-166: mp.Process args omit os.getpid(); keep spawn (L156) - fork is unsafe with CUDA"
    - path: "python/tests/test_launch_rust_e2e.py"
      issue: "L228-240: orphan test kills only after the handshake, outside the window"
    - path: "scripts/gpu_phase1_check.sh"
      issue: "step 4 (~L212-235) kills only after the handshake; same blind spot on the GPU box"
  missing:
    - "Pass the launcher pid explicitly: args=(rank_args, ready_queue, upstream_sha, os.getpid()); run_scheduler/start_parent_watchdog take launcher_pid"
    - "In the child: on Linux prctl(PR_SET_PDEATHSIG, SIGKILL) via ctypes first, then immediately `if os.getppid() != launcher_pid: os._exit(1)`; keep the polling thread comparing against launcher_pid (only mechanism on macOS, backstop on Linux). Optional: daemon thread on multiprocessing.parent_process().join()"
    - "Mac e2e test: kill -9 the launcher right after 'spawned scheduler rank=0' and assert the scheduler pid disappears; make the window deterministic with a test-only PYTHONPATH sitecustomize.py that sleeps when '--multiprocessing-fork' in sys.orig_argv; deadline > sleep + ~3 s imports"
    - "Mac unit test: subprocess calls start_parent_watchdog(launcher_pid=<not its parent>) and must exit 1 promptly; Linux-only skipif test for PR_GET_PDEATHSIG == SIGKILL"
    - "Add an early-kill variant to gpu_phase1_check.sh step 4 (kill -9 right after 'spawned scheduler rank=0')"
  debug_session: .planning/debug/wr02-watchdog-late-ppid.md
