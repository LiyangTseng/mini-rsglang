---
phase: 01-vendored-base-wire-codec
reviewed: 2026-10-04T00:00:00Z
depth: standard
files_reviewed: 4
files_reviewed_list:
  - python/rsglang/backend.py
  - python/rsglang/launch.py
  - python/tests/test_launch_args.py
  - python/tests/test_parent_watchdog.py
findings:
  critical: 1
  warning: 1
  info: 2
  total: 4
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-10-04T00:00:00Z
**Depth:** standard
**Files Reviewed:** 4
**Status:** issues_found

## Summary

Reviewed `backend.py` (scheduler-process entry + parent watchdog) and `launch.py`
(the `--frontend python|rust` launcher) plus their two test files. The parent-watchdog
logic (`start_parent_watchdog`) and the readiness/error-envelope protocol in
`run_scheduler` are carefully built and match their tests closely — no defects found
there. The main gap is in `_run_rust_mode`'s process lifecycle: once `rsg-server` and
the TP scheduler ranks are spawned, the function has no top-level exception guard, so
an unexpected exception (a spawn failure for one of several TP ranks, a malformed
handshake payload, etc.) propagates out of `_run_rust_mode` without the orphaned
`rsg-server` process or already-started scheduler ranks ever being killed. That
directly undermines the project's own stated D-12 goal ("benchmarks don't leak GPU
processes") and is classified Critical given how central clean process-group teardown
is to this project's benchmark-correctness story. A secondary, lower-probability
thread-safety issue exists in the stderr-tail reporting path. Two Info-level
maintainability notes round out the findings.

## Critical Issues

### CR-01: `_run_rust_mode` has no cleanup path for unexpected exceptions — can leak GPU-holding processes

**File:** `python/rsglang/launch.py:124-298` (spawn loop: `166-177`; unguarded handshake write: `268-275`)
**Issue:**
`_run_rust_mode` spawns `rsg-server` (`subprocess.Popen`, line 151) and then one
`multiprocessing.Process` per TP rank (lines 166-177), before any of `shutdown()`,
`report_errors()`, or `children()` exist as a safety net. Every *anticipated* failure
path explicitly calls `return shutdown(...)`, but there is no top-level
`try/finally` (or `try/except BaseException`) around the body, so any exception the
code does **not** anticipate leaves the already-spawned `rsg-server` process and any
already-started scheduler-rank processes running, orphaned, with nothing left to kill
them.

Concrete ways this triggers:
- The TP spawn loop itself (lines 166-177) has no `try`. If rank *i*'s
  `mp.Process(...).start()` raises (e.g. `OSError: Cannot allocate memory`, a common
  failure mode spawning several GPU processes under memory pressure), ranks
  `0..i-1` — already holding GPU/NCCL state — and the already-running `rsg-server`
  are never terminated. `run_rust_mode`'s `finally` (line 119-121) only unlinks socket
  files; it never kills a process.
- `rust.stdin.write(handshake.encode_handshake_line(payload))` (line 269) is wrapped
  only in `except BrokenPipeError` (line 271). `encode_handshake_line` (handshake.py)
  raises `ValueError` if the payload's keys don't exactly match `HANDSHAKE_KEYS` —
  that `ValueError` is not caught here, so a future drift between
  `backend.extract_handshake`'s hand-built dict and `handshake.HANDSHAKE_KEYS` (see
  IN-01) would leak processes on the very first mismatched handshake.

This is central to the project's stated design (CLAUDE.md: "Kill the Python shim and
its TP children cleanly … so benchmarks don't leak GPU processes", D-12), and a leaked
scheduler rank directly corrupts the host-RAM/cold-start benchmark scenario the whole
tool exists to measure.

