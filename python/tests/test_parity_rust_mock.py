"""Phase 6 Task 2: the real Rust frontend over mock-scheduler, driven by
`discover` and by Phase 6's own external-target stress driver -- proves
`scripts/parity_check.py`'s Phase-5-derived defaults before any GPU time is
spent.

`rsglang.testing.rust_frontend` wires the real `rsg-server` binary to a real
`mock-scheduler` subprocess (two separate OS processes over real `ipc://`
sockets), standing in for the Mac-side "serve rsg-server's HTTP API backed by
mock-scheduler" command this plan's precondition required. `discover` and the
stress driver both treat it as an ordinary `--server-cmd`/externally-launched
server -- no special-casing anywhere else.
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
MODEL = "Qwen/Qwen3-0.6B"

pytestmark = pytest.mark.slow

_RUST_FRONTEND_SERVER_CMD = (
    "{python} -m rsglang.testing.rust_frontend --port {port} --model {model}"
)


@pytest.fixture(scope="module")
def rust_bins() -> None:
    subprocess.run(["cargo", "build", "-p", "rsg-server"], cwd=REPO_ROOT, check=True)


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def test_discover_rust_against_mock(rust_bins, tmp_path):
    port = _free_port()
    out_path = tmp_path / "discover-rust.json"
    work_dir = tmp_path / "w"

    result = subprocess.run(
        [
            sys.executable,
            "scripts/parity_check.py",
            "discover",
            "--frontend", "rust",
            "--skip-tap-check",
            "--model", MODEL,
            "--port", str(port),
            "--timeout", "120",
            "--server-cmd", _RUST_FRONTEND_SERVER_CMD,
            "--work-dir", str(work_dir),
            "--out", str(out_path),
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=180,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r}\nstderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    endpoints = doc["endpoints"]
    assert len(endpoints) == 8, endpoints
    by_name = {e["name"]: e for e in endpoints}
    for name, entry in by_name.items():
        assert entry.get("ok") is True, f"{name}: {entry}"


def test_stress_cmd_targets_running_server(rust_bins, tmp_path):
    import os

    from rsglang.profiling import procs

    port = _free_port()
    argv = procs.server_argv(
        _RUST_FRONTEND_SERVER_CMD, python=sys.executable, model=MODEL, port=port
    )
    log_path = tmp_path / "rust-frontend.log"
    handle = procs.launch_server(argv, env=os.environ, log_path=log_path)
    try:
        procs.wait_ready(handle, port=port, timeout_s=120.0)

        base_url = f"http://127.0.0.1:{port}"
        # The plan's own default --stress-cmd (rsglang.parity.stress_client),
        # formatted exactly as stress.py's _run_one_timing formats it.
        stress_argv_template = (
            "{python} -m rsglang.parity.stress_client --base-url {base_url} "
            "--requests 128 --abort-fraction 0.3 --seed 0 --model {model}"
        )
        import shlex

        stress_argv = shlex.split(
            stress_argv_template.format(python=sys.executable, base_url=base_url, model=MODEL)
        )
        result = subprocess.run(
            stress_argv,
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=600,
        )
        assert result.returncode == 0, (
            f"stress_client against an already-running server exited "
            f"{result.returncode}\nstdout={result.stdout!r}\nstderr={result.stderr!r}\n"
            f"server log:\n{log_path.read_text(errors='replace')}"
        )
    finally:
        procs.teardown(handle, grace_s=30.0)
