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
