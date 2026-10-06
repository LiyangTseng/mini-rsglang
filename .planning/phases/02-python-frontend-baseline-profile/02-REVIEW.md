---
phase: 02-python-frontend-baseline-profile
reviewed: 2026-10-05T00:00:00Z
depth: standard
files_reviewed: 19
files_reviewed_list:
  - docs/benchmarks/baseline-profile.md
  - python/rsglang/profiling/__init__.py
  - python/rsglang/profiling/analysis.py
  - python/rsglang/profiling/hook.py
  - python/rsglang/profiling/procs.py
  - python/rsglang/profiling/scenarios.py
  - python/rsglang/profiling/session.py
  - python/rsglang/profiling/sidecar.py
  - python/rsglang/testing/fake_profile_env.py
  - python/tests/test_baseline_profile.py
  - python/tests/test_baseline_profile_report.py
  - python/tests/test_gpu_profile_script.py
  - python/tests/test_profile_analysis.py
  - python/tests/test_profile_hook.py
  - python/tests/test_profile_scenarios.py
  - python/tests/test_profile_sidecar.py
  - requirements-mac.in
  - requirements-mac.txt
  - scripts/baseline_profile.py
  - scripts/gpu_phase2_profile.sh
findings:
  critical: 4
  warning: 7
  info: 1
  total: 12
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-10-05
**Depth:** standard
**Files Reviewed:** 19
**Status:** issues_found

## Summary

This phase adds a large, deliberately careful profiling harness (in-process GC/
tracemalloc hook, process discovery/role-ID/teardown, py-spy-based CPU/GIL/radix
analysis, three scenario drivers, a validated JSON sidecar schema, and a GPU-only
shell wrapper). The two previously-known late fixes called out for this review —
`analysis.load_speedscope`'s `errors="replace"` decode of py-spy's possibly-invalid
UTF-8 frame names, and `gpu_phase2_profile.sh`'s `PATH` prepend for WSL/micromamba —
are both sound as implemented and are not re-flagged below.

Despite the overall care (atomic sidecar writes, extensive null-vs-zero schema
invariants, `shlex.join`-based hyperfine argv construction, careful `finally`-based
teardown structure), this pass found four BLOCKER-level defects, three of which sit
exactly in the areas this review was asked to scrutinize hardest:

1. The one function this whole phase's threat model is built around —
   `procs.teardown()` — catches a narrower set of signal-delivery exceptions than its
   own sibling helper (`scenarios._kill_group()`), which explicitly documents and
   guards against the exact failure it's missing.
2. `scripts/baseline_profile.py run`'s own documented exit-code contract ("2 ... a
   py-spy permission error") is silently violated because the `run` subcommand's
   exception handling doesn't catch `procs.PySpyPermissionError`, unlike `discover`'s
   otherwise-identical code path.
3. A numeric null-handling gap in `analysis.cpu_metrics` is masked by half of its own
   zero-guard (`ticks <= 0`) but not the other half (`rate_hz` as a bare divisor),
   producing an unhandled `ZeroDivisionError`.
4. A systemic "only `except OSError`" pattern around `subprocess.run(..., timeout=N)`
   calls, several of which run *after* a full (potentially hours-long) measurement
   session has already completed and before its results are persisted to disk.

## Critical Issues

### CR-01: `procs.teardown()` doesn't handle the documented EPERM-on-reused-pgid race, unlike its sibling in `scenarios.py`

**File:** `python/rsglang/profiling/procs.py:217-255` (specifically lines 221-223 and 235-237)
**Issue:** `teardown()` is the function this phase's own threat model calls out by
name ("a leaked GPU-holding process is explicitly called out as a high-severity
threat"), and it is used for every real profiling session (`session.run_session`'s
`finally`) and for `discover` (`scripts/baseline_profile.py`'s `cmd_discover`
`finally`). Both of its `os.killpg()` calls only catch `ProcessLookupError`:

```python
try:
    os.killpg(pgid, signal.SIGINT)
except ProcessLookupError:
    pass
...
try:
    os.killpg(pgid, signal.SIGKILL)
except ProcessLookupError:
    pass
```

