"""Mac-runnable tests for scripts/gpu_phase6_parity.sh, the Phase 6 GPU
human-run wrapper (ROADMAP Phase 6 criteria 1-4, PAR-01/PAR-02).

test_tracer_mac_dry_run drives the whole pipeline -- cargo build, both
discover calls, the full run (endpoints/sequential/concurrent/stress), the
validate --require-gpu check, all 4 verdicts and check_upstream.py -- against
stub `nvidia-smi`/`cargo` and the Mac-only fake_parity_server stand-ins, so
the wrapper's wiring is proven before any GPU time is spent. Every step
passes except step 5 (validate --require-gpu), which fails only because the
run is not on Linux (platform check in sidecar.validate_sidecar).
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import time
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "gpu_phase6_parity.sh"


def _free_port() -> int:
    import socket

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _write_stub(bin_dir: Path, name: str, body: str) -> Path:
    bin_dir.mkdir(parents=True, exist_ok=True)
    stub = bin_dir / name
    stub.write_text(f"#!/bin/bash\n{body}\n")
    stub.chmod(0o755)
    return stub


def _write_nvidia_smi_stub(bin_dir: Path) -> Path:
    """`--query-gpu` prints "Fake GPU"; `--query-compute-apps` prints the
    fake scheduler child's pid via `pgrep -f`, matching the real
    scripts/gpu_phase6_watch.sh and rsglang.profiling.sidecar._gpu_name
    query shapes the stress part's watcher and the gpu-name helper use."""
    body = (
        'if printf "%s" "$*" | grep -q -- "--query-compute-apps"; then\n'
        "  pgrep -f fake-parity-scheduler || true\n"
        "else\n"
        '  echo "Fake GPU"\n'
        "fi\n"
        "exit 0\n"
    )
    return _write_stub(bin_dir, "nvidia-smi", body)


def _base_env(tmp_path: Path, bin_dir: Path) -> dict:
    env = dict(os.environ)
    env["TMPDIR"] = str(tmp_path)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    return env


# --- CLI shape (help / unknown arg) ---------------------------------------------


def test_help_anywhere():
    result = subprocess.run(["bash", str(SCRIPT), "--help"], capture_output=True, text=True, timeout=30)
    assert result.returncode == 0, result.stderr
    for token in (
        "Usage:",
        "--model",
        "--llama-model",
        "--concurrency",
        "--port",
        "--timeout",
        "--out",
        "--corpus",
        "--python-server-cmd",
        "--rust-server-cmd",
        "--stress-server-cmd",
        "--stress-cmd",
        "--run-extra-args",
        "--criterion",
    ):
        assert token in result.stdout, result.stdout


def test_unknown_arg_exits_2():
    result = subprocess.run(["bash", str(SCRIPT), "--bogus"], capture_output=True, text=True, timeout=30)
    assert result.returncode == 2, result.stdout + result.stderr


# --- preflight -------------------------------------------------------------------


def test_preflight_missing_tool_fails(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub(bin_dir, "cargo", "exit 0")

    import shutil

    bash_dir = str(Path(shutil.which("bash") or "/bin/bash").parent)
    env = dict(os.environ)
    env["TMPDIR"] = str(tmp_path)
    env["PATH"] = os.pathsep.join([str(bin_dir), bash_dir, "/bin", "/usr/bin"])

    result = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True, timeout=30, env=env)
    assert result.returncode == 1, result.stdout + result.stderr
    assert "FAIL preflight: not on PATH: nvidia-smi" in result.stdout, result.stdout

    match = re.search(r"^logs: (.+)$", result.stdout, re.MULTILINE)
    assert match, result.stdout
    log_dir = Path(match.group(1))
    leftover = list(log_dir.glob("discover-*"))
    assert not leftover, f"discover-* directory created despite failed preflight: {leftover}"


# --- full tracer dry run ----------------------------------------------------------


@pytest.mark.slow
def test_tracer_mac_dry_run(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_nvidia_smi_stub(bin_dir)
    _write_stub(bin_dir, "cargo", "exit 0")

    port = _free_port()
    out_path = tmp_path / "report.json"
    python = sys.executable

    python_server_cmd = (
        f"{python} -m rsglang.testing.fake_parity_server server --port {{port}} "
        "--model {model} --flavor python"
    )
    rust_server_cmd = (
        f"{python} -m rsglang.testing.fake_parity_server server --port {{port}} "
        "--model {model} --flavor rust"
    )
    stress_server_cmd = (
        f"{python} -m rsglang.testing.fake_parity_server server --port {{port}} "
        "--model {model} --flavor rust --emit-scheduler-child --bind-backend "
        "--abort-timing {abort_timing}"
    )
    stress_cmd = (
        f"{python} -m rsglang.testing.fake_parity_server stress --base-url {{base_url}} "
        "--requests 16 --abort-fraction 0.5 --seed 7"
    )

    env = _base_env(tmp_path, bin_dir)
    env["PYTHON"] = python
    env["CHECK_UPSTREAM_ARGS"] = "--offline"

    result = subprocess.run(
        [
            "bash",
            str(SCRIPT),
            "--model", "fake/model",
            "--llama-model", "fake/other",
            "--out", str(out_path),
            "--corpus", "fixtures/parity/corpus.json",
            "--port", str(port),
            "--timeout", "60",
            "--python-server-cmd", python_server_cmd,
            "--rust-server-cmd", rust_server_cmd,
            "--stress-server-cmd", stress_server_cmd,
            "--stress-cmd", stress_cmd,
            "--run-extra-args", "--probe-delays-ms 0,5 --probe-repeats 2 --settle-s 0.5",
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=300,
        env=env,
    )

    assert result.returncode == 1, f"stdout={result.stdout!r}\nstderr={result.stderr!r}"
    assert "SOME STEPS FAILED" in result.stdout, result.stdout

    expected = {
        1: "PASS",
        2: "PASS",
        3: "PASS",
        4: "PASS",
        5: "FAIL",
        6: "PASS",
        7: "PASS",
        8: "PASS",
        9: "PASS",
        10: "PASS",
    }
    for step, status in expected.items():
        pattern = rf"^{status} step {step}: "
        assert re.search(pattern, result.stdout, re.MULTILINE), (
            f"expected {status} for step {step}; stdout:\n{result.stdout}"
        )

    # Let a just-signalled child fully leave the process table before checking.
    time.sleep(0.5)
    for pattern in ("fake_parity_server", "fake-parity-scheduler"):
        leftover = subprocess.run(
            ["pgrep", "-f", pattern], capture_output=True, text=True
        ).stdout.strip()
        assert not leftover, f"leftover {pattern!r} process(es) after the run: {leftover}"
