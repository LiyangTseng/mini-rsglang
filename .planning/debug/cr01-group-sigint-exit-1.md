---
status: diagnosed
trigger: "CR-01: group SIGINT (Ctrl-C) to the rust-mode launcher's process group exits 1 with a failure report instead of exiting 0 (UAT test 2, user chose to fix: 修)"
created: 2026-10-04T05:22:18Z
updated: 2026-10-04T05:40:00Z
goal: find_root_cause_only
---

## Current Focus

bug_class: Bohrbug in practice. It is a race on paper, but PEP 475 makes the losing interleaving deterministic: the launcher's blocking wait always outlasts rsg-server's millisecond-scale exit. SBFL skipped: the suite has no failing test and no per-test coverage.
hypothesis: CONFIRMED (H1, extended to H1-pre and H1-envelope). The launcher judges child exits and scheduler error envelopes before it re-checks stop_requested, in both the ready-wait loop and the supervise loop.
test: done. Reproduced 6/6 post-ready, 3/3 pre-ready during boot, 5/5 pre-ready with a hung scheduler. A scratch copy with only the stop re-check gave 15/15 exit 0.
expecting: n/a
next_action: none (find_root_cause_only). Hand back to the orchestrator for a fix plan.

reasoning_checkpoint:
  hypothesis: "The rust-mode launcher exits 1 on a group SIGINT. Its ready-wait loop (launch.py:232-251) and supervise loop (launch.py:261-275) check stop_requested only at the top of each iteration. The SIGINT handler only sets a flag, and PEP 475 retries the interrupted ready_queue.get until its 0.5 s or 0.2 s timeout. So by the time the loop body runs, the children have already reacted to the same signal: rsg-server has exited 0 (or -2 if it was still booting), or a pre-ready scheduler has posted a KeyboardInterrupt error envelope. The body classifies that as a failure and calls shutdown(1)."
  confirming_evidence:
    - "PEP 475 probe: the handler runs at +0.101 s, but mp.Queue.get(timeout=0.5/2.0) returns only at 0.502 s / 2.002 s"
    - "Original code: group SIGINT after the handshake gives exit 1 in 6/6 runs ('rsg-server exited with code 0' about 1.2 s after rsg-server's 'received SIGINT; exiting')"
    - "Original code: group SIGINT during scheduler boot gives exit 1 in 3/3 runs ('exited with code 0/-2 before ready'). Group SIGINT with a hung pre-ready scheduler gives exit 1 in 5/5 runs ('scheduler rank 0 failed: KeyboardInterrupt')"
    - "Differential control: the same original code with SIGTERM to the launcher pid only (children not signalled directly) gives exit 0 in 3/3 runs"
    - "Intervention: a scratch copy that adds only `if stop_requested: return shutdown(0)` right after each get gives exit 0 in 15/15 runs (post 5, pre 5, hang 5), with no leftover processes"
  falsification_test: "If the patched copy had still exited 1, or the pid-only control had also exited 1, the cause would lie elsewhere (shutdown(), SIGKILL escalation, an inherited signal disposition). Neither happened."
  fix_rationale: "The defect is the order of operations: failure classification runs before the stop check. Re-checking stop right after the blocking wait, and again at the point of deciding failure, routes stop-induced child exits and envelopes to shutdown(0). It does not change how a real crash is reported when no stop was requested."
  blind_spots: "The handshake BrokenPipeError branch (launch.py:256-258), hit when 'ready' and the stop arrive together, was found only by code reading and was not reproduced. The real GPU scheduler was not exercised; its pre-ready window (weight load) is much longer, which only makes pre-ready Ctrl-C more likely. TP>1 was not tested."
  candidate_causes:
    - "code: stop_requested re-checked only at the top of each loop, after a blocking wait, and failure branches never consult it (CONFIRMED)"
    - "environment/runtime: PEP 475 auto-retry of the interrupted ready_queue.get turns a timing race into a deterministic one (contributing condition; not a defect in itself)"
    - "config: _READY_POLL_S=0.5 / _SUPERVISE_POLL_S=0.2 and report_errors(1.0) only set the delay (1.2-1.5 s), not the outcome; shrinking them would not fix it (ELIMINATED as cause)"
    - "data/protocol: rsg-server's exit 0 means 'signal' by contract (STATE.md), but the launcher treats every child exit code as a failure (part of the code cause)"
  and_gate: "yes. The failure needs (1) the stop signal delivered directly to the children (group delivery, i.e. terminal Ctrl-C or kill -INT -- -pgid) AND (2) the launcher classifying child state before re-checking stop. PEP 475 guarantees (2)'s window always covers (1)'s effect. (1) is the legitimate user scenario. Removing (2) alone is sufficient (15/15 exit 0)."

