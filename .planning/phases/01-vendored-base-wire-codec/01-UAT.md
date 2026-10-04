---
status: partial
phase: 01-vendored-base-wire-codec
source: [01-VERIFICATION.md]
started: 2026-10-04T04:55:00Z
updated: 2026-10-04T09:52:00Z
---

## Current Test

[testing paused — 2 items outstanding (tests 1 and 6 blocked on a Linux machine)]

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
result: blocked
blocked_by: physical-device
reason: "blocked. no linux machine yet. I can let other collaborators contineu on that after opening draft PR"

### 7. Triage the open code-review warnings in 01-REVIEW-DISPOSITION.md
expected: Record fixed / deferred / skipped for WR-06..WR-10 (new incremental review) and the earlier WR-01, WR-03, WR-04, WR-05. Settle WR-07 (the GPU-orphan check can false-PASS) and WR-08 (the start_session race can false-FAIL) before trusting gpu_phase1_check.sh steps 4/4b.
result: pass
reported: "修 WR-01, WR-04, WR-06, WR-07, WR-08, WR-09；延後 WR-03, WR-05, WR-10"
severity: major
triage: "fix WR-01, WR-04, WR-06, WR-07, WR-08, WR-09 (gaps G-01-7-WR01..WR09 below); defer WR-03/WR-10 to Phase 7 and WR-05 to Phase 6 (recorded in 01-REVIEW-DISPOSITION.md)"
resolution: "All 6 fix gaps closed by plans 01-09..01-12 and re-verified 2026-10-04 in 01-VERIFICATION.md (gaps_closed, gaps_remaining: [])"

### 8. Decide the disposition of the newly-surfaced WR-01 finding (abbreviated `--shell-m` with no rust binary built yet reports a misleading "binary not found" error instead of the shell-mode rejection)
expected: Either fix now (reorder: parse_args/run_shell check before resolve_rust_bin in python/rsglang/launch.py) or mark deferred in 01-REVIEW-DISPOSITION.md with a target phase. This is a NEW, narrower finding than the original WR-01 (already fixed and tested — abbreviations always exit 2, never silently run with shell-mode limits); it is a misleading-diagnostic issue only, not a correctness regression, and defeats no Phase 1 must-have, but must not be silently dropped.
result: issue
reported: "yes (fix now, per assistant recommendation: cheap reorder, prevents a confusing error on first-run setup)"
severity: minor

### 9. Record fixed/deferred for the carried-forward info-level findings IN-01, IN-02, IN-03 (new, from the latest incremental review) alongside the still-open IN-04..IN-13 and the still-deferred WR-03/WR-05/WR-10
expected: Each is marked fixed or deferred with a target phase, or explicitly accepted as non-blocking, in 01-REVIEW-DISPOSITION.md. None of these defeats a Phase 1 must-have.
result: issue
reported: "OK. let's go with your suggestion (fix IN-01 now; defer IN-02 and IN-03 to Phase 6)"
severity: minor
triage: "fix IN-01 (gap G-01-9 below); defer IN-02/IN-03 to Phase 6 (recorded in 01-REVIEW-DISPOSITION.md)"

## Summary

total: 9
passed: 5
issues: 2
pending: 0
skipped: 0
blocked: 2

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

