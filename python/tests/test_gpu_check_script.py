"""Mac-runnable tests for scripts/gpu_phase1_check.sh's GPU-orphan helpers
(G-01-7-WR07 / WR-07, G-01-7-WR08 / WR-08).

Each test sources the script under bash with stub `nvidia-smi` and `setsid`
executables placed first on PATH, so the real preflight, build and GPU steps
never run (the source guard returns before the "# --- Preflight" section).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import time
from pathlib import Path

import pytest

pytestmark = pytest.mark.slow  # every test spawns bash

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "gpu_phase1_check.sh"


def _rc(result: subprocess.CompletedProcess[str]) -> int:
    """Extract the `rc=<N>` line a snippet echoes after calling a helper."""
    match = re.search(r"^rc=(-?\d+)$", result.stdout, re.MULTILINE)
    assert match, f"no rc= line in stdout: {result.stdout!r} (stderr: {result.stderr!r})"
    return int(match.group(1))


def _write_stub(tmp_path: Path, name: str, body: str) -> Path:
    """Write an executable `#!/bin/bash` stub named `name` into tmp_path/bin."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / name
    stub.write_text(f"#!/bin/bash\n{body}\n")
    stub.chmod(0o755)
    return stub


def _write_setsid_stub(tmp_path: Path) -> Path:
    """Write a stub `setsid` that execs in place like util-linux setsid does for a
    non-leader, after an optional delay (STUB_SETSID_DELAY) and an optional skip of
    os.setsid() (STUB_SETSID_MODE=never, default "detach")."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / "setsid"
    stub.write_text(
        f"#!{sys.executable}\n"
        "import os, sys, time\n"
        "delay = float(os.environ.get('STUB_SETSID_DELAY', '0'))\n"
        "mode = os.environ.get('STUB_SETSID_MODE', 'detach')\n"
        "time.sleep(delay)\n"
        "if mode == 'detach':\n"
        "    os.setsid()\n"
        "os.execvp(sys.argv[1], sys.argv[1:])\n"
    )
    stub.chmod(0o755)
    return stub


def _bash(snippet: str, tmp_path: Path, **env_vars: object) -> subprocess.CompletedProcess[str]:
    """Source SCRIPT, then run `snippet`, with tmp_path/bin first on PATH."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["TMPDIR"] = str(tmp_path)
    env["PYTHON"] = sys.executable
    env.update({k: str(v) for k, v in env_vars.items()})
    script = f'source "{SCRIPT}"\n{snippet}'
    return subprocess.run(
        ["bash", "-c", script],
        capture_output=True,
        text=True,
        timeout=60,
        start_new_session=True,
        env=env,
    )


# --- on_gpu --------------------------------------------------------------------


