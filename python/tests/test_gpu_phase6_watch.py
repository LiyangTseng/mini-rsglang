"""Mac-runnable tests for scripts/gpu_phase6_watch.sh, the D-12 process-health
watcher (PAR-02).

A stub `nvidia-smi` placed first on PATH reports STUB_GPU_PIDS for
`--query-compute-apps`, and a real `ps` is used for process state. The script
is run both as an executed subprocess (crash/zombie/restart/usage paths) and
sourced under bash (helper-level unit tests), matching the technique in
test_gpu_check_script.py.
"""

from __future__ import annotations

import os
import re
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest

pytestmark = pytest.mark.slow  # every test spawns bash/processes

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "gpu_phase6_watch.sh"


def _rc(result: subprocess.CompletedProcess[str]) -> int:
    """Extract the `rc=<N>` line a sourced snippet echoes after calling a helper."""
    match = re.search(r"^rc=(-?\d+)$", result.stdout, re.MULTILINE)
    assert match, f"no rc= line in stdout: {result.stdout!r} (stderr: {result.stderr!r})"
    return int(match.group(1))


def _write_stub(bin_dir: Path, name: str, body: str) -> Path:
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / name
    stub.write_text(f"#!/bin/bash\n{body}\n")
    stub.chmod(0o755)
    return stub


def _write_nvidia_smi_stub(bin_dir: Path, pids: str = "") -> Path:
    """Stub nvidia-smi: prints $STUB_GPU_PIDS (one pid per line) for
    --query-compute-apps, "Fake GPU" for anything else."""
    body = (
        'if printf "%s" "$*" | grep -q -- "--query-compute-apps"; then\n'
        '  if [ -n "${STUB_GPU_PIDS:-}" ]; then\n'
        '    printf "%s\\n" $STUB_GPU_PIDS\n'
        "  fi\n"
        "else\n"
        '  echo "Fake GPU"\n'
        "fi\n"
    )
    return _write_stub(bin_dir, "nvidia-smi", body)


def _env_with_stub_path(tmp_path: Path, **extra: object) -> dict[str, str]:
    bin_dir = tmp_path / "bin"
    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env.update({k: str(v) for k, v in extra.items()})
    return env


def _bash_source(snippet: str, tmp_path: Path, **env_vars: object) -> subprocess.CompletedProcess[str]:
    """Source SCRIPT (no args), then run `snippet`."""
    env = _env_with_stub_path(tmp_path, **env_vars)
    script = f'source "{SCRIPT}"\n{snippet}'
    return subprocess.run(
        ["bash", "-c", script],
        capture_output=True,
        text=True,
        timeout=30,
        env=env,
    )


# --- Tracer: Task 1 -------------------------------------------------------------


