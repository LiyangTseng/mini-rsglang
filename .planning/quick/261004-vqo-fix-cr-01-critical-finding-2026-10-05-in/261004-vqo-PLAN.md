---
phase: 261004-vqo
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - python/rsglang/launch.py
  - python/tests/test_launch_rust_e2e.py
  - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
autonomous: true
requirements: [BASE-02]

estimate:
  tokens: 45000
  raw_tokens: 45000
  tasks: 2
  confidence: low

must_haves:
  truths:
    - "With --tp-size 2, when TP rank 1's start raises an unanticipated OSError after rank 0 is fully up, the launcher exits non-zero within 30 s, and rsg-server and scheduler rank 0 are both gone (CR-01 first trigger, D-12)"
    - "When handshake.encode_handshake_line raises an unanticipated ValueError after rank 0 is ready, the launcher exits non-zero within 30 s, and rsg-server and the scheduler are both gone (CR-01 second trigger, D-12)"
    - "Before the group SIGKILL, the launcher prints 'unexpected error in the launcher' and the exception's traceback, so the cause is reported (D-12: print the cause, exit non-zero)"
    - "Both regression cases fail on the unmodified launch.py: the launcher hangs in multiprocessing's exit-time join of the non-daemon rank 0, with both children alive"
    - "Anticipated paths are unchanged: every existing test in test_launch_rust_e2e.py and the whole python/tests suite still pass, and launch.py's whitespace-insensitive diff contains additions only"
    - "01-REVIEW-DISPOSITION.md records CR-01 as fixed and the frontmatter open count drops from 14 to 13. No other row changes"
  artifacts:
    - path: "python/rsglang/launch.py"
      provides: "an except BaseException guard in _run_rust_mode that covers everything after the rsg-server spawn and SIGKILLs the launcher's process group before re-raising"
      contains: "except BaseException:"
    - path: "python/tests/test_launch_rust_e2e.py"
      provides: "the CR-01 regression test, parametrized over the spawn-loop and handshake-encode triggers, plus an extra_args option on LauncherRun"
      contains: "test_unexpected_error_leaves_no_orphans"
    - path: ".planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md"
      provides: "CR-01 disposition fixed, open count 13"
      contains: "| CR-01 | critical | fixed |"
  key_links:
    - from: "python/rsglang/launch.py _run_rust_mode except BaseException guard"
      to: "every process in the launcher's process group (rsg-server, started scheduler ranks, the multiprocessing resource tracker)"
      via: "os.killpg(os.getpgrp(), signal.SIGKILL). The guard opens only after os.setpgid(0, 0) and the rsg-server Popen, so the group is always the launcher's own group. The group kill also reaches a rank that started but never got into the ranks list"
      pattern: "os\\.killpg\\(os\\.getpgrp\\(\\), signal\\.SIGKILL\\)"
    - from: "python/tests/test_launch_rust_e2e.py sitecustomize hooks"
      to: "the launcher subprocess only"
      via: "PYTHONPATH points at a tmp_path hook dir. Each hook is gated on 'rsglang.launch' in sys.orig_argv, so spawned scheduler children, the resource tracker and pytest itself are never patched"
      pattern: "rsglang\\.launch' in sys\\.orig_argv"
---

<objective>
Fix CR-01, the critical finding of the 2026-10-05 incremental code review (01-REVIEW.md). `_run_rust_mode` in python/rsglang/launch.py spawns rsg-server and the TP scheduler ranks with no top-level exception guard. Every anticipated failure path calls `return shutdown(...)`. An exception the code does not anticipate propagates out of the function, and the already-spawned processes are never killed. `run_rust_mode`'s `finally` only unlinks socket files.

