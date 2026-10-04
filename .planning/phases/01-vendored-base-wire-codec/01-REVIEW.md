---
phase: 01-vendored-base-wire-codec
reviewed: 2026-10-04T00:00:00Z
depth: standard
files_reviewed: 8
files_reviewed_list:
  - crates/rsg-server/src/handshake.rs
  - crates/rsg-server/tests/cli.rs
  - python/rsglang/backend.py
  - python/rsglang/launch.py
  - python/tests/test_gpu_check_script.py
  - python/tests/test_launch_args.py
  - python/tests/test_parent_watchdog.py
  - scripts/gpu_phase1_check.sh
findings:
  critical: 0
  warning: 1
  info: 3
  total: 4
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-10-04T00:00:00Z
**Depth:** standard
**Files Reviewed:** 8
**Status:** issues_found

## Summary

This is an incremental review scoped to the files touched while closing WR-01, WR-04, WR-06, WR-07, WR-08, and WR-09 from the prior 01-REVIEW.md (see 01-REVIEW-DISPOSITION.md). I re-read each gap-closure fix on its own merits rather than re-litigating the already-disposed findings.

The closures hold up well under tracing:

- **WR-04** (`crates/rsg-server/src/handshake.rs`): `eos_token_id` is now required via `#[serde(deserialize_with = "Option::deserialize")]`, which correctly suppresses serde's implicit "missing key means `None`" special-case for `Option` fields while still accepting an explicit `null`. The `missing_eos_key_is_malformed` / `missing_eos_key_exits_2` tests (Rust unit + process-level) exercise this correctly.
- **WR-06** (`python/rsglang/backend.py`): `start_parent_watchdog` now declares `prctl`'s `argtypes` (avoiding ctypes' default-int truncation) and is invoked inside `run_scheduler`'s outer `try`, so a watchdog-setup failure reaches the launcher as a `{"kind": "error", ...}` envelope instead of a silent exit. Verified via `test_watchdog_startup_failure_reaches_launcher_as_error_envelope` and the prctl-failure parametrized test.
- **WR-07 / WR-08** (`scripts/gpu_phase1_check.sh`): `on_gpu` now captures `nvidia-smi`'s output into a variable before `grep -qx`, eliminating the old pipe/SIGPIPE false-PASS; `start_session` now polls for up to 5 s for `setsid` to exec in place instead of a fixed 0.5 s sleep, and kills only the single PID it started (not a process group) on timeout, with a comment explaining why a group kill would be unsafe at that point. Both are covered by targeted Mac-runnable bash tests.
- **WR-09** (`python/tests/test_parent_watchdog.py`): the "exits at once" test now requires the `calling` marker, the absence of `returned`, the absence of `survived`, and the absence of a traceback — so a crash elsewhere in the watchdog path can no longer masquerade as the intended exit-1 behavior.
- **WR-01** (`python/rsglang/launch.py`): rust mode now treats `run_shell` from upstream's own `parse_args(rest)` as authoritative, so `--shell` / `--shell-m` (argparse abbreviations of `--shell-mode`) are rejected, not just a literal `"--shell-mode" in rest` check. This closes the original "rust mode runs silently with shell-mode limits" risk. However, tracing the new code path surfaced an ordering bug (WR-local-01 below): the authoritative check runs *after* `resolve_rust_bin`, so an abbreviated `--shell-mode` combined with a missing `rsg-server` binary now reports the wrong root cause to the user.

One warning and three info-level observations are recorded below. None of the fixed WR- items themselves regressed; the warning is a new side effect of the WR-01 fix's specific ordering choice.

## Warnings

### WR-01: Abbreviated `--shell-mode` rejection can be masked by an unrelated "binary not found" error

**File:** `python/rsglang/launch.py:100-128`
**Issue:** `run_rust_mode` only catches the *literal* `--shell-mode` token early (line 101-103) and returns 2 immediately. For the abbreviated forms (`--shell`, `--shell-m`), the authoritative check — `run_shell` from `parse_args(rest)` inside `_run_rust_mode` (line 123-126) — only runs *after* `resolve_rust_bin(ns.rust_bin)` has already succeeded (line 104-106). If a user runs `--frontend rust --shell-m ...` on a machine where `rsg-server` hasn't been built yet (no `--rust-bin`, no `$RSGLANG_RUST_BIN`, no `target/{release,debug}/rsg-server`), `resolve_rust_bin` returns `None` and the function returns 2 with "rsg-server binary not found; run: cargo build -p rsg-server" — never reaching the shell-mode check. The user is told to build the Rust binary when the actual, unrelated problem is that `--shell-mode` isn't supported in rust mode at all. `test_rust_mode_rejects_abbreviated_shell_mode_without_spawning` only exercises the case where `--rust-bin` is valid (`sys.executable`), so this ordering gap isn't covered by the new tests.

