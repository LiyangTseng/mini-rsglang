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
from rsglang.testing.fake_scheduler import FACTORY_PATH, MODE_ENV, STATUS_DIR_ENV

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
    def __init__(self, rust_bin: Path, status_dir: Path, mode: str = "ok", ready_timeout: float = 60):
        env = {
            **os.environ,
            "RSGLANG_SCHEDULER_FACTORY": FACTORY_PATH,
            STATUS_DIR_ENV: str(status_dir),
            MODE_ENV: mode,
            "RUST_LOG": "info",
        }
        self.proc = subprocess.Popen(
            [sys.executable, "-m", "rsglang.launch", "--frontend", "rust",
             "--rust-bin", str(rust_bin), "--ready-timeout", f"{ready_timeout:g}", *UPSTREAM_ARGS],
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

    def finish(self, timeout: float) -> int:
        """Wait for the launcher to exit and for its stderr to be fully read."""
        code = self.proc.wait(timeout=timeout)
        self._drain.join(timeout=10)
        return code

    def text(self) -> str:
        return "\n".join(self.lines)

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
def make_launcher(rust_bin, tmp_path):
    runs = []

    def _make(**kwargs) -> LauncherRun:
        run = LauncherRun(rust_bin, tmp_path, **kwargs)
        runs.append(run)
        return run

    yield _make
    for run in runs:
        run.cleanup()


@pytest.fixture
def launcher(make_launcher):
    return make_launcher()


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


# --- D-12 failure contract: every failure exits non-zero, prints its cause, leaves no child ---


def _gone(pid: int, timeout: float) -> bool:
    return _wait_until(lambda: not _alive(pid), timeout)


def test_scheduler_crash_before_ready(make_launcher):
    run = make_launcher(mode="crash_before_ready")
    run.wait_for("spawned scheduler rank=0", 30)
    children = run.pids()
    assert run.finish(60) != 0, run.text()  # 1, or -9 if SIGKILL escalation was needed
    assert "fake scheduler crash before ready" in run.text()
    assert "scheduler rank 0 failed" in run.text()
    assert _gone(children["rsg-server"], 15), run.text()
    assert _gone(children["scheduler"], 15), run.text()


def test_ready_timeout(make_launcher):
    run = make_launcher(mode="hang_before_ready", ready_timeout=5)
    run.wait_for("spawned scheduler rank=0", 30)
    children = run.pids()
    assert run.finish(60) != 0, run.text()  # 1, or -9 if SIGKILL escalation was needed
    assert "backend not ready after 5" in run.text()
    for name, pid in children.items():
        assert _gone(pid, 20), f"{name} pid={pid} still alive\n{run.text()}"


def test_scheduler_crash_after_ready(make_launcher):
    run = make_launcher(mode="crash_after_ready")
    run.wait_for("handshake received", 90)
    children = run.pids()
    assert run.finish(30) != 0, run.text()  # 1, or -9 if SIGKILL escalation was needed
    assert "fake scheduler crash after ready" in run.text()
    assert _gone(children["rsg-server"], 15), run.text()


def test_rsg_server_death_triggers_shutdown(make_launcher):
    run = make_launcher()
    run.wait_for("handshake received", 90)
    children = run.pids()
    os.kill(children["rsg-server"], signal.SIGKILL)
    assert run.finish(30) != 0, run.text()  # 1, or -9 if SIGKILL escalation was needed
    assert "lines of rsg-server stderr" in run.text()
    assert "rsg-server exited with code -9" in run.text()
    assert _gone(children["scheduler"], 20), run.text()


def test_launcher_sigkill_leaves_no_orphans(make_launcher):
    run = make_launcher()
    run.wait_for("handshake received", 90)
    children = run.pids()
    assert set(children) == {"rsg-server", "scheduler"}, run.text()
    os.kill(run.proc.pid, signal.SIGKILL)  # the launcher only, not its group
    run.finish(10)
    # rsg-server: stdin EOF; scheduler: parent watchdog.
    for name, pid in children.items():
        assert _gone(pid, 20), f"{name} pid={pid} orphaned\n{run.text()}"
    # The launcher had no chance to clean up its sockets.
    sockets.unlink_run_sockets(f".rsg={run.proc.pid}")
    assert not any(path.exists() for path in sockets.run_socket_paths(f".rsg={run.proc.pid}"))
