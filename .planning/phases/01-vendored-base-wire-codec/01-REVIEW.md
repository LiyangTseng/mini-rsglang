---
phase: 01-vendored-base-wire-codec
reviewed: 2026-10-03T00:00:00Z
depth: standard
files_reviewed: 6
files_reviewed_list:
  - python/rsglang/backend.py
  - python/rsglang/launch.py
  - python/rsglang/testing/fake_scheduler.py
  - python/tests/test_launch_rust_e2e.py
  - python/tests/test_parent_watchdog.py
  - scripts/gpu_phase1_check.sh
findings:
  critical: 0
  warning: 5
  info: 4
  total: 9
status: issues_found
---

# Phase 1: Code Review Report (incremental, after gap-closure plans 01-07 and 01-08)

**Reviewed:** 2026-10-03
**Depth:** standard
**Files Reviewed:** 6
**Status:** issues_found

> **Finding IDs (orchestrator note):** this is an incremental review of the 01-07 and 01-08 gap-closure changes. Its findings are numbered after the previous review (commit 261f8ee: CR-01, WR-01..WR-05, IN-01..IN-09), so IDs stay unique in `01-REVIEW-DISPOSITION.md`. In the Summary, `CR-01` and `WR-02` refer to that previous review. WR-10 restates the previous WR-03, and IN-12 restates the previous IN-08.

## Summary

The CR-01 fix (re-checking `stop_requested` after each blocking `ready_queue.get` and before reporting a child exit) is logically sound in both the ready-wait and supervise loops, and the shutdown path stays consistent. The WR-02 fix (launcher pid passed at spawn time, PR_SET_PDEATHSIG armed first, then a getppid re-check, then polling) has correct ordering and covers the reparent-before-prctl race. No blockers were found. The remaining defects are robustness gaps: the prctl failure path, a verification script that can report a false PASS on the GPU-orphan check, a startup race in the script's `start_session`, and a unit test that cannot distinguish its intended exit from an import failure.

## Warnings

### WR-06: A prctl failure kills the scheduler before the error envelope and for a mere backstop

**File:** `python/rsglang/backend.py:77-80, 100`
**Issue:** `start_parent_watchdog` is called at `run_scheduler` line 100, outside the `try` that posts `{"kind": "error"}` on the ready queue. If `prctl(PR_SET_PDEATHSIG)` fails (e.g. a seccomp-restricted container), the `OSError` escapes. The launcher then only sees "scheduler exited with code 1 before ready" with no traceback envelope. It also turns a defence-in-depth feature into a hard startup failure, even though the polling thread alone would still protect against a dead launcher.
**Fix:** Degrade to polling and log, rather than raise:
```python
if sys.platform.startswith("linux"):
    try:
        libc = ctypes.CDLL(None, use_errno=True)
        if libc.prctl(PR_SET_PDEATHSIG, int(signal.SIGKILL), 0, 0, 0) != 0:
            raise OSError(ctypes.get_errno(), "prctl(PR_SET_PDEATHSIG) failed")
    except OSError as exc:
        print(f"rsglang: PDEATHSIG unavailable ({exc}); using polling watchdog only", file=sys.stderr)
```
Also declare `libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]` so the variadic arguments are passed at full width.

### WR-07: `gpu_pids` failures and SIGPIPE turn the GPU-orphan check into a false PASS

**File:** `scripts/gpu_phase1_check.sh:97, 219-221, 228, 263-265, 272`
**Issue:** `gpu_pids` swallows nvidia-smi errors (`2>/dev/null`), so a failing nvidia-smi yields an empty list and `! gpu_pids | grep -qx ...` reports "not on the GPU". Under `set -o pipefail`, `gpu_pids | grep -qx` can also return the SIGPIPE status (141) of the writer when `grep -q` exits early on a match. In `if gpu_pids | grep -qx "$pid"` (lines 228, 272) that makes a pid that is still listed look absent. The `ps -p` checks catch most real orphans, but this check exists specifically to confirm that GPU memory was released, and it can silently pass.
**Fix:** Capture once and test the captured text:
```bash
gpu_pids() { nvidia-smi --query-compute-apps=pid --format=csv,noheader | tr -d ' '; }  # let failure propagate
on_gpu() { local out; out="$(gpu_pids)" || { echo "nvidia-smi failed" >&2; return 2; }; grep -qx "$1" <<<"$out"; }
```
and treat return code 2 as a step failure.

### WR-08: `start_session` has a 0.5 s startup race that can fail a healthy run