This doesn't reintroduce the original security/correctness concern (rust mode never silently runs with shell-mode limits — it still exits 2 either way), but it does give an incorrect diagnostic in a state a developer is likely to hit (first run, before `cargo build`).

**Fix:** Determine `run_shell` once, before resolving the binary, and drop the now-redundant literal check:

```python
def run_rust_mode(ns: argparse.Namespace, rest: List[str]) -> int:
    from minisgl.server.args import parse_args

    server_args, run_shell = parse_args(rest)
    if run_shell:
        _log("--shell-mode is not supported with --frontend rust")
        return 2
    rust_bin = resolve_rust_bin(ns.rust_bin)
    if rust_bin is None:
        return 2
    suffix = f".rsg={os.getpid()}"
    try:
        return _run_rust_mode(ns, rest, rust_bin, suffix, server_args)
    finally:
        sockets.unlink_run_sockets(suffix)
```

and thread `server_args` into `_run_rust_mode` instead of re-parsing it there, so `rest` is parsed exactly once.

## Info

### IN-01: `prctl` failure handling only catches `OSError`, not a missing symbol

**File:** `python/rsglang/backend.py:81-97`
**Issue:** `start_parent_watchdog`'s `try` only catches `OSError`. `libc.prctl.argtypes = [...]` (line 83) itself triggers a `ctypes.CDLL.__getattr__` symbol lookup for `"prctl"`; if that symbol isn't exported by whatever `ctypes.CDLL(None)` resolves to (a minimal/alternative libc, or a build without the usual glibc wrapper), ctypes raises `AttributeError`, not `OSError`. That exception isn't caught here, so it propagates out of `start_parent_watchdog` uncaught by this function — it is still caught one level up by `run_scheduler`'s outer `except BaseException`, so it does reach the launcher as an error envelope rather than failing silently, but the "log it and degrade to the polling watchdog" behavior that WR-06 was built to guarantee doesn't trigger for this particular failure mode, even though the function's own docstring specifically calls out "a seccomp-restricted container" as the motivating case for staying resilient here.
**Fix:** Broaden the catch to include the symbol-lookup failure mode, e.g. `except (OSError, AttributeError) as exc:`.

### IN-02: `gpu_phase1_check.sh`'s safety-net cleanup can signal an unrelated process group

**File:** `scripts/gpu_phase1_check.sh:68-74, 97-103`
**Issue:** `STARTED_PGIDS` records `BG_PID` immediately after spawning (line 86), before `start_session`'s polling loop confirms `setsid` actually detached into its own process group. If `setsid` never detaches (the "setsid did not exec in place" failure path, line 98-103), `start_session` already kills the single PID directly and explains why a group kill would be unsafe at that moment (the PID still shares the script's own process group). However, `BG_PID` remains in `STARTED_PGIDS`, and the `cleanup()` trap (run on any script exit, as a declared safety net) will later do `kill -9 -- "-$pgid"` for that same value, interpreting it as a process-group id. Since the group never actually took on that id, this sends a signal to whatever process group happens to have that numeric id at trap time — most likely nothing, but on a long-lived or heavily-loaded box it is possible (if unlikely) for an unrelated process group to have been assigned that pgid by the time the trap runs.
**Fix:** Only append to `STARTED_PGIDS` after `start_session` confirms the pgid matches `BG_PID`, or have `start_session` remove the entry itself on the "did not exec in place" failure path.

### IN-03: Duplicated pass/report logic in `wait_no_orphans`

**File:** `scripts/gpu_phase1_check.sh:121-158`
**Issue:** The polling loop (lines 125-139) and the final failure-reporting pass (lines 140-157) duplicate the same "check `ps -p`, then `on_gpu`, interpret rc 0/1/2" logic almost line for line, differing only in whether they print a per-pid message. This isn't a functional defect — both call sites were exercised by the tests — but it's an easy place for the two copies to drift out of sync on a future edit.
**Fix:** Factor the per-pid check into a small helper (e.g. `pid_is_orphaned <pid>` returning 0/1/2) called from both the polling loop and the final report.

---

_Reviewed: 2026-10-04T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