Compare this to `scenarios._kill_group()` (used by the cold-start scenario's own
teardown path), which handles exactly this signal with a comment explaining why:

```python
# Once the group leader has already exited, the pgid number can be
# reclaimed by the OS; a follow-up signal to it then raises EPERM rather
# than ESRCH (observed on macOS). Either means "nothing left to signal".
try:
    os.killpg(pgid, signal.SIGINT)
except (ProcessLookupError, PermissionError):
    return
```

`scenarios.coldstart_stop()` and `python/tests/test_launch_rust_e2e.py:127` use the
same `(ProcessLookupError, PermissionError)` pair. `procs.teardown()` is the odd one
out. If this race fires (documented as "observed on macOS" — i.e. on the exact
platform this project develops and tests on per `.claude/CLAUDE.md`'s "dev machine is
a Mac"), an uncaught `PermissionError` propagates out of `teardown()`. Since
`teardown()` is itself invoked from inside a `finally:` block in both callers, this
exception either replaces an exception already in flight or propagates as a fresh
one — and critically, the SIGKILL step, the IPC-socket-file cleanup, and the final
liveness check never run, leaving the profiled server's process group (which may
still be holding GPU memory) alive and undetected.
**Fix:**
```python
    try:
        os.killpg(pgid, signal.SIGINT)
    except (ProcessLookupError, PermissionError):
        pass
    ...
    try:
        os.killpg(pgid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
```

### CR-02: `scripts/baseline_profile.py run` silently drops its own documented py-spy-permission exit code

**File:** `scripts/baseline_profile.py:382-390` (module docstring at lines 19-23)
**Issue:** The module docstring promises: "Exit codes: ... 2 environment error
(py-spy missing from PATH, or a py-spy permission error)." `cmd_discover`'s
`run_body()` honors this by explicitly catching `procs.PySpyPermissionError` around
its `py_spy_dump` calls (line 176) and returning 2 with the remediation message.
`cmd_run`, however, calls `session.run_session(...)` / `session.run_coldstart(...)`,
both of which call `procs.py_spy_dump()` internally with **no** try/except around it
(`session.py:329`), so a denied py-spy attach raises `PySpyPermissionError` straight
through `run_session`'s `finally` (teardown still runs — good) and up into `cmd_run`'s
own exception handler:

```python
except (
    procs.ServerExited,
    TimeoutError,
    procs.RoleError,
    session.MeasurementError,
    analysis.SpeedscopeError,
) as exc:
    print(str(exc), file=sys.stderr)
    return 1
```