def test_tracer_crash_detected(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    sleep_proc = subprocess.Popen(["sleep", "30"])
    try:
        pid = sleep_proc.pid
        env = _env_with_stub_path(tmp_path, STUB_GPU_PIDS=str(pid))
        _write_nvidia_smi_stub(bin_dir)
        out_file = tmp_path / "watch.log"
        watcher = subprocess.Popen(
            [
                "bash",
                str(SCRIPT),
                "--pid",
                str(pid),
                "--interval",
                "0.2",
                "--out",
                str(out_file),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        time.sleep(0.6)
        sleep_proc.kill()
        sleep_proc.wait(timeout=5)

        stdout, stderr = watcher.communicate(timeout=5)
        assert watcher.returncode == 1, f"stdout={stdout!r} stderr={stderr!r}"
        assert "crashed=1" in stdout, stdout
        assert "verdict=unhealthy" in stdout, stdout
    finally:
        if sleep_proc.poll() is None:
            sleep_proc.kill()
            sleep_proc.wait(timeout=5)


def test_tracer_healthy_until_sigterm(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    sleep_proc = subprocess.Popen(["sleep", "30"])
    try:
        pid = sleep_proc.pid
        env = _env_with_stub_path(tmp_path, STUB_GPU_PIDS=str(pid))
        _write_nvidia_smi_stub(bin_dir)
        out_file = tmp_path / "watch.log"
        watcher = subprocess.Popen(
            [
                "bash",
                str(SCRIPT),
                "--pid",
                str(pid),
                "--interval",
                "0.2",
                "--out",
                str(out_file),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        time.sleep(1.0)
        watcher.send_signal(signal.SIGTERM)
        stdout, stderr = watcher.communicate(timeout=5)

        assert watcher.returncode == 0, f"stdout={stdout!r} stderr={stderr!r}"
        assert "crashed=0" in stdout, stdout
        assert "zombie=0" in stdout, stdout
        assert "verdict=healthy" in stdout, stdout
        match = re.search(r"samples=(\d+)", stdout)
        assert match and int(match.group(1)) >= 2, stdout

        lines = out_file.read_text().splitlines()
        sample_lines = [ln for ln in lines if ln.startswith("t=") and "gpu=listed" in ln]
        assert len(sample_lines) >= 2, lines
    finally:
        if sleep_proc.poll() is None:
            sleep_proc.kill()
            sleep_proc.wait(timeout=5)


# --- Usage paths (also exercised more thoroughly in Task 2) --------------------


def test_help_prints_usage(tmp_path):
    bin_dir = tmp_path / "bin"
    env = _env_with_stub_path(tmp_path)
    result = subprocess.run(
        ["bash", str(SCRIPT), "--help"],
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert "--pid PID" in result.stdout, result.stdout


# --- Task 2: zombie, restart, nvidia-smi failure, usage, sourced helpers -------


def test_zombie_detected(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    _write_nvidia_smi_stub(bin_dir)
    helper_code = (
        "import os, sys, time\n"
        "pid = os.fork()\n"
        "if pid == 0:\n"
        "    os._exit(0)\n"
        "else:\n"
        "    print(pid, flush=True)\n"
        "    time.sleep(30)\n"
    )
    parent = subprocess.Popen([sys.executable, "-c", helper_code], stdout=subprocess.PIPE, text=True)
    try:
        child_pid_line = parent.stdout.readline()
        child_pid = int(child_pid_line.strip())
        # Give the kernel a moment to mark the unreaped child as a zombie.
        time.sleep(0.3)
        env = _env_with_stub_path(tmp_path, STUB_GPU_PIDS=str(child_pid))
        result = subprocess.run(
            ["bash", str(SCRIPT), "--pid", str(child_pid), "--interval", "0.2"],
            capture_output=True,
            text=True,
            timeout=10,
            env=env,
        )
        assert result.returncode == 1, result.stdout + result.stderr
        assert "zombie=1" in result.stdout, result.stdout
        assert "verdict=unhealthy" in result.stdout, result.stdout
    finally:
        parent.kill()
        parent.wait(timeout=5)


def test_restart_detected_from_launcher_log(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    _write_nvidia_smi_stub(bin_dir)
    launcher_log = tmp_path / "launcher.log"
    launcher_log.write_text(
        "rsglang.launch: spawned scheduler rank=0 pid=111\n"
        "rsglang.launch: spawned scheduler rank=0 pid=222\n"
    )
    sleep_proc = subprocess.Popen(["sleep", "30"])
    try:
        pid = sleep_proc.pid
        env = _env_with_stub_path(tmp_path, STUB_GPU_PIDS=str(pid))
        watcher = subprocess.Popen(
            [
                "bash",
                str(SCRIPT),
                "--pid",
                str(pid),
                "--interval",
                "0.2",
                "--launcher-log",
                str(launcher_log),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        time.sleep(0.6)
        watcher.send_signal(signal.SIGTERM)
        stdout, stderr = watcher.communicate(timeout=5)
        assert watcher.returncode == 1, stdout + stderr
        assert "restarts=1" in stdout, stdout
        assert "verdict=unhealthy" in stdout, stdout
    finally:
        if sleep_proc.poll() is None:
            sleep_proc.kill()
            sleep_proc.wait(timeout=5)


def test_nvidia_smi_failure_counted_not_fatal(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    _write_stub(bin_dir, "nvidia-smi", 'echo "nvidia-smi: driver error" >&2\nexit 9')
    sleep_proc = subprocess.Popen(["sleep", "30"])
    try:
        pid = sleep_proc.pid
        env = _env_with_stub_path(tmp_path)
        out_file = tmp_path / "watch.log"
        watcher = subprocess.Popen(
            [
                "bash",
                str(SCRIPT),
                "--pid",
                str(pid),
                "--interval",
                "0.2",
                "--out",
                str(out_file),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        time.sleep(0.8)
        watcher.send_signal(signal.SIGTERM)
        stdout, stderr = watcher.communicate(timeout=5)
        assert watcher.returncode == 0, stdout + stderr
        assert "verdict=healthy" in stdout, stdout
        match = re.search(r"nvsmi_errors=(\d+)", stdout)
        assert match and int(match.group(1)) >= 1, stdout
        lines = out_file.read_text().splitlines()
        sample_lines = [ln for ln in lines if ln.startswith("t=")]
        assert any("gpu=nvsmi_error" in ln for ln in sample_lines), lines
        assert not any("gpu=listed" in ln for ln in sample_lines), lines
    finally:
        if sleep_proc.poll() is None:
            sleep_proc.kill()
            sleep_proc.wait(timeout=5)


def test_unlisted_pid_counted(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    _write_nvidia_smi_stub(bin_dir)  # STUB_GPU_PIDS unset -> empty listing
    sleep_proc = subprocess.Popen(["sleep", "30"])
    try:
        pid = sleep_proc.pid
        env = _env_with_stub_path(tmp_path)  # pid is never listed
        watcher = subprocess.Popen(
            ["bash", str(SCRIPT), "--pid", str(pid), "--interval", "0.2"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
        )
        time.sleep(0.6)
        watcher.send_signal(signal.SIGTERM)
        stdout, stderr = watcher.communicate(timeout=5)
        assert watcher.returncode == 0, stdout + stderr
        assert "verdict=healthy" in stdout, stdout
        match = re.search(r"gpu_unlisted=(\d+)", stdout)
        assert match and int(match.group(1)) >= 1, stdout
    finally:
        if sleep_proc.poll() is None:
            sleep_proc.kill()
            sleep_proc.wait(timeout=5)


def test_usage_errors(tmp_path):
    env = _env_with_stub_path(tmp_path)

    no_args = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True, timeout=10, env=env)
    assert no_args.returncode == 2, no_args.stdout + no_args.stderr

    bad_pid = subprocess.run(
        ["bash", str(SCRIPT), "--pid", "abc"], capture_output=True, text=True, timeout=10, env=env
    )
    assert bad_pid.returncode == 2, bad_pid.stdout + bad_pid.stderr

    bogus = subprocess.run(
        ["bash", str(SCRIPT), "--bogus"], capture_output=True, text=True, timeout=10, env=env
    )
    assert bogus.returncode == 2, bogus.stdout + bogus.stderr

    help_result = subprocess.run(
        ["bash", str(SCRIPT), "--help"], capture_output=True, text=True, timeout=10, env=env
    )
    assert help_result.returncode == 0, help_result.stdout + help_result.stderr


def test_sourced_helpers(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)

    # Sourcing with no arguments runs nothing past the source guard.
    result = _bash_source('echo "after_source=ok"', tmp_path)
    assert result.returncode == 0, result.stdout + result.stderr
    assert "after_source=ok" in result.stdout, result.stdout
    assert "missing required --pid" not in result.stderr, result.stderr

    # proc_state on a reaped pid prints "gone".
    snippet = """
sleep 0 &
p=$!
wait "$p"
proc_state "$p"
"""
    result = _bash_source(snippet, tmp_path)
    assert result.stdout.strip() == "gone", result.stdout + result.stderr

    # scheduler_spawns prints 0 for a missing file.
    missing_log = tmp_path / "does-not-exist.log"
    result = _bash_source(f'scheduler_spawns "{missing_log}"', tmp_path)
    assert result.stdout.strip() == "0", result.stdout + result.stderr

    # scheduler_spawns prints 2 for a two-line log.
    two_line_log = tmp_path / "two.log"
    two_line_log.write_text(
        "rsglang.launch: spawned scheduler rank=0 pid=111\n"
        "rsglang.launch: spawned scheduler rank=0 pid=222\n"
    )
    result = _bash_source(f'scheduler_spawns "{two_line_log}"', tmp_path)
    assert result.stdout.strip() == "2", result.stdout + result.stderr

    # on_gpu returns rc 2 when the nvidia-smi stub fails.
    _write_stub(bin_dir, "nvidia-smi", 'echo "nvidia-smi: driver error" >&2\nexit 9')
    result = _bash_source('rc=0; on_gpu 4242 || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 2, result.stdout + result.stderr