## Symptoms

expected: Group SIGINT (Ctrl-C) to the rust-mode launcher's process group exits 0 without a failure report; stop_requested re-checked right after each ready_queue.get and before scanning children; e2e test SIGINTs the launcher's process group and asserts exit 0.
actual: User reported "修" (fix CR-01 now rather than defer). Code review reproduced 3/3 with the fake scheduler: prints "rsg-server exited with code 0", then "exit code 1", process exits 1.
errors: "rsg-server exited with code 0" followed by "exit code 1"; launcher exits 1 and dumps rsg-server stderr tail as a failure.
reproduction: start launcher with start_new_session=True using RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler; after "handshake sent" do os.killpg(pid, SIGINT). See CR-01 in 01-REVIEW.md (python/rsglang/launch.py:232-246 and :261-275).
started: Discovered during Phase 01 code review; confirmed for fixing during UAT.

## Eliminated

- hypothesis: rsg-server genuinely fails or crashes on SIGINT, so a non-zero exit is warranted
  evidence: rsg-server logs "received SIGINT; exiting" and exits 0, its documented signal code (main.rs:90-93). The one -2 came from a SIGINT that arrived before its tokio handler was installed. In both cases the exit is the signal's intended effect, not a crash.
  timestamp: 2026-10-04T05:27:20Z

- hypothesis: SIGKILL escalation, or a child ignoring SIGINT for the 10 s grace, produces the non-zero exit (the deferred-items "slow e2e failure" pattern)
  evidence: No "escalating to SIGKILL" line in any run. The exit code is exactly 1 (returned by shutdown(1)), the total elapsed time is under 2 s, and no processes are left over.
  timestamp: 2026-10-04T05:27:00Z

- hypothesis: An environment problem, such as the launcher's SIGINT handler not being installed or SIGINT ignored by inheritance (the gpu_phase1_check.sh background-job issue)
  evidence: The PEP 475 probe shows the handler running. The original code with a pid-only SIGTERM exits 0 (3/3). The patched copy routes the same group SIGINT to shutdown(0) (15/15). The handler works; the loop ignores its flag at the wrong moment.
  timestamp: 2026-10-04T05:33:00Z