`procs.PySpyPermissionError` is not in this tuple, so it is never caught here (or
anywhere above `main()`). The operator gets a raw Python traceback and an
implicit exit code of 1 (an uncaught exception's default) instead of the documented,
actionable "2 ... remediation options: setcap / --py-spy-sudo / ptrace_scope" message
that `discover` gives for the identical underlying condition. This is exactly the
failure an operator is likely to hit in practice: `scripts/gpu_phase2_profile.sh`
runs `discover` (step 1) before `run` (step 2) using the *same* py-spy privilege
setup, so a privilege grant that silently expires or is scoped incorrectly between
steps 1 and 2 surfaces as a clean, documented error in step 1 but an unhandled
traceback in step 2. No test exercises this path (contrast with
`test_discover_permission_denied_exits_2`, which has no `run`-subcommand analog).
**Fix:**
```python
    except (
        procs.ServerExited,
        TimeoutError,
        procs.RoleError,
        procs.PySpyPermissionError,
        session.MeasurementError,
        analysis.SpeedscopeError,
    ) as exc:
        print(str(exc), file=sys.stderr)
        return 2 if isinstance(exc, procs.PySpyPermissionError) else 1
```

### CR-03: `analysis.cpu_metrics`'s `per_request_ms` divides by `rate_hz` with no zero-guard

**File:** `python/rsglang/profiling/analysis.py:195-245` (division at lines 228-230)
**Issue:** The function's own docstring and `cpu_active_pct`/`gil_held_pct` logic
correctly treat `rate_hz * window_s <= 0` as a reason to return `None` instead of
dividing:

```python
ticks = rate_hz * window_s
cpu_active_pct = 100 * active_samples / ticks if ticks > 0 else None
```

But the per-bucket `per_request_ms` computation uses `rate_hz` as a bare divisor,
guarded only by `requests_completed > 0`, never by `rate_hz > 0`:

```python
per_request_ms = (
    1000 * count / rate_hz / requests_completed if requests_completed > 0 else None
)
```

`rate_hz` is a plain CLI integer (`run.add_argument("--py-spy-rate", type=int,
default=100, ...)` in `scripts/baseline_profile.py`) with no `>= 1` enforcement at
parse time — `sidecar._validate_cpu_role` only enforces `rate_hz >= 1` on the
*already-written* output document, which is too late to prevent this crash.
`--py-spy-rate 0` (or any future caller passing `rate_hz=0`, e.g. a refactor that
forwards the `discover` subcommand's placeholder `rate_hz=0` into a code path that
calls `cpu_metrics`) raises `ZeroDivisionError: division by zero` inside
`build_scenario_entry`, which runs only *after* `run_session`'s entire scenario has
already executed (server launched, workload run for the full `duration_s`/
`requests`, py-spy recorders stopped) — so the crash destroys a fully-measured,
potentially long-running scenario's results before they are ever written to disk.
No test exercises `rate_hz == 0`.
**Fix:**
```python
        per_request_ms = (
            1000 * count / rate_hz / requests_completed
            if (requests_completed > 0 and rate_hz > 0)
            else None
        )
```

### CR-04: `subprocess.run(..., timeout=N)` calls only catch `OSError`, letting a hang crash the script after measurement data is already captured but not yet persisted

**File:**
`python/rsglang/profiling/sidecar.py:67-110` (`_git_commit`, `_git_dirty`, `_gpu_name`,
called from `build_meta` at lines 130-134),
`python/rsglang/profiling/procs.py:138-147` (`py_spy_version`) and `:150-163`
(`py_spy_dump`, which has no try/except at all around its `subprocess.run`),
`scripts/baseline_profile.py:422-444` (`_check_hyperfine_version`)
**Issue:** `subprocess.TimeoutExpired` is **not** an `OSError` subclass (it derives
from `subprocess.SubprocessError` → `Exception`), so every one of these helpers'
`except OSError:` (or, for `py_spy_dump`, the complete absence of a `try`) fails to
catch a hang in the external tool it's shelling out to. The highest-impact instance
is `sidecar.build_meta()`, which is called from `cmd_run` only *after* every
requested scenario (`s1_cancel`/`s2_saturation`/`s3_coldstart`) has already run to
completion:

```python
except (...) as exc:          # scenario measurement exceptions
    ...
    return 1

meta = sidecar.build_meta(...)   # <-- calls _git_commit/_git_dirty/_gpu_name here
doc = {..., "meta": meta, "scenarios": scenarios_out, ...}
sidecar.write_sidecar(doc, out_path, ...)   # never reached if build_meta() raises
```

If `git` or `nvidia-smi` hangs (lock contention, a stuck driver, etc.) for more than
the 10s `timeout=10`, `subprocess.TimeoutExpired` propagates uncaught straight through
`build_meta()`, `cmd_run()`, and `main()`, discarding the entire `scenarios_out`
dict — a fully-measured, potentially hours-long GPU profiling run (default
`s1-duration-s=120`, `s2-requests=512`, three `s3` hyperfine runs) — without ever
calling `write_sidecar()`. This is a genuine data-loss risk, not just an ugly
traceback.
**Fix:** Catch both exception types everywhere a timeout is set, e.g.:
```python
    except (OSError, subprocess.TimeoutExpired):
        return None
```
and wrap `procs.py_spy_dump`'s currently-bare `subprocess.run(..., timeout=30)` in an
equivalent try/except that degrades to a clean error instead of an uncaught
exception.

## Warnings

### WR-01: `scenarios.coldstart_once`'s self-heal abandons a stale process group it cannot parse, instead of killing it

**File:** `python/rsglang/profiling/scenarios.py:338-351`
**Issue:**
```python
if pgid_file.exists():
    try:
        stale_pgid = int(pgid_file.read_text().strip())
    except (OSError, ValueError):
        stale_pgid = None
    if stale_pgid is not None:
        _kill_group(stale_pgid)
    try:
        pgid_file.unlink()
    except OSError:
        pass
```
If the pgid file exists but is unreadable or contains non-integer content,
`stale_pgid` becomes `None`, the `_kill_group()` call is skipped entirely, and the
file is still deleted. Any process group that file was tracking is now both
un-killed and untraceable (the one artifact that could identify it is gone). This
silently defeats the self-heal mechanism's entire purpose in exactly the failure mode
(corrupted/partial state file) it exists to recover from.
**Fix:** If the pgid file can't be parsed, don't delete it blind — log a warning to
stderr before unlinking, or better, leave it in place (so a human/next run can at
least see something is wrong) rather than discarding the one clue to the orphaned
process group's identity.

### WR-02: `analysis.load_speedscope`/`bucket_samples` don't validate that sample frame indices are in range

**File:** `python/rsglang/profiling/analysis.py:30-85` (`load_speedscope`), `140-171`
(`bucket_samples`, indexing at line 167: `hit |= frame_bucket_mask[idx]`)
**Issue:** `load_speedscope` validates `shared.frames` is a list, `profiles` is a
list, and each profile's `samples`/`weights` lists have matching lengths — but never
checks that the integers inside each `samples` stack are valid indices into
`shared.frames`. A malformed or corrupted real py-spy speedscope file (the same class
of "py-spy wrote something unexpected" problem the already-fixed invalid-UTF-8 case
addresses) with an out-of-range frame index crashes `bucket_samples` with an
unhandled `IndexError`, propagating through `PySpyRecorder.stop_all()` →
`session.run_session` → `cmd_run`, again discarding completed measurement data.
**Fix:** In `load_speedscope`, validate each sample index is `0 <= idx <
len(frames)` (or catch `IndexError` in `bucket_samples` and re-raise as
`SpeedscopeError` with the offending profile/sample identified), consistent with how
every other structural defect in this function is turned into a named
`SpeedscopeError`.

### WR-03: `procs.py_spy_dump` silently swallows non-permission py-spy failures

**File:** `python/rsglang/profiling/procs.py:150-163`
**Issue:**
```python
if out.returncode != 0:
    combined = (out.stdout or "") + (out.stderr or "")
    if _PERMISSION_RE.search(combined):
        raise PySpyPermissionError(...)
    return ""
```
Any py-spy dump failure that isn't a permission error (process vanished mid-dump,
py-spy internal error, unsupported platform quirk, etc.) returns an empty string
rather than surfacing the failure. `classify_dump("")` then classifies that pid as
`"other"` with no diagnostic trail — a transient failure for the *scheduler* pid
would silently masquerade as "this pid is not the scheduler" rather than "py-spy
could not dump this pid", making `identify_roles`'s eventual `RoleError` (if role
counts come up wrong) harder to debug than it needs to be.
**Fix:** At minimum, log `combined` to stderr (or include it in a wrapping warning)
before falling back to `""`, so a real but non-permission py-spy failure isn't
indistinguishable from "this just isn't the role in question."