- gap_id: G-01-7-WR01
  truth: "rust mode rejects shell mode however it is spelled: the parsed run_shell flag from upstream parse_args is authoritative, so `--shell` / `--shell-m` abbreviations exit 2 instead of silently running with max_running_req=1, cuda_graph_max_bs=1 (WR-01 in the first 01-REVIEW.md, commit 261f8ee)"
  status: resolved
  resolved_by: 01-11-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-01"
  severity: major
  test: 7
  root_cause: "python/rsglang/launch.py guards with a literal `\"--shell-mode\" in rest` (L101) and discards run_shell (`server_args, _ = parse_args(rest)`, L123); upstream argparse uses allow_abbrev=True."
  artifacts:
    - path: "python/rsglang/launch.py"
      issue: "literal pre-check + discarded run_shell"
  missing:
    - "`server_args, run_shell = parse_args(rest)`; if run_shell: log and return 2"
    - "test that passes `--shell` with --frontend rust and expects exit 2"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-7-WR04
  truth: "The rsg-server handshake rejects a line with no `eos_token_id` key as Malformed (exit 2); only an explicit null is accepted (WR-04 in the first 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-11-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-04"
  severity: major
  test: 7
  root_cause: "crates/rsg-server/src/handshake.rs:18-26: serde derive treats a missing Option<T> field as None even with deny_unknown_fields"
  artifacts:
    - path: "crates/rsg-server/src/handshake.rs"
      issue: "eos_token_id: Option<u64> gets an implicit default"
  missing:
    - "#[serde(deserialize_with = \"Option::deserialize\")] on eos_token_id"
    - "unit test removing the eos_token_id key expects Malformed"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-7-WR06
  truth: "A failing prctl(PR_SET_PDEATHSIG) does not kill the scheduler: it logs and degrades to the polling watchdog, and any startup failure still reaches the launcher as an error envelope (WR-06 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-10-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-06"
  severity: major
  test: 7
  root_cause: "python/rsglang/backend.py:77-80,100: start_parent_watchdog is called outside the try that posts {kind: error}; an OSError from prctl escapes; prctl argtypes not declared for variadic args"
  artifacts:
    - path: "python/rsglang/backend.py"
      issue: "prctl failure is fatal and bypasses the error envelope"
  missing:
    - "wrap prctl in try/except OSError, log to stderr, keep the polling thread"
    - "declare libc.prctl.argtypes as five c_int/c_ulong args"
    - "test (Mac-runnable, e.g. monkeypatched prctl failure) that the watchdog still arms and the scheduler keeps running"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-7-WR07
  truth: "gpu_phase1_check.sh's GPU-orphan check cannot false-PASS: an nvidia-smi failure is a step failure, and a listed pid is never reported absent because of SIGPIPE under pipefail (WR-07 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-09-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-07"
  severity: major
  test: 7
  root_cause: "scripts/gpu_phase1_check.sh:97,219-221,228,263-265,272: gpu_pids swallows nvidia-smi errors (2>/dev/null) and `gpu_pids | grep -qx` can return 141 under pipefail"
  artifacts:
    - path: "scripts/gpu_phase1_check.sh"
      issue: "gpu_pids / grep -q pipeline"
  missing:
    - "gpu_pids lets failure propagate; on_gpu captures output once and greps a here-string; return 2 on nvidia-smi failure treated as step failure"
    - "Mac-runnable check with a stubbed nvidia-smi on PATH (failing stub -> step fails; stub listing the pid -> detected)"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-7-WR08
  truth: "gpu_phase1_check.sh's start_session does not fail a healthy run on slow startup: it polls up to a timeout for pgid == BG_PID, and step4_early cleans up its session on that failure (WR-08 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-09-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-08"
  severity: major
  test: 7
  root_cause: "scripts/gpu_phase1_check.sh:85-92: fixed `sleep 0.5` then a one-shot pgid check; early return in step4_early leaves the session running until the EXIT trap"
  artifacts:
    - path: "scripts/gpu_phase1_check.sh"
      issue: "start_session fixed-sleep race; step4_early early return"
  missing:
    - "poll pgid every 0.1 s for up to ~5 s"
    - "step4_early stops its session when start_session fails"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-7-WR09
  truth: "test_exits_at_once_when_parent_is_not_the_launcher passes only when the watchdog itself caused the exit, not on an import error or any other exit 1 (WR-09 in 01-REVIEW.md)"
  status: resolved
  resolved_by: 01-10-PLAN.md
  resolved_at: 2026-10-04
  reason: "User reported: 修 WR-09"
  severity: major
  test: 7
  root_cause: "python/tests/test_parent_watchdog.py:27-39 asserts only returncode == 1, no 'survived', elapsed < 5"
  artifacts:
    - path: "python/tests/test_parent_watchdog.py"
      issue: "assertions cannot distinguish a watchdog exit from a traceback"
  missing:
    - "child prints an 'armed' marker after start_parent_watchdog returns; assert the marker is present and 'Traceback' not in stderr"
    - "mutation check: break the import or the watchdog and confirm the test fails"
  debug_session: "none - root cause from 01-REVIEW.md, re-verified in code at b15a8fd during UAT"