Root cause, confirmed during planning by running the unmodified launcher with each failure injected (scratch probe, nothing in the repo changed):
- Handshake trigger: `encode_handshake_line` raising ValueError after rank 0 is ready escapes the narrow `except BrokenPipeError`. Python prints the traceback, then multiprocessing's atexit `_exit_function` calls `join()` on the non-daemon rank 0 forever. 15 s later the launcher was still running and rsg-server and the scheduler were both alive. The SIGINT/SIGTERM handlers are still `_request_stop`, so Ctrl-C cannot end it either. Only SIGKILL works.
- Spawn-loop trigger (--tp-size 2, rank 1's `start()` raises): if rank 0 is fully up when rank 1 fails, the result is the same hang with both children alive. If rank 0 is still unpickling its arguments, multiprocessing's priority-0 exit finalizers unlink the ready queue's named semaphore under it. Rank 0 then dies of FileNotFoundError, and the launcher exits 1 with no orphans. That is an accident of timing, not a guarantee. The regression test removes the race by having rank 1 fail only once rank 0 is up.

Fix shape: per the review and the quick-task constraints, the fix mirrors shutdown()'s existing SIGKILL escalation (launch.py L218-227) instead of inventing a new idiom. A `try` opens directly after the rsg-server Popen and covers the rest of the function. `except BaseException` prints the cause, unlinks the run sockets, flushes, `os.killpg(os.getpgrp(), signal.SIGKILL)`s the group, and then re-raises.

Purpose: D-12 says all children run in one process group, and a failure `killpg`s the whole group, prints the cause and exits non-zero. A leaked GPU-holding scheduler rank corrupts the host-RAM and cold-start benchmark scenarios.
Output: a guarded `_run_rust_mode`, a two-case e2e regression test, and CR-01 recorded as fixed.
</objective>

<execution_context>
@.claude/gsd-core/workflows/execute-plan.md
@.claude/gsd-core/templates/summary.md
</execution_context>

<context>
@.planning/STATE.md
@.planning/phases/01-vendored-base-wire-codec/01-REVIEW.md
@.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md
@python/rsglang/launch.py
@python/tests/test_launch_rust_e2e.py

<interfaces>
Current state (line numbers from HEAD 0f6fa48; read each file before editing):
- python/rsglang/launch.py:
  - L13-21 are the stdlib imports, in alphabetical order: argparse, dataclasses, os, queue, signal, subprocess, sys, threading, time. There is no `traceback` import yet.
  - `_write_stderr(text)` (L37-44) and `_log(msg)` (L47-48) already swallow OSError/ValueError from a broken stderr.
  - `run_rust_mode` (L103-121) computes `suffix = f".rsg={os.getpid()}"` and runs `return _run_rust_mode(ns, server_args, rust_bin, suffix)` in a try whose `finally` calls `sockets.unlink_run_sockets(suffix)`. The comment there reads "Every exit path the launcher survives; a group SIGKILL cleans up before it fires."
  - `_run_rust_mode(ns, server_args, rust_bin, suffix)` (L124) runs in this order:
    - function-level imports, the `_unique_suffix` replace, `sockets.unlink_run_sockets(suffix)`;
    - L135-136 `if os.getpgrp() != os.getpid(): os.setpgid(0, 0)`, after which the launcher leads its own group;
    - `upstream_sha`, the `_request_stop` SIGINT/SIGTERM handlers (L140-148);
    - L151-156 `rust = subprocess.Popen(...)`, the first child spawn;
    - L157 `_log(f"spawned rsg-server pid={rust.pid}")`, then the stderr pump thread and `mp.set_start_method`/`ready_queue`;
    - the TP spawn loop (L166-177: `mp.Process(..., name=f"rsglang-TP{i}-scheduler")`, `p.start()`, `ranks.append(p)`, `_log(f"spawned scheduler rank={i} pid={p.pid}")`);
    - the nested `children`, `report_errors` and `shutdown` defs (L179-234);
    - the ready-wait loop (L236-266), the handshake write guarded only by `except BrokenPipeError` (L268-276), and the supervise `while True:` loop (L278-298), which is the end of the function.
  - shutdown()'s escalation tail (L218-227) is the pattern to mirror:
    - `sockets.unlink_run_sockets(suffix)`;
    - `_log("escalating to SIGKILL for the process group")`;
    - a `for stream in (sys.stdout, sys.stderr):` loop doing `stream.flush()` under `except (OSError, ValueError): pass`;
    - the comment "Ends the launcher too: its exit status is the SIGKILL status, non-zero.";
    - `os.killpg(os.getpgrp(), signal.SIGKILL)`.
  - The launcher calls the handshake encoder as `handshake.encode_handshake_line(payload)` through the module imported by `from . import handshake, sockets`, so replacing the module attribute takes effect.
- python/tests/test_launch_rust_e2e.py:
  - `pytestmark = pytest.mark.slow`. `UPSTREAM_ARGS` is fixed (tp size defaults to 1).
  - `LauncherRun.__init__(self, rust_bin, status_dir, mode="ok", ready_timeout=60, extra_env=None)` runs `[sys.executable, "-m", "rsglang.launch", "--frontend", "rust", "--rust-bin", ..., "--ready-timeout", ..., *UPSTREAM_ARGS]` with STATUS_DIR_ENV (RSGLANG_FAKE_STATUS_DIR) set to status_dir, which is the test's tmp_path.
  - Helpers: `wait_for(pattern, timeout)`, `finish(timeout)` (`proc.wait`, raises subprocess.TimeoutExpired), `text()`, `lines`, `pids()` (parses "spawned rsg-server pid=N" and "spawned scheduler rank=0 pid=N"), and `cleanup()` (killpg of the launcher's pgid, plus a per-pid SIGKILL).
  - The `make_launcher` fixture builds runs and cleans every one of them up. `_gone(pid, timeout)` waits for a pid to disappear.
  - test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans (L293-316) shows the established test-only sitecustomize pattern: it writes `tmp_path/"slowboot"/"sitecustomize.py"`, joins PYTHONPATH as `os.pathsep.join(p for p in (str(dir), os.environ.get("PYTHONPATH")) if p)`, and passes it via `extra_env`.
  - test_group_sigint_clean_stop forbids "escalating to SIGKILL" in the launcher's own lines for clean stops. The new guard never runs on those paths.
- python/rsglang/testing/fake_scheduler.py: in "ok" mode the primary rank, after ready, touches `<RSGLANG_FAKE_STATUS_DIR>/detok_peer_connected` once rsg-server's detok peer is reachable. Non-primary ranks just loop. `sync_all_ranks` is a no-op, so `--tp-size 2` works on the Mac.
- 01-REVIEW-DISPOSITION.md: the frontmatter has `  - id: CR-01` / `    severity: critical` / `    disposition: open` / `    title: ...` (L58-61) and `open: 14` (L102). The table row (L137) starts `| CR-01 | critical | open | python/rsglang/launch.py:124-298 ...`. A `|` inside a Source cell is escaped by the gate, so Source text must not contain `|`.
</interfaces>
</context>

<tasks>

<task type="tracer" tdd="true">
  <name>Task 1: Tracer — an unexpected launcher error SIGKILLs the whole process group and leaves no orphans (CR-01)</name>
  <precondition>.venv/bin/python exists in this checkout and imports rsglang from this checkout (.venv/bin/python -c "import rsglang; print(rsglang.__file__)" prints a path under this checkout's python/ directory); otherwise run bash scripts/bootstrap_mac_env.sh first (idempotent, worktree-safe)</precondition>
  <files>python/tests/test_launch_rust_e2e.py, python/rsglang/launch.py</files>
  <read_first>
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW.md (section "CR-01: `_run_rust_mode` has no cleanup path for unexpected exceptions")
    - python/rsglang/launch.py (whole file, especially _run_rust_mode and shutdown()'s escalation tail at L218-227)
    - python/tests/test_launch_rust_e2e.py (whole file, especially LauncherRun, make_launcher and test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans)
    - python/rsglang/testing/fake_scheduler.py (run_forever: where detok_peer_connected is touched)
  </read_first>
  <behavior>
    - Case spawn_loop: the launcher runs with extra upstream args `--tp-size 2`. A launcher-only hook makes the start of the process named "rsglang-TP1-scheduler" wait until `<status_dir>/detok_peer_connected` exists (so rank 0 is fully up) and then raise OSError("injected: TP rank 1 failed to start"). The test waits for "injected: ". The launcher must then exit within 30 s with a non-zero code (-9 from the group SIGKILL). Its own "rsglang.launch:" lines must include "unexpected error in the launcher". pids() must be exactly {"rsg-server", "scheduler"}, and both pids must be gone within 15 s.
    - Case handshake_encode: no extra args (tp size 1). A launcher-only hook replaces rsglang.handshake.encode_handshake_line with a function that raises ValueError("injected: handshake keys drifted"). The assertions are the same as for spawn_loop.
    - RED on the unmodified launch.py: both cases fail at the 30 s finish with the "launcher still running" failure. The launcher hangs in multiprocessing's exit-time join of rank 0, and both children stay alive (this was observed during planning).
    - GREEN: both cases pass. All the existing tests in test_launch_rust_e2e.py and test_launch_args.py still pass. The anticipated failure and stop paths behave as before because the guard never runs on them.
  </behavior>
  <action>
RED first, in python/tests/test_launch_rust_e2e.py:
1. Give LauncherRun.__init__ one new keyword parameter, `extra_args: list[str] | None = None`. Append `*(extra_args or [])` to the launcher command after `*UPSTREAM_ARGS`. Change nothing else in LauncherRun or the fixtures.
2. Add two module-level string constants, each holding the source of a test-only sitecustomize.py. Gate both on the string "rsglang.launch" being an element of sys.orig_argv. That way they patch only the launcher process, and never a spawned `--multiprocessing-fork` child, the resource tracker or pytest.
   - Spawn-loop hook. Import multiprocessing.process and keep a reference to the original BaseProcess.start. Replace BaseProcess.start with a wrapper:
     - When self.name equals "rsglang-TP1-scheduler", poll for up to 60 s, sleeping 50 ms each time, until the file detok_peer_connected exists in the directory named by the RSGLANG_FAKE_STATUS_DIR environment variable. Then raise OSError("injected: TP rank 1 failed to start").
     - For any other process, call the original method.
     - The wait is load-bearing, so put a short comment on it in the test. Without the wait, rank 0 is usually still unpickling its arguments when the error escapes. multiprocessing's exit finalizers then unlink the ready queue's semaphore under it, and the unfixed launcher exits 1 with no orphans by accident. The case would not be RED (seen during planning).
   - Handshake hook. Import rsglang.handshake and replace its encode_handshake_line attribute with a function that raises ValueError("injected: handshake keys drifted").
3. Add `test_unexpected_error_leaves_no_orphans(make_launcher, tmp_path, hook, extra_args)`, parametrized over (spawn-loop hook, ["--tp-size", "2"]) and (handshake hook, []) with ids "spawn_loop" and "handshake_encode". Give it a one-line comment naming CR-01 and D-12. The steps:
   1. Write the hook source to tmp_path/"hook"/"sitecustomize.py". Build PYTHONPATH with the same os.pathsep.join idiom as the slow-boot test.
   2. Create the run with `make_launcher(extra_env={"PYTHONPATH": ...}, extra_args=extra_args)`.
   3. `run.wait_for("injected: ", 90)`. The injected exception has fired; its text reaches stderr through a traceback, both before and after the fix.
   4. Call `run.finish(30)` inside a try. On subprocess.TimeoutExpired, call pytest.fail with a message saying the launcher is still running 30 s after the injected error and its children were never killed (CR-01), followed by run.text().
   5. Assert the exit code is non-zero. Comment that it is -9, because the group SIGKILL ends the launcher too.
   6. Assert that some line starting with "rsglang.launch:" contains "unexpected error in the launcher".
   7. Assert `set(run.pids()) == {"rsg-server", "scheduler"}`.
   8. For each child, assert `_gone(pid, 15)` with a "pid orphaned" message that includes run.text().
   Do not assert socket-file cleanup. In the spawn_loop timing, rsg-server can bind its socket in the instant between the guard's unlink and the SIGKILL, and CR-01 is about processes. Do not refactor the existing slow-boot test's inline hook. Its IN-13 caveat stays open.
4. Run `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py -k unexpected_error -q`. Confirm that both cases fail on the unmodified launch.py with the "still running" failure (each takes about 30 s, and the fixture cleanup kills the hung group). Record the RED output in the SUMMARY, then commit the test alone (test(261004-vqo): ...) on the current feature branch, never main.

GREEN, per D-12 (the launcher owns failure handling, killpg's the whole group, prints the cause and exits non-zero) and CR-01. In python/rsglang/launch.py:
1. Add `import traceback` to the stdlib import block, keeping alphabetical order (after `import time`).
2. In _run_rust_mode, open `try:` on the line directly after the `rust = subprocess.Popen(...)` statement. Move every following statement of the function into it by indentation only, from `_log(f"spawned rsg-server pid={rust.pid}")` through the end of the supervise `while True:` loop, including the nested children/report_errors/shutdown defs. Every moved line must stay byte-identical apart from its leading whitespace.
   - Keep the Popen and everything before it outside the guard. Before the setpgid at L135-136, `os.getpgrp()` is still the invoking shell's or pytest's group, and a group SIGKILL there would kill the caller.
   - If Popen itself fails, nothing has been spawned. Python's own traceback and exit 1 are already correct there.
3. Add `except BaseException:` at the end of the function. Its body mirrors shutdown()'s escalation tail:
   - An inner `try:` holds the diagnostic and cleanup steps, in this order:
     1. `_log("unexpected error in the launcher; escalating to SIGKILL for the process group")`.
     2. `_write_stderr(traceback.format_exc())`. The cause must be printed now, because the SIGKILL ends the launcher before Python could print the traceback itself.
     3. `sockets.unlink_run_sockets(suffix)`. The SIGKILL also skips run_rust_mode's finally; this is the same reason shutdown() unlinks before it escalates.
     4. The same sys.stdout/sys.stderr flush loop, with `except (OSError, ValueError): pass`.
   - The inner try's `finally:` runs `os.killpg(os.getpgrp(), signal.SIGKILL)`, with the same comment as in shutdown() ("Ends the launcher too: its exit status is the SIGKILL status, non-zero."). This way no failure in a diagnostic step can skip the kill.
   - After the inner try comes a bare `raise`, with a short comment that the exception is never swallowed if the launcher outlives its own SIGKILL.
   - The group kill reaches every started rank, even one that never got into `ranks` (for example a start() that raised after forking). That is why the guard kills the group rather than iterating `ranks`.
4. Change nothing else. shutdown(), run_rust_mode's finally and its comment, and all anticipated paths stay as they are. WR-01, IN-01 and IN-02 stay open: do not fix them here. The guard happens to also catch a WR-01 RuntimeError escaping shutdown(), but WR-01's own fix is a separate finding. Do not touch vendor/.
5. Before committing, run `git diff -w --numstat -- python/rsglang/launch.py` and confirm 0 deleted lines (the edit is pure additions apart from indentation). Run the verify command, then commit the fix (fix(261004-vqo): ...).
  </action>
  <verify>
    <automated>.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py python/tests/test_launch_args.py -q && test -z "$(git status --porcelain vendor/)"</automated>
    <fails_when>non-zero exit: any failed or errored test (including both new parametrized cases and every pre-existing e2e test), or any change under vendor/</fails_when>
  </verify>
  <acceptance_criteria>
    - Both new cases were observed failing on the unmodified launch.py with the "still running" failure, and the SUMMARY records that RED output
    - `.venv/bin/python -m pytest python/tests/test_launch_rust_e2e.py python/tests/test_launch_args.py -q` exits 0, including test_unexpected_error_leaves_no_orphans[spawn_loop] and [handshake_encode]
    - `grep -c "except BaseException:" python/rsglang/launch.py` prints 1
    - `grep -c "os.killpg(os.getpgrp(), signal.SIGKILL)" python/rsglang/launch.py` prints 2 (shutdown() and the new guard)
    - `grep -c "unexpected error in the launcher" python/rsglang/launch.py` prints 1, and `grep -cx "import traceback" python/rsglang/launch.py` prints 1
    - `awk '/os.setpgid\(0, 0\)/{s=NR} /rust = subprocess.Popen\(/{p=NR} p && !t && /^ *try:$/{t=NR} /spawned rsg-server pid=/{l=NR} /except BaseException:/{e=NR} END{exit !(s && p && t && l && e && s < p && p < t && t < l && l < e)}' python/rsglang/launch.py` exits 0 (setpgid, then Popen, then the guard's try, then the moved spawn log, then the except)
    - Before the fix commit, `git diff -w --numstat -- python/rsglang/launch.py` reported 0 deleted lines
    - `git status --porcelain vendor/` prints nothing
  </acceptance_criteria>
  <done>An exception that escapes the rust-mode spawn loop or the handshake write no longer leaves rsg-server or a scheduler rank running. The launcher prints the cause, SIGKILLs its process group and exits non-zero (-9). A two-case regression test proves it and failed before the fix. Every anticipated path behaves exactly as before.</done>
</task>

<task type="auto">
  <name>Task 2: Record CR-01 as fixed and run the full Python regression suite</name>
  <precondition>Task 1's fix commit is present: grep -c "except BaseException:" python/rsglang/launch.py prints 1</precondition>
  <files>.planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md</files>
  <read_first>
    - .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md (frontmatter CR-01 entry at L58-61, `open:` at L102, the CR-01 table row at L137, footer rules at L149-151)
  </read_first>
  <action>
In .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md change exactly three lines and nothing else:
- In the frontmatter `findings:` list, the CR-01 entry's `    disposition: open` becomes `    disposition: fixed`. Its id, severity and title stay unchanged.
- The frontmatter `open: 14` becomes `open: 13`. `total: 24` and `recorded:` stay as they are.
- The CR-01 table row becomes:
  `| CR-01 | critical | fixed | .planning/quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/261004-vqo-PLAN.md (quick task 261004-vqo): everything in _run_rust_mode after the rsg-server spawn runs inside try/except BaseException, which prints the traceback, unlinks the run sockets and SIGKILLs the launcher's process group before re-raising; regression test test_unexpected_error_leaves_no_orphans covers the spawn-loop and handshake-encode triggers |`
  The Source text must contain no `|` of its own.
Leave every other row, every other frontmatter entry, the ID-reuse notice and the footer untouched. WR-01, IN-01 and IN-02 stay `open`. Before committing, `git diff --numstat` on this file must report exactly 3 added and 3 deleted lines.
Then run the full Python suite (`.venv/bin/python -m pytest python/tests -q`, the check_all.sh step-2 command) as the regression gate, and confirm vendor/ is untouched. Commit the disposition change (docs(261004-vqo): ...) on the current feature branch, never main.
  </action>
  <verify>
    <automated>grep -q '| CR-01 | critical | fixed | .planning/quick/261004-vqo-' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md && grep -qx 'open: 13' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md && test "$(awk '/^  - id: CR-01$/{f=1;next} f && /disposition:/{print $2; exit}' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md)" = fixed && .venv/bin/python -m pytest python/tests -q && test -z "$(git status --porcelain vendor/)"</automated>
    <fails_when>non-zero exit: the CR-01 row or frontmatter entry is not fixed, open is not 13, any test in python/tests fails or errors, or vendor/ changed</fails_when>
  </verify>
  <acceptance_criteria>
    - `grep -c '| CR-01 | critical | fixed |' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` prints 1
    - `grep -cx 'open: 13' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` prints 1
    - The awk lookup of the CR-01 frontmatter entry's disposition prints `fixed`
    - `grep -c '| WR-01 | warning | open |' .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` prints 1 (untouched neighbour)
    - Before the docs commit, `git diff --numstat -- .planning/phases/01-vendored-base-wire-codec/01-REVIEW-DISPOSITION.md` reported `3	3`
    - `.venv/bin/python -m pytest python/tests -q` exits 0
    - `git status --porcelain vendor/` prints nothing
  </acceptance_criteria>
  <done>CR-01 is recorded as fixed, with a Source cell citing this plan and describing the fix. The open count is 13, nothing else in the ledger changed, the whole Python suite passes and vendor/ is untouched.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| launcher → its child processes (same host, same user) | The launcher owns the lifetime of rsg-server and the scheduler ranks it spawns into its own process group (D-12). An orphaned rank holds GPU memory and corrupts benchmark measurements |
| test hook → launcher subprocess | A test-only sitecustomize.py, injected through PYTHONPATH from pytest's tmp_path into the e2e test's launcher subprocess only |

## STRIDE Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation Plan |
|-----------|----------|-----------|----------|-------------|-----------------|
| T-261004-vqo-01 | Denial of Service | python/rsglang/launch.py `_run_rust_mode` unexpected-exception path | high | mitigate | An `except BaseException` guard after the rsg-server Popen SIGKILLs the launcher's process group before re-raising (Task 1). test_unexpected_error_leaves_no_orphans asserts that rsg-server and the scheduler are gone for both CR-01 triggers |
| T-261004-vqo-02 | Denial of Service | guard placement relative to `os.setpgid(0, 0)` | high | mitigate | The guard opens only after the Popen, which runs after setpgid, so `os.getpgrp()` is always the launcher's own group and the kill can never reach the invoking shell's or pytest's group. Gated by the Task 1 awk ordering check (setpgid < Popen < try < spawn log < except) |
| T-261004-vqo-03 | Denial of Service | a diagnostic step inside the guard raising and skipping the kill | medium | mitigate | The log, traceback, socket unlink and flush run in an inner try whose `finally` performs the killpg |
| T-261004-vqo-04 | Information Disclosure | traceback printed to the launcher's stderr | low | accept | Python prints the same content for any uncaught exception. It goes only to the local operator's terminal or log |
| T-261004-vqo-05 | Denial of Service | named POSIX semaphores and the multiprocessing resource tracker are killed by the group SIGKILL | low | accept | Identical to shutdown()'s existing SIGKILL escalation. Outside CR-01's scope |
| T-261004-vqo-06 | Tampering | test sitecustomize on PYTHONPATH | low | accept | Test-only, written under pytest's tmp_path, and gated to the launcher process by `sys.orig_argv`. The shadowing caveat (IN-13) stays open and is outside this plan's scope |
| T-261004-vqo-SC | Tampering | npm/pip/cargo installs | low | accept | This plan installs and changes no packages, so no package-legitimacy gate applies |
</threat_model>

<verification>
- `.venv/bin/python -m pytest python/tests -q` exits 0. This includes both test_unexpected_error_leaves_no_orphans cases and every pre-existing launcher test (the clean SIGINT stops still exit 0 with no "escalating to SIGKILL" line).
- The RED evidence for both cases (the launcher still running 30 s after the injected error on the unmodified launch.py) is recorded in the SUMMARY.
- `git diff -w` of python/rsglang/launch.py relative to the RED commit is additions only.
- `git status --porcelain vendor/` prints nothing.
- 01-REVIEW-DISPOSITION.md: CR-01 is `fixed` in both the frontmatter and the table, `open: 13`, and no other row changed.
</verification>

<success_criteria>
- An unanticipated exception after rsg-server is spawned (spawn-loop OSError or handshake-encode ValueError) makes the launcher print the cause, SIGKILL its process group and exit non-zero, and leaves no rsg-server or scheduler process behind (CR-01 closed, D-12 upheld).
- The regression test failed before the fix and passes after it.
- No anticipated path changed behaviour, and the whole Python suite passes.
- CR-01 is recorded as fixed, with the open count going from 14 to 13.
</success_criteria>

<output>
Create `.planning/quick/261004-vqo-fix-cr-01-critical-finding-2026-10-05-in/261004-vqo-SUMMARY.md` when done
</output>