- hypothesis: Process-group setup (start_new_session vs the launcher's own setpgid) makes the review's repro differ from the real terminal case
  evidence: The bug reproduces identically with and without start_new_session (3/3 each). In both cases pgid == launcher pid, the same group a terminal Ctrl-C reaches when the launcher is started directly from the shell.
  timestamp: 2026-10-04T05:27:00Z

- hypothesis: The failure is an artifact of the fake scheduler
  evidence: Post-ready, the fake scheduler exits quietly. The failure comes only from the launcher's children() scan of the real rsg-server binary. Pre-ready, the error envelope is produced by backend.py:103-107, the same wrapper the real upstream Scheduler runs under. The fake contributes only the hang point.
  timestamp: 2026-10-04T05:28:30Z

## Evidence

- timestamp: 2026-10-04T05:24:00Z
  checked: Phase 0 knowledge base (.planning/debug/knowledge-base.md in worktree and main checkout); MemPalace
  found: No knowledge base exists yet (.planning/debug did not exist). MemPalace is not available in this agent's toolset.
  implication: No known-pattern candidate; investigate from scratch.

- timestamp: 2026-10-04T05:25:00Z
  checked: python/rsglang/launch.py:133-141 (signal handler)
  found: _request_stop only sets the nonlocal stop_requested = True for SIGINT and SIGTERM. It does not raise, so a blocking call interrupted by the signal is retried (PEP 475) and nothing in the loop runs until that call returns.
  implication: Any blocking wait keeps going after a stop signal until its own timeout or data arrives.

- timestamp: 2026-10-04T05:25:30Z
  checked: python/rsglang/launch.py:229-251 (ready-wait loop)
  found: stop_requested is checked only at the top of the loop (L233). After ready_queue.get(timeout=0.5) the code goes straight into (a) on queue.Empty: the children() scan (L238-242) -> any non-None exit code -> "X exited with code C before ready" -> shutdown(1); (b) on a message of kind "error": "scheduler rank N failed" -> shutdown(1) (L249-251); (c) on kind "ready": it writes the handshake, and a BrokenPipeError -> "rsg-server exited with code C before the handshake was sent" -> shutdown(1) (L253-258). None of these three branches re-checks stop_requested.
  implication: The pre-ready path has three ways to turn a stop into exit 1, not just the children-scan path the review names.

- timestamp: 2026-10-04T05:26:00Z
  checked: python/rsglang/launch.py:261-275 (supervise loop)
  found: stop_requested is checked only at the top (L262). After ready_queue.get(timeout=0.2), the error-envelope check (L268-270) and the children() scan (L271-275) both run before any re-check. Any child with a non-None exit code -> "X exited with code C" -> report_errors(1.0) -> shutdown(1).
  implication: Matches the review's post-ready mechanism.

- timestamp: 2026-10-04T05:26:30Z
  checked: crates/rsg-server/src/main.rs:90-93,168-169,186-187
  found: rsg-server handles SIGINT/SIGTERM with tokio signal handlers, both before and after the handshake, and calls exit_on_signal -> std::process::exit(0). By contract (STATE.md) exit 0 means "signal".
  implication: Under a group SIGINT, rsg-server exits 0 almost at once. The launcher's children() then sees code 0 (not None) and treats the contractual "signal" exit as a failure.

- timestamp: 2026-10-04T05:27:00Z
  checked: python/rsglang/backend.py:74-107 (scheduler wrapper)
  found: A KeyboardInterrupt before passed_ready puts {"kind":"error",...,"traceback":...KeyboardInterrupt} on ready_queue and re-raises (exitcode 1). After passed_ready, run_forever's except KeyboardInterrupt runs scheduler.shutdown() and returns. A second KeyboardInterrupt inside that is swallowed by the outer `except BaseException ... and passed_ready: return`.
  implication: Before ready, a group SIGINT also yields an error envelope that the ready-wait loop reports as a failure (branch b). After ready, the scheduler side exits quietly, so the failure there comes only from the launcher's children() scan of rsg-server.

- timestamp: 2026-10-04T05:27:30Z
  checked: python/rsglang/launch.py:188-227 (shutdown) and 01-REVIEW.md WR-05
  found: shutdown(code) always sets SIG_IGN on the launcher and then os.killpg(os.getpgrp(), SIGINT). It waits for rsg-server and joins the ranks within a shared 10 s grace, dumps the rsg-server stderr tail only if code != 0, and SIGKILLs the group if anything is still alive. shutdown(0) is the same as shutdown(1) except for the tail dump and the returned code.
  implication: Routing a stop to shutdown(0) does not change the signal fan-out. WR-05 (a second group SIGINT during the children's graceful cleanup) still happens on the fixed path and is neither made worse nor fixed by the CR-01 fix.

- timestamp: 2026-10-04T05:28:00Z
  checked: python/tests/test_launch_rust_e2e.py
  found: The only stop-signal test (test_rust_mode_handshake_reaches_rsg_server, L172-173) sends SIGTERM to the launcher pid only (launcher.proc.send_signal) and asserts exit 0. No test signals the process group. LauncherRun spawns without start_new_session. The launcher's own setpgid(0,0) (launch.py:128-129) still makes its pgid == its pid, so os.killpg(run.proc.pid, SIGINT) reaches launcher + rsg-server + scheduler once "spawned scheduler" has been logged.
  implication: The group-signal path has never been exercised by the suite. A group-SIGINT test fits naturally into test_launch_rust_e2e.py with the existing LauncherRun / make_launcher fixtures.

- timestamp: 2026-10-04T05:25:50Z
  checked: PEP 475 probe (.planning/debug/scratch/pep475_probe.py, project .venv Python 3.12.12, macOS). mp.Queue().get(timeout=T) with a non-raising SIGINT handler; the probe SIGINTs itself 0.1 s in.
  found: "timeout=0.5: get returned after 0.502s; handler ran at +0.101s" and "timeout=2.0: get returned after 2.002s; handler ran at +0.101s".
  implication: CONFIRMED. The handler runs promptly, but ready_queue.get keeps waiting for its full timeout. rsg-server, which exits in ms, is therefore always dead by the time the launcher scans children(). That makes the race deterministic.

- timestamp: 2026-10-04T05:26:25Z
  checked: Full repro, post-ready (.planning/debug/scratch/repro_group_sigint.py post 3 --new-session). Fake scheduler, real rsg-server (main checkout target/debug/rsg-server, built after the last main.rs change; worktree sources byte-identical to main). os.killpg(pgid, SIGINT) after "backend ready; handshake sent".
  found: 3/3 exit=1. Each run: "+0.000 [rsg-server] received SIGINT; exiting" -> "+1.20 rsglang.launch: rsg-server exited with code 0" -> rsg-server stderr tail dumped as a failure -> "exit code 1". No leftover processes. The 1.2 s is up to 0.2 s of the PEP 475-extended supervise get plus the report_errors(1.0) call that runs before the log line.
  implication: H1 CONFIRMED for the post-ready path (launch.py:264-275).

- timestamp: 2026-10-04T05:27:00Z
  checked: The same post-ready repro without start_new_session (how LauncherRun in the e2e suite spawns)
  found: 3/3 exit=1, same sequence. pgid == launcher pid because of the launcher's own setpgid(0,0).
  implication: An e2e test can use the existing LauncherRun unchanged and signal with os.killpg(run.proc.pid, SIGINT) after "backend ready; handshake sent".

- timestamp: 2026-10-04T05:27:20Z
  checked: Pre-ready repro (repro_group_sigint.py pre 3). SIGINT right after "spawned scheduler rank=0", while the scheduler is still booting (spawn + torch import).
  found: 3/3 exit=1 with "rsg-server exited with code 0 before ready" (or in run 1 "code -2 before ready": the SIGINT arrived before rsg-server installed its tokio handler, so it died from the default disposition). "exit code 1" appears about 1.5 s after the SIGINT (up to 0.5 s of get plus report_errors(1.0)).
  implication: The pre-ready children-scan branch (launch.py:237-242) is also affected. The fix must treat ANY child exit after a stop as part of the stop, not only rsg-server code 0.

- timestamp: 2026-10-04T05:28:30Z
  checked: Hang-mode repro (RSGLANG_FAKE_MODE=hang_before_ready, SIGINT 4.25 s after spawn so the scheduler is blocked in FakeScheduler.__init__ past imports; repro_group_sigint.py hang 5)
  found: 5/5 exit=1 with "rsglang.launch: scheduler rank 0 failed:" + a KeyboardInterrupt traceback within 1-6 ms of the SIGINT. The scheduler's pre-ready KeyboardInterrupt error envelope arrives while the launcher is in get(), and get returns it immediately. The envelope branch (launch.py:249-251) reports a failure without re-checking stop_requested. An earlier run at a 4.0 s delay gave codes [1,0,1]. The single 0 is a harness artifact: 4.0 s is an exact multiple of the 0.5 s poll period, so the SIGINT landed as a get expired and before rsg-server exited, and the top-of-loop check won. That itself shows the outcome depends only on where the signal lands relative to the blocking wait.
  implication: A third pre-ready failure branch, the error envelope, is confirmed. It is not named in CR-01. A fix that only guards the children() scan would still exit 1 here.

- timestamp: 2026-10-04T05:31:00Z
  checked: Intervention on a scratch copy of python/rsglang (worktree source untouched; git status shows only .planning/debug/ untracked). The patch is only this, in both loops:
    ready-wait:  `except queue.Empty: msg = None` / `if stop_requested: return shutdown(0)` / `if msg is None: <children scan as before> ... continue`
    supervise:   after `except queue.Empty: msg = None`, add `if stop_requested: return shutdown(0)`
  found: post 5/5 exit 0 ("exit code 0" about 0.2 s after the SIGINT), pre 5/5 exit 0 (about 0.5 s), hang 5/5 exit 0 (about 0.35 s). No failure line, no rsg-server stderr tail, no leftover processes in any run.
  implication: ROOT CAUSE CONFIRMED by intervention. The missing post-wait stop re-check alone causes all three reproduced failure branches.

- timestamp: 2026-10-04T05:33:00Z
  checked: WR-05 interaction. The scratch fake scheduler's shutdown() was made slow (RSGLANG_FAKE_SHUTDOWN_S=2) and touches status files shutdown_started / shutdown_completed. Run against the patched launcher.
  found: Group SIGINT post-ready gives exit 0 3/3, but status = [shutdown_started] only (shutdown_completed missing 3/3). The launcher's own shutdown() killpg(SIGINT) about 0.2 s later raises a second KeyboardInterrupt inside the scheduler's graceful cleanup, which backend.py's outer handler then swallows. SIGTERM to the launcher pid only gives exit 0 3/3 with status = [shutdown_started, shutdown_completed]: the scheduler receives exactly one SIGINT, from shutdown()'s killpg.
  implication: The CR-01 fix neither causes nor fixes WR-05; WR-05 is still live on the corrected path. shutdown()'s group SIGINT is REQUIRED for pid-only stops (the existing tracer test test_rust_mode_handshake_reaches_rsg_server depends on it) and HARMFUL for group stops. A WR-05 fix must keep children signalled on pid-only stops. A slow-shutdown fake with completion markers is a cheap way to make WR-05 testable. In the original-code hang runs, the "During handling of the above exception, another exception occurred: KeyboardInterrupt" traces in the scheduler's stderr fit the same second SIGINT interrupting its pre-ready exception handling (inferred).

- timestamp: 2026-10-04T05:34:00Z
  checked: Differential control. Original code, SIGTERM to the launcher pid only (PID_ONLY_SIGNAL=TERM), post-ready.
  found: 3/3 exit 0.
  implication: Same code, same timing. The only variable is whether the children receive the stop signal directly, which isolates the cause to the launcher classifying stop-induced child reactions as failures.

- timestamp: 2026-10-04T05:35:00Z
  checked: Residual windows in the suggested fix, by code reading (python/rsglang/launch.py)
  found: (1) If the signal lands between the new post-get check and children()'s rust.poll(), the window is microseconds, while rsg-server needs milliseconds to exit. A second check inside `if code is not None:` before shutdown(1) closes it race-free. The group signal is pending on the launcher before rsg-server can exit, and CPython runs the Python handler at the next eval-breaker check inside Popen.poll()/_internal_poll, before poll() returns. (2) launch.py:253-258: if a "ready" message is dequeued and the stop lands before the handshake write, a BrokenPipeError gives "rsg-server exited with code C before the handshake was sent" and shutdown(1). The post-get check covers the common case. A stop check in that except branch covers the rest. Not reproduced.
  implication: The fix should check stop both right after each get and at each shutdown(1) decision point driven by child state (children scan, envelope, BrokenPipe).

- timestamp: 2026-10-04T05:36:00Z
  checked: Test placement. python/tests/test_launch_rust_e2e.py (pytestmark = slow, LauncherRun/make_launcher fixtures, tracer stop test at L150-177, D-12 failure tests L180-240); pyproject markers; scripts/check_all.sh step 2 runs `pytest python/tests -q`, so slow tests are in the default gate.
  found: Every helper a group-SIGINT test needs already exists: LauncherRun spawns without a new session, the launcher's setpgid makes pgid == run.proc.pid, wait_for/pids/finish/text/_gone/_alive, and the fake modes ok and hang_before_ready. The cr01 repro is deterministic on current code post-ready (6/6), so it makes a reliable RED test.
  implication: The group-SIGINT e2e test belongs in python/tests/test_launch_rust_e2e.py, next to test_rust_mode_handshake_reaches_rsg_server (the stop-signal contract), not with the D-12 failure tests.

## Resolution

root_cause: "In rust mode, the launcher's ready-wait loop (python/rsglang/launch.py:232-251) and supervise loop (launch.py:261-275) check stop_requested only at the top of each iteration. After the blocking ready_queue.get returns, they classify child state as a failure without re-checking it. The SIGINT/SIGTERM handler (launch.py:135-137) only sets a flag, and PEP 475 retries the interrupted get until its full 0.5 s / 0.2 s timeout. So on a group SIGINT (terminal Ctrl-C, kill -INT -- -pgid) the children always react first. rsg-server exits 0, its contractual signal code (or -2 if it is still booting), and a pre-ready scheduler posts a KeyboardInterrupt error envelope (backend.py:103-107). The launcher then reaches shutdown(1) through one of three branches: the children() scan (L238-242 pre-ready, L271-275 post-ready), the error-envelope branch (L249-251; also L268-270 post-ready by construction), or the handshake BrokenPipe branch (L256-258, code-reading only). It prints a failure report and exits 1. AND-gate: this needs group delivery to the children (the legitimate scenario) AND failure classification before the stop re-check (the defect)."
fix: "(not applied — find_root_cause_only)"
verification: "Diagnosis verified. Reproduced 6/6 post-ready, 3/3 pre-ready during boot, 5/5 pre-ready with a hung scheduler. A pid-only SIGTERM control exits 0 3/3. A scratch patch with only the post-get stop re-check exits 0 15/15."
files_changed: []
scratch_note: "The reproduction scripts (.planning/debug/scratch/pep475_probe.py, repro_group_sigint.py) and the patched copy were throwaway and were deleted at the end of the session. To reproduce: run `python -m rsglang.launch --frontend rust --rust-bin target/debug/rsg-server --ready-timeout 60 --model Qwen/Qwen3-0.6B --dtype bfloat16 --page-size 16 --max-running-requests 8` with RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler, wait for 'backend ready; handshake sent', then os.killpg(launcher_pid, SIGINT). The result is exit 1 every time."