- gap_id: G-01-8
  truth: "Passing `--shell-m` (or another abbreviation) in rust mode always reports the shell-mode rejection as the error, even when the rust binary has not been built yet (new, narrower WR-01 instance surfaced by the 2026-10-04 incremental review)"
  status: failed
  reason: "User reported: yes (fix now, per assistant recommendation)"
  severity: minor
  test: 8
  root_cause: "python/rsglang/launch.py: run_rust_mode (L100-112) does a literal `\"--shell-mode\" in rest` pre-check (L101, catches only the exact spelling), then calls resolve_rust_bin (L104) and returns 2 on 'binary not found' (L105-106) BEFORE _run_rust_mode (L115) ever calls parse_args (L123) to get the authoritative run_shell flag that catches abbreviations like --shell/--shell-m. So `--shell-m` with no binary built yet hits the binary-not-found return first and the real reason (shell mode unsupported) is never reported."
  artifacts:
    - path: "python/rsglang/launch.py"
      issue: "run_rust_mode resolves rust_bin (L104) before _run_rust_mode's parse_args/run_shell check (L123-126) runs"
  missing:
    - "Move `server_args, run_shell = parse_args(rest)` and the `if run_shell: return 2` check into run_rust_mode, before the resolve_rust_bin call; pass the already-parsed server_args/run_shell down to _run_rust_mode instead of re-parsing"
    - "Test: --frontend rust --shell-m with no rust binary on PATH/resolved still exits 2 with the shell-mode-not-supported message, not a binary-not-found message"
  debug_session: "none - root cause found by direct code read during UAT (python/rsglang/launch.py:100-126)"

- gap_id: G-01-9
  truth: "The parent-death watchdog logs and degrades to the polling watchdog on ANY prctl(PR_SET_PDEATHSIG) failure mode, not just OSError (IN-01 in the 2026-10-04 incremental review)"
  status: failed
  reason: "User reported: OK. let's go with your suggestion (fix IN-01 now)"
  severity: minor
  test: 9
  root_cause: "python/rsglang/backend.py:81-97: start_parent_watchdog's try only catches OSError. `libc.prctl.argtypes = [...]` (L83) itself triggers a ctypes.CDLL.__getattr__ symbol lookup for 'prctl'; if that symbol isn't exported (a minimal/alternative libc, or a build without the usual glibc wrapper), ctypes raises AttributeError, not OSError, so it isn't caught here and the 'log it and degrade to the polling watchdog' behavior WR-06 was built to guarantee doesn't trigger for this failure mode (it's still caught one level up by run_scheduler's outer except BaseException, so it reaches the launcher as an error envelope rather than failing silently, but the degrade-and-keep-running guarantee is defeated)."
  artifacts:
    - path: "python/rsglang/backend.py"
      issue: "L92: `except OSError as exc:` does not catch AttributeError from a missing prctl symbol"
  missing:
    - "Broaden the catch to `except (OSError, AttributeError) as exc:` at L92"
    - "Mac-runnable test: monkeypatch libc.prctl (or the CDLL) to raise AttributeError and assert start_parent_watchdog logs 'PDEATHSIG unavailable' and returns normally (scheduler keeps running) instead of propagating"
  debug_session: "none - root cause from 01-REVIEW.md (IN-01), re-verified in code at python/rsglang/backend.py:81-97 during UAT"

- gap_id: G-01-7-WR06-CHECK
  truth: "A passing gpu_phase1_check.sh step 3 proves PR_SET_PDEATHSIG armed: step 3 fails when the scheduler logged 'PDEATHSIG unavailable' (fell back to the polling watchdog), while 01-10's degrade-to-polling runtime behavior stays"
  status: resolved
  resolved_by: 01-12-PLAN.md
  resolved_at: 2026-10-04
  reason: "User chose plan A on 2026-10-04 after comparing 01-08 (prctl failure aborts) with 01-10 (prctl failure degrades): keep the degrade, but make the GPU check fail if it triggered"
  severity: minor
  test: 7
  root_cause: "01-10 makes a prctl failure non-fatal, so a GPU run that reaches the handshake no longer proves PDEATHSIG armed; nothing in scripts/gpu_phase1_check.sh looks for the degrade line"
  artifacts:
    - path: "scripts/gpu_phase1_check.sh"
      issue: "step3 does not check rust-mode.log for 'PDEATHSIG unavailable'"
  missing:
    - "pdeathsig_degraded <log> helper above the source guard; step3 returns 1 when it matches"
    - "Mac tests on fixture logs with and without the line"
  debug_session: "none - design decision from the 01-08 vs 01-10 review in the UAT session"