def test_on_gpu_reports_nvidia_smi_failure(tmp_path):
    _write_stub(tmp_path, "nvidia-smi", 'echo "nvidia-smi: driver error" >&2\nexit 9')
    result = _bash('rc=0; on_gpu 4242 || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 2, result.stderr
    assert "nvidia-smi failed" in result.stderr, result.stderr


def test_on_gpu_detects_pid_in_long_listing(tmp_path):
    # The old piped check (gpu_pids | grep -qx) reports this pid absent under
    # pipefail, with PIPESTATUS starting 141 (writer SIGPIPE'd by grep's early exit).
    _write_stub(
        tmp_path,
        "nvidia-smi",
        'echo " 4242"\nfor i in $(seq 1 200000); do echo "99999"; done',
    )
    result = _bash('rc=0; on_gpu 4242 || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 0, result.stderr


def test_on_gpu_reports_absent_pid(tmp_path):
    _write_stub(tmp_path, "nvidia-smi", 'echo " 1111"\necho " 2222"')
    result = _bash('rc=0; on_gpu 4242 || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 1, result.stderr


# --- wait_no_orphans -------------------------------------------------------------


def test_wait_no_orphans_fails_when_nvidia_smi_fails(tmp_path):
    # The old step logic passed here, which is the WR-07 false PASS.
    _write_stub(tmp_path, "nvidia-smi", 'echo "nvidia-smi: driver error" >&2\nexit 9')
    snippet = """
sleep 0 &
p=$!
wait "$p"
rc=0; wait_no_orphans 2 "$p" || rc=$?
echo "rc=$rc"
"""
    result = _bash(snippet, tmp_path)
    assert _rc(result) != 0, result.stderr
    assert "nvidia-smi failed" in result.stderr, result.stderr


def test_wait_no_orphans_fails_when_exited_pid_is_still_listed(tmp_path):
    _write_stub(
        tmp_path,
        "nvidia-smi",
        'cat "$STUB_PIDS_FILE"\nfor i in $(seq 1 200000); do echo "99999"; done',
    )
    pids_file = tmp_path / "pids.txt"
    snippet = f"""
sleep 0 &
p=$!
wait "$p"
echo "$p" > "{pids_file}"
rc=0; wait_no_orphans 2 "$p" || rc=$?
echo "rc=$rc"
"""
    result = _bash(snippet, tmp_path, STUB_PIDS_FILE=str(pids_file))
    assert _rc(result) == 1, result.stderr
    assert "still listed by nvidia-smi" in result.stdout + result.stderr


def test_wait_no_orphans_passes_when_pids_are_gone_and_unlisted(tmp_path):
    _write_stub(tmp_path, "nvidia-smi", "exit 0")
    snippet = """
sleep 0 &
p=$!
wait "$p"
rc=0; wait_no_orphans 2 "$p" || rc=$?
echo "rc=$rc"
"""
    result = _bash(snippet, tmp_path)
    assert _rc(result) == 0, result.stderr


def test_wait_no_orphans_fails_while_pid_runs(tmp_path):
    _write_stub(tmp_path, "nvidia-smi", "exit 0")
    snippet = """
sleep 30 >"$TMPDIR/sleep_out.log" 2>&1 &
p=$!
rc=0; wait_no_orphans 2 "$p" || rc=$?
echo "rc=$rc"
kill -9 "$p" 2>/dev/null || true
wait "$p" 2>/dev/null || true
"""
    result = _bash(snippet, tmp_path)
    assert _rc(result) == 1, result.stderr
    assert "still running" in result.stdout + result.stderr


# --- start_session ---------------------------------------------------------------


def test_start_session_waits_for_slow_setsid(tmp_path):
    # The old fixed half-second wait returned 1 here.
    _write_setsid_stub(tmp_path)
    snippet = """
rc=0; start_session "$TMPDIR/log.txt" sleep 30 || rc=$?
echo "rc=$rc"
echo "pid=$BG_PID"
pgid="$(ps -o pgid= -p "$BG_PID" 2>/dev/null | tr -d ' ')"
echo "pgid=$pgid"
"""
    result = _bash(snippet, tmp_path, STUB_SETSID_DELAY="1.5", STUB_SETSID_MODE="detach")
    assert _rc(result) == 0, result.stderr
    pid_match = re.search(r"^pid=(\d+)$", result.stdout, re.MULTILINE)
    pgid_match = re.search(r"^pgid=(\d+)$", result.stdout, re.MULTILINE)
    assert pid_match and pgid_match, result.stdout
    assert pid_match.group(1) == pgid_match.group(1), result.stdout


def test_start_session_kills_what_it_started_when_setsid_never_detaches(tmp_path):
    _write_setsid_stub(tmp_path)
    snippet = """
rc=0; start_session "$TMPDIR/log.txt" sleep 30 || rc=$?
echo "rc=$rc"
if kill -0 "$BG_PID" 2>/dev/null; then echo "alive"; else echo "dead"; fi
"""
    result = _bash(snippet, tmp_path, STUB_SETSID_MODE="never")
    assert _rc(result) == 1, result.stderr
    assert "setsid did not exec in place" in result.stderr, result.stderr
    assert "dead" in result.stdout, result.stdout


def test_start_session_fast_path(tmp_path):
    _write_setsid_stub(tmp_path)
    snippet = """
rc=0; start_session "$TMPDIR/log.txt" sleep 30 || rc=$?
echo "rc=$rc"
echo "pid=$BG_PID"
pgid="$(ps -o pgid= -p "$BG_PID" 2>/dev/null | tr -d ' ')"
echo "pgid=$pgid"
"""
    start = time.monotonic()
    result = _bash(snippet, tmp_path, STUB_SETSID_DELAY="0", STUB_SETSID_MODE="detach")
    elapsed = time.monotonic() - start
    assert _rc(result) == 0, result.stderr
    pid_match = re.search(r"^pid=(\d+)$", result.stdout, re.MULTILINE)
    pgid_match = re.search(r"^pgid=(\d+)$", result.stdout, re.MULTILINE)
    assert pid_match and pgid_match, result.stdout
    assert pid_match.group(1) == pgid_match.group(1), result.stdout
    assert elapsed < 4, elapsed


def test_start_session_leaves_an_exited_process_to_the_caller(tmp_path):
    _write_setsid_stub(tmp_path)
    snippet = """
rc=0; start_session "$TMPDIR/log.txt" true || rc=$?
echo "rc=$rc"
"""
    result = _bash(snippet, tmp_path, STUB_SETSID_MODE="detach")
    assert _rc(result) == 0, result.stderr