### WR-04: `procs.wait_ready` doesn't verify the HTTP endpoint it probes belongs to the process it just launched

**File:** `python/rsglang/profiling/procs.py:95-115`
**Issue:** `wait_ready` polls `GET /v1/models` on the given port and returns success
on the *first* successful response, before ever checking `handle.proc.poll()`:
```python
while True:
    try:
        urllib.request.urlopen(url, timeout=1)
        return time.perf_counter()
    except (urllib.error.URLError, OSError, ConnectionError):
        pass
    if handle.proc.poll() is not None:
        ...
```
If an unrelated, already-running server (e.g. a previous orphaned run on the same
port, or simply another process the operator forgot about) is listening on `port`,
`wait_ready` reports success against that unrelated listener even if the just-spawned
`handle.proc` failed to bind and is about to exit. Downstream role identification
would likely fail loudly (no children / `RoleError`), but the pre-existing unrelated
process that actually answered the probe is never discovered or torn down by this
code path — it's outside `handle.pgid` and `discover_children(handle.proc.pid)`
entirely.
**Fix:** Low-cost mitigation: after a successful `urlopen`, confirm
`handle.proc.poll() is None` before returning readiness (so a probe racing a crash
doesn't get "lucky"), or track which pid is actually bound to the port (e.g. via
`psutil.net_connections` filtered to `handle.proc`'s descendants) before declaring
success.

### WR-05: `PySpyRecorder.stop_all()` doesn't detect py-spy recorder subprocesses that survive SIGKILL

**File:** `python/rsglang/profiling/session.py:85-118`
**Issue:**
```python
for proc in self._procs.values():
    try:
        proc.wait(timeout=timeout_s)
    except subprocess.TimeoutExpired:
        proc.kill()
        try:
            proc.wait(timeout=10.0)
        except subprocess.TimeoutExpired:
            pass
```
If a py-spy recorder process is still alive after SIGKILL + a 10s wait (e.g. stuck
in uninterruptible I/O), the second `TimeoutExpired` is swallowed with no error or
warning raised, and `stop_all()` proceeds to read whatever (possibly incomplete)
speedscope file that process wrote. This is a smaller-blast-radius sibling of the
main teardown concern (py-spy processes don't hold GPU memory), but it's an
unreported leak and a possible source of a truncated/incomplete speedscope file read
silently as if it were a clean recording.
**Fix:** Collect pids that are still alive after the final wait and surface them
(append to the `warnings` list threaded through `run_session`, or raise via
`MeasurementError`) rather than silently continuing.

### WR-06: `session.run_session`'s teardown `finally` can mask an original in-flight exception

**File:** `python/rsglang/profiling/session.py:323-383`
**Issue:**
```python
try:
    t_ready = procs.wait_ready(handle, port=port, timeout_s=timeout_s)
    ...
finally:
    survivors = procs.teardown(handle, extra_pids=children)
    if survivors:
        raise MeasurementError(
            f"{name}: process(es) survived teardown: {survivors}"
        )
```
If the `try` body raises for one reason (e.g. `ServerExited`) *and* `teardown()`
happens to also report survivors, the `finally` block's own `raise` replaces the
original exception per Python's normal `finally`-raises-wins semantics, discarding
the actual root cause (why the server exited/wasn't ready) in favor of the teardown
symptom. This doesn't risk a process leak (teardown already ran), but it can send
operators chasing the wrong failure.
**Fix:** Use `raise ... from original_exc` (capturing the original exception via
`except Exception as exc: ... finally: ...` restructuring, or Python 3.11+'s
`ExceptionGroup`) so both failures are visible, or at least log the survivors as a
warning without re-raising when an exception is already propagating
(`sys.exc_info()[0] is not None`).

### WR-07: No synchronization between the hook's daemon flush thread and its `atexit` final flush

**File:** `python/rsglang/profiling/hook.py:185-201`
**Issue:** `_flush_loop` (daemon thread) and `_final_flush` (registered via
`atexit.register(_final_flush, hook_file)`, which runs on the main thread during
interpreter shutdown) both call `_drain(hook_file)`, and both ultimately call
`_append_records`, which opens the same `hook_file` path independently
(`open(path, "a", ...)`) with no lock between the two call sites. Because
`atexit` callbacks run before daemon threads are torn down, `_final_flush` can
execute concurrently with an in-progress `_flush_loop` iteration, each with its own
file handle open in append mode. Reading from the shared `_gc_pending` deque is
safe (GIL-atomic `popleft`), so no record is double-counted, but two independent
`open()`/`write()` sequences to the same path from two threads is a theoretical race
with no test coverage.
**Fix:** Guard `_append_records` (or at least the drain-and-append sequence) with a
`threading.Lock` shared between `_flush_loop` and `_final_flush`.

## Info

### IN-01: `procs.ROLES` is assigned but never used

**File:** `python/rsglang/profiling/procs.py:30`
**Issue:** `ROLES = sidecar.ROLES` is defined but not referenced anywhere else in
`procs.py`, and no other module imports `procs.ROLES` (verified via repo-wide grep —
only `sidecar.ROLES` and `session.ROLES` are actually used elsewhere).
**Fix:** Remove the unused alias, or if it's meant as part of `procs`'s public
surface for future use, note that explicitly; otherwise it's dead code.

---

_Reviewed: 2026-10-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
