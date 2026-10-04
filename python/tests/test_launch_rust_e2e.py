"""End-to-end tracer: launcher + fake scheduler (upstream's real queues) + real rsg-server."""

from __future__ import annotations

import os
import re
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

from rsglang import sockets
from rsglang.handshake import read_upstream_sha
from rsglang.testing.fake_scheduler import FACTORY_PATH, STATUS_DIR_ENV

pytestmark = pytest.mark.slow

REPO = Path(__file__).resolve().parents[2]
UPSTREAM_ARGS = [
    "--model", "Qwen/Qwen3-0.6B",
    "--dtype", "bfloat16",
    "--page-size", "16",
    "--max-running-requests", "8",
]


@pytest.fixture(scope="module")
def rust_bin() -> Path:
    subprocess.run(["cargo", "build", "-p", "rsg-server"], cwd=REPO, check=True)
    return REPO / "target" / "debug" / "rsg-server"


def _alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def _wait_until(predicate, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return predicate()


class LauncherRun:
    def __init__(self, rust_bin: Path, status_dir: Path):
        env = {
            **os.environ,
            "RSGLANG_SCHEDULER_FACTORY": FACTORY_PATH,
            STATUS_DIR_ENV: str(status_dir),
            "RUST_LOG": "info",
        }
        self.proc = subprocess.Popen(
            [sys.executable, "-m", "rsglang.launch", "--frontend", "rust",
             "--rust-bin", str(rust_bin), "--ready-timeout", "60", *UPSTREAM_ARGS],
            cwd=REPO,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.lines: list[str] = []
        self._drain = threading.Thread(target=self._read, daemon=True)
        self._drain.start()

    def _read(self) -> None:
        for line in self.proc.stderr:
            self.lines.append(line.rstrip("\n"))

    def wait_for(self, pattern: str, timeout: float) -> str:
        rx = re.compile(pattern)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for line in list(self.lines):
                if rx.search(line):
                    return line
            if self.proc.poll() is not None and not self._drain.is_alive():
                break
            time.sleep(0.05)
        raise AssertionError(f"no line matching {pattern!r} within {timeout} s:\n" + "\n".join(self.lines))

    def index_of(self, needle: str) -> int:
        return next(i for i, line in enumerate(self.lines) if needle in line)

    def pids(self) -> dict[str, int]:
        found = {}
        for line in list(self.lines):
            if m := re.search(r"spawned rsg-server pid=(\d+)", line):
                found["rsg-server"] = int(m.group(1))
            if m := re.search(r"spawned scheduler rank=0 pid=(\d+)", line):
                found["scheduler"] = int(m.group(1))
        return found

    def cleanup(self) -> None:
        try:
            os.killpg(self.proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        for pid in self.pids().values():
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        sockets.unlink_run_sockets(f".rsg={self.proc.pid}")


@pytest.fixture
def launcher(rust_bin, tmp_path):
    run = LauncherRun(rust_bin, tmp_path)
    yield run
    run.cleanup()


def test_rust_mode_handshake_reaches_rsg_server(launcher, tmp_path):
    line = launcher.wait_for("handshake received", 90)
    for expected in ("max_seq_len=4096", "eos_token_id=151645", "page_size=16",
                     "max_running_req=8", "num_pages=1024", f"upstream_sha={read_upstream_sha()}"):
        assert expected in line, line

    launcher.wait_for("backend ready; handshake sent", 10)
    # D-10: rsg-server is up and waiting before the backend reports ready.
    assert launcher.index_of("awaiting handshake on stdin") < launcher.index_of("backend ready; handshake sent")

    pid = launcher.proc.pid
    sock0 = Path(f"/tmp/minisgl_0.rsg={pid}")
    sock1 = Path(f"/tmp/minisgl_1.rsg={pid}")
    assert sock0.exists()
    assert sock1.exists()

    # D-07: the scheduler side sees a bound peer on _1 (Rust).
    assert _wait_until((tmp_path / "detok_peer_connected").exists, 15)

    children = launcher.pids()
    assert set(children) == {"rsg-server", "scheduler"}, launcher.lines

    launcher.proc.send_signal(signal.SIGTERM)
    assert launcher.proc.wait(timeout=30) == 0, "\n".join(launcher.lines)
    for name, child in children.items():
        assert _wait_until(lambda: not _alive(child), 15), f"{name} pid={child} still alive"
    assert not sock0.exists()
    assert not sock1.exists()