**Fix:** Wrap the spawn-and-supervise body in a guard that forces a group kill before
re-raising, e.g.:
```python
rust = subprocess.Popen(...)
...
ranks: List[mp.Process] = []
try:
    for i in range(world):
        ...
        p.start()
        ranks.append(p)
        _log(f"spawned scheduler rank={i} pid={p.pid}")

    # ... existing ready-wait loop, handshake write, supervise loop ...
except BaseException:
    _log("unexpected error in launcher; killing process group")
    try:
        os.killpg(os.getpgrp(), signal.SIGKILL)
    except ProcessLookupError:
        pass
    raise
```
(or equivalently, move `shutdown`'s definition above the spawn loop and call
`shutdown(1)` from a single top-level `except BaseException` before re-raising).

## Warnings

### WR-01: `rust_tail` deque is read and mutated from different threads without synchronization

**File:** `python/rsglang/launch.py:94-100, 158-161, 210-217`
**Issue:** `_pump_rsg_stderr` (lines 94-100) runs on a background thread and calls
`tail.append(line)` on `rust_tail` (a `deque(maxlen=200)`, line 158) for as long as
`rsg-server`'s stderr stream is open. `shutdown()` only `pump.join()`s the thread when
`rust.poll() is not None` (line 210-211); if `rsg-server` is still alive when the
`_SHUTDOWN_GRACE_S` deadline is reached (e.g. it ignores `SIGINT` and the grace period
expires before the forced `SIGKILL` path), `rust.poll()` is still `None`, `pump.join()`
is skipped, and the main thread then does `lines = list(rust_tail)` (line 213) while
the pump thread may still be concurrently calling `tail.append(line)`. CPython's
`deque` can raise `RuntimeError: deque mutated during iteration` when iterated from
one thread while appended to from another; an exception here would escape
`shutdown()` uncaught and crash the launcher mid-teardown, before it reaches the
`SIGKILL` escalation a few lines below (line 219-227) — defeating the exact
forced-cleanup path WR-01 and CR-01 both depend on.
**Fix:** Snapshot without iterating the shared deque directly from another thread,
e.g. hold the tail behind a small lock, or copy defensively:
```python
try:
    lines = list(rust_tail)
except RuntimeError:
    lines = list(rust_tail)  # or protect `tail.append` / this read with a `threading.Lock`
```
A `threading.Lock` guarding both `tail.append` in `_pump_rsg_stderr` and the `list(...)`
read in `shutdown()` is the more robust fix.

## Info

### IN-01: Handshake key order is duplicated by hand in two files

**File:** `python/rsglang/backend.py:42-55` (vs. `python/rsglang/handshake.py:16-24`)
**Issue:** `extract_handshake` builds its return dict as a literal with keys typed out
in the same order as `handshake.HANDSHAKE_KEYS`, but never imports or references
`HANDSHAKE_KEYS` itself. The only thing catching a future mismatch (e.g. someone adds
a field to one side and forgets the other) is the runtime check inside
`encode_handshake_line` at send time (see CR-01's second trigger) — there's no
compile-time or import-time coupling between the two definitions of the same
contract.
**Fix:** Build the dict from `HANDSHAKE_KEYS` directly (e.g.
`dict(zip(HANDSHAKE_KEYS, (HANDSHAKE_VERSION, upstream_sha, ...)))`) or add a shared
`TypedDict`/dataclass so the two call sites can't silently drift apart.

### IN-02: Code after the self-directed `SIGKILL` is effectively dead/racy

**File:** `python/rsglang/launch.py:219-233`
**Issue:** When `shutdown()` escalates (`os.killpg(os.getpgrp(), signal.SIGKILL)`,
line 227), the launcher has joined its own process group, so this call also delivers
`SIGKILL` to the launcher itself (correctly noted in the comment on line 226). The
function nonetheless continues on to close `rust.stdin` (lines 228-232) and log the
exit code (line 233) as if execution would reliably continue — in practice, whether
those lines run at all is a race against kernel signal delivery to the calling
process. This isn't incorrect (the lines are harmless if skipped), but it reads as
intentional sequential logic when it is actually unreachable-in-practice code, which
can confuse future maintainers debugging shutdown-path logs that stop appearing.
**Fix:** Either move the `rust.stdin.close()` / final `_log` above the `SIGKILL`
escalation (closing stdin before escalating is safe) or add a short comment noting
these statements are best-effort and may not execute.

---

_Reviewed: 2026-10-04T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