**File:** `scripts/gpu_phase1_check.sh:85-92`
**Issue:** After backgrounding the python-then-`setsid` exec chain, the script sleeps a fixed 0.5 s and then requires `pgid == BG_PID`. If interpreter startup plus `exec setsid` takes longer (cold disk, loaded box), the process still has the script's pgid and the helper wrongly reports "setsid did not exec in place". The failure is also unhandled in `step4_early` (the early return leaves a started session until the EXIT trap).
**Fix:** Poll until the pgid matches or a timeout elapses:
```bash
for _ in $(seq 1 50); do
  pgid="$(ps -o pgid= -p "$BG_PID" 2>/dev/null | tr -d ' ')"
  [ "$pgid" = "$BG_PID" ] && break
  sleep 0.1
done
[ "$pgid" = "$BG_PID" ] || { echo "setsid did not exec in place ..." >&2; return 1; }
```

### WR-09: `test_exits_at_once_when_parent_is_not_the_launcher` cannot tell a watchdog exit from any other exit 1

**File:** `python/tests/test_parent_watchdog.py:27-39`
**Issue:** The test asserts only `returncode == 1`, no "survived", and elapsed < 5. A child that fails to import `rsglang.backend` (missing install, ImportError, any traceback) also exits 1 within 5 s with no "survived", so the test passes without the watchdog ever running. There is no conftest or sys.path setup, so importability depends on the environment.
**Fix:** Also assert the child died of the watchdog and not a traceback, e.g. `assert "Traceback" not in result.stderr`, and have the child print a marker (`print('armed', flush=True)`) after `start_parent_watchdog` returns in the `is_the_launcher` variant, or assert the exit happened after the call by writing to a file before `time.sleep(30)`.

### WR-10: `setpgid(0, 0)` moves the launcher out of the terminal's foreground group when run under a wrapper

**File:** `python/rsglang/launch.py:128-129`
**Issue:** When the launcher is not its own group leader (run via `uv run`, `make`, or as a non-first pipeline member such as `... | tee`), `os.setpgid(0, 0)` creates a new group that is not the terminal's foreground group. A terminal Ctrl-C then reaches the wrapper's group, not the launcher or its children, so the D-12/CR-01 "Ctrl-C exits 0" contract silently does not apply, and the launcher is a background group for tty purposes. The docstring and tests assume the launcher already leads its group (they run it via `Popen`/`setsid`), so this path is untested.
**Fix:** Document the limitation, or handle it: when stdin/stderr is a tty and the launcher is not the group leader, either refuse with a message ("run the launcher directly or under setsid") or `os.tcsetpgrp(tty_fd, os.getpgrp())` after `setpgid` (with SIGTTOU ignored around the call). Add a test for the wrapped case.

## Info

### IN-10: Hard-coded Qwen3-specific assertions in a script that accepts `--model`

**File:** `scripts/gpu_phase1_check.sh:194, 196-197`
**Issue:** `max_running_req == 256` and `max_seq_len <= 40960` are checked regardless of `--model`, while only the eos check is gated on the default model. Any other model can fail step 3 spuriously.
**Fix:** Gate these two checks on `[ "$MODEL" = "$DEFAULT_MODEL" ]` as the eos check is, or relax them to sanity bounds.

### IN-11: Repeated stop_requested boilerplate in the launch loops

**File:** `python/rsglang/launch.py:233-250, 272-288`
**Issue:** The same "check stop_requested, then shutdown(0)" pattern is repeated five or six times, which is how CR-01 slipped in originally. A future branch can easily miss one.
**Fix:** Fold it into a small helper (e.g. `def stopped(): return shutdown(0) if stop_requested else None`) or restructure so every exit decision runs after a single post-`get` check.

### IN-12: Test cleanup can signal a reused pid or process group

**File:** `python/tests/test_launch_rust_e2e.py:122-131`
**Issue:** `cleanup` calls `os.killpg(self.proc.pid, SIGKILL)` and `os.kill(<child pid>, SIGKILL)` unconditionally, even after the launcher and children have exited and been reaped. A reused pid/pgid would receive SIGKILL. The risk is small but the blast radius on a developer machine is real.
**Fix:** Only signal when `self.proc.poll() is None`, and for children check liveness and that the pid still belongs to the run's process group (`os.getpgid(pid) == self.proc.pid`) before killing.

### IN-13: The slow-boot test's `sitecustomize` can shadow an existing one and relies on `sys.orig_argv`

**File:** `python/tests/test_launch_rust_e2e.py:298-306`
**Issue:** Prepending a `sitecustomize.py` directory to PYTHONPATH shadows any site-provided `sitecustomize` (e.g. coverage or venv hooks) in every Python subprocess of the run. `sys.orig_argv` also needs Python 3.10, which matches `requires-python >=3.10` but leaves no margin. A missing marker would make the test pass vacuously, because the window would not be widened and the kill might land after boot.
**Fix:** Have the `sitecustomize` touch a marker file in `tmp_path` when it sleeps, and assert the marker exists before killing the launcher, so the test fails loudly if the widening did not take effect.

---

_Reviewed: 2026-10-03_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
