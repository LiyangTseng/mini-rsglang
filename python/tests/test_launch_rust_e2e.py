"""End-to-end tracer: launcher + fake scheduler (upstream's real queues) + real rsg-server."""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import threading
import time
import urllib.request
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
    "--port", "0",
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
    def __init__(
        self,
        rust_bin: Path,
        status_dir: Path,
        mode: str = "ok",
        ready_timeout: float = 60,
        extra_env: dict[str, str] | None = None,
        extra_args: list[str] | None = None,
    ):
        env = {
            **os.environ,
            "RSGLANG_SCHEDULER_FACTORY": FACTORY_PATH,
            STATUS_DIR_ENV: str(status_dir),
            MODE_ENV: mode,
            "RUST_LOG": "info",
            **(extra_env or {}),
        }
        self.proc = subprocess.Popen(
            [sys.executable, "-m", "rsglang.launch", "--frontend", "rust",
             "--rust-bin", str(rust_bin), "--ready-timeout", f"{ready_timeout:g}", *UPSTREAM_ARGS,
             *(extra_args or [])],
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
    # D-10: rsg-server is spawned before the backend reports ready, so its own
    # front-half startup (tokenizer load, socket open) overlaps backend weight
    # loading rather than following it. Checked against "rsg-server starting"
    # (the very first rsg-server log line, emitted before Phase 5's tokenizer
    # load) rather than "awaiting handshake on stdin": plan 05-08 moved that
    # second line to after the tokenizer load completes, so on this test's
    # near-instant fake scheduler -- unlike a real multi-second GPU weight
    # load -- tokenizer-load latency can legitimately outlast the fake
    # backend's own "ready" line without violating D-10's actual intent.
    assert launcher.index_of("rsg-server starting") < launcher.index_of("backend ready; handshake sent")

    pid = launcher.proc.pid
    sock0 = Path(f"/tmp/minisgl_0.rsg={pid}")
    sock1 = Path(f"/tmp/minisgl_1.rsg={pid}")
    assert sock0.exists()
    assert sock1.exists()

    # D-07: the scheduler side sees a bound peer on _1 (Rust).
    assert _wait_until((tmp_path / "detok_peer_connected").exists, 15)

    children = launcher.pids()
    assert set(children) == {"rsg-server", "scheduler"}, launcher.lines

    # plan 05-08: the launcher forwards the upstream --host/--port the user asked
    # for (here --port 0, an ephemeral port); confirm the real rsg-server binary
    # is listening on the forwarded port and serving end to end.
    listening_line = launcher.wait_for(r"http server listening addr=", 10)
    port_match = re.search(r"addr=\S+:(\d+)", listening_line)
    assert port_match, listening_line
    port = int(port_match.group(1))
    launcher.wait_for("ready to serve", 10)

    with urllib.request.urlopen(f"http://127.0.0.1:{port}/v1/models", timeout=10) as resp:
        assert resp.status == 200
        body = json.loads(resp.read())
    assert body["data"][0]["id"] == "Qwen/Qwen3-0.6B", body

    launcher.proc.send_signal(signal.SIGTERM)
    assert launcher.proc.wait(timeout=30) == 0, "\n".join(launcher.lines)
    for name, child in children.items():
        assert _wait_until(lambda: not _alive(child), 15), f"{name} pid={child} still alive"
    assert not sock0.exists()
    assert not sock1.exists()


def _gone(pid: int, timeout: float) -> bool:
    return _wait_until(lambda: not _alive(pid), timeout)


# --- D-12 stop contract: a group SIGINT (terminal Ctrl-C) tears the run down and exits 0 ---


def _group_sigint_clean_stop(run: LauncherRun) -> None:
    children = run.pids()
    assert {"rsg-server", "scheduler"} <= set(children), run.text()

    # The launcher leads its own process group, so this reaches the launcher, rsg-server
    # and the scheduler at once, exactly like a terminal Ctrl-C.
    os.killpg(run.proc.pid, signal.SIGINT)

    assert run.finish(30) == 0, run.text()
    # The scheduler child's inherited stderr may print a KeyboardInterrupt traceback, so
    # only the launcher's own lines are judged.
    own = [line for line in run.lines if line.startswith("rsglang.launch:")]
    assert "rsglang.launch: exit code 0" in own, run.text()
    for bad in ("exited with code", "failed", "lines of rsg-server stderr", "escalating to SIGKILL"):
        assert not any(bad in line for line in own), f"launcher reported {bad!r}\n{run.text()}"
    for name, pid in children.items():
        assert _gone(pid, 15), f"{name} pid={pid} still alive\n{run.text()}"
    assert not any(path.exists() for path in sockets.run_socket_paths(f".rsg={run.proc.pid}"))


def test_group_sigint_after_ready_exits_0(make_launcher):
    run = make_launcher()
    run.wait_for("backend ready; handshake sent", 90)
    _group_sigint_clean_stop(run)


def test_group_sigint_while_scheduler_boots_exits_0(make_launcher):
    run = make_launcher()
    run.wait_for("spawned scheduler rank=0", 30)
    _group_sigint_clean_stop(run)  # lands while the scheduler is still booting its interpreter


def test_group_sigint_while_scheduler_hangs_exits_0(make_launcher, tmp_path):
    run = make_launcher(mode="hang_before_ready")
    run.wait_for("spawned scheduler rank=0", 30)
    assert _wait_until((tmp_path / "hang_entered").exists, 60), run.text()
    _group_sigint_clean_stop(run)  # the scheduler's KeyboardInterrupt envelope reaches the launcher


# --- D-12 failure contract: every failure exits non-zero, prints its cause, leaves no child ---


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


# --- CR-01: an unexpected (not-just-anticipated) error must still SIGKILL the group ---

# Gated on "rsglang.launch" in sys.orig_argv: this patches the launcher process only, never
# a spawned --multiprocessing-fork child, the resource tracker, or pytest itself (IN-13).
_SPAWN_LOOP_HOOK = """\
import os, sys, time

if "rsglang.launch" in sys.orig_argv:
    import multiprocessing.process as _mp_process

    _orig_start = _mp_process.BaseProcess.start

    def _start(self, *args, **kwargs):
        if self.name == "rsglang-TP1-scheduler":
            # Wait for rank 0 to be fully up before failing. Without this, rank 0 is usually
            # still unpickling its arguments when the error escapes, and multiprocessing's
            # exit finalizers unlink the ready queue's semaphore out from under it, making the
            # unfixed launcher exit 1 with no orphans by a timing accident (seen during
            # planning) rather than RED for the right reason.
            status_dir = os.environ.get("RSGLANG_FAKE_STATUS_DIR")
            deadline = time.monotonic() + 60.0
            while status_dir and not os.path.exists(os.path.join(status_dir, "detok_peer_connected")):
                if time.monotonic() >= deadline:
                    break
                time.sleep(0.05)
            raise OSError("injected: TP rank 1 failed to start")
        return _orig_start(self, *args, **kwargs)

    _mp_process.BaseProcess.start = _start
"""

_HANDSHAKE_HOOK = """\
import sys

if "rsglang.launch" in sys.orig_argv:
    import rsglang.handshake as _handshake

    def _raise_handshake_error(payload):
        raise ValueError("injected: handshake keys drifted")

    _handshake.encode_handshake_line = _raise_handshake_error
"""


@pytest.mark.parametrize(
    "hook, extra_args",
    [
        pytest.param(_SPAWN_LOOP_HOOK, ["--tp-size", "2"], id="spawn_loop"),
        pytest.param(_HANDSHAKE_HOOK, [], id="handshake_encode"),
    ],
)
def test_unexpected_error_leaves_no_orphans(make_launcher, tmp_path, hook, extra_args):
    # CR-01 / D-12: an exception _run_rust_mode does not anticipate must still print the
    # cause, SIGKILL the launcher's whole process group, and leave no rsg-server or
    # scheduler process behind.
    hook_dir = tmp_path / "hook"
    hook_dir.mkdir()
    (hook_dir / "sitecustomize.py").write_text(hook)
    pythonpath = os.pathsep.join(p for p in (str(hook_dir), os.environ.get("PYTHONPATH")) if p)
    run = make_launcher(extra_env={"PYTHONPATH": pythonpath}, extra_args=extra_args)
    run.wait_for("injected: ", 90)
    try:
        code = run.finish(30)
    except subprocess.TimeoutExpired:
        pytest.fail(
            "launcher still running 30 s after the injected error; its children were "
            f"never killed (CR-01)\n{run.text()}"
        )
    assert code != 0, run.text()  # -9: the group SIGKILL ends the launcher too
    own = [line for line in run.lines if line.startswith("rsglang.launch:")]
    assert any("unexpected error in the launcher" in line for line in own), run.text()
    assert set(run.pids()) == {"rsg-server", "scheduler"}, run.text()
    for name, pid in run.pids().items():
        assert _gone(pid, 15), f"{name} pid={pid} orphaned\n{run.text()}"


def test_launcher_sigkill_during_scheduler_boot_leaves_no_orphans(make_launcher, tmp_path):
    # G-01-3 / WR-02: a launcher SIGKILLed while its scheduler child is still booting must not
    # leave the scheduler behind. The boot window is widened without touching repo code: a
    # test-only sitecustomize sleeps in spawned multiprocessing children (only they carry
    # --multiprocessing-fork in their argv) before they import anything.
    slowboot = tmp_path / "slowboot"
    slowboot.mkdir()
    (slowboot / "sitecustomize.py").write_text(
        "import sys, time\n"
        "if '--multiprocessing-fork' in sys.orig_argv:\n"
        "    time.sleep(3.0)\n"
    )
    pythonpath = os.pathsep.join(p for p in (str(slowboot), os.environ.get("PYTHONPATH")) if p)
    run = make_launcher(extra_env={"PYTHONPATH": pythonpath})
    run.wait_for("spawned scheduler rank=0", 30)
    children = run.pids()
    assert set(children) == {"rsg-server", "scheduler"}, run.text()
    os.kill(run.proc.pid, signal.SIGKILL)  # the launcher only, not its group
    run.finish(10)
    assert _gone(children["scheduler"], 30), f"scheduler pid={children['scheduler']} orphaned\n{run.text()}"
    assert _gone(children["rsg-server"], 15), f"rsg-server pid={children['rsg-server']} orphaned\n{run.text()}"
    # The launcher had no chance to clean up its sockets.
    sockets.unlink_run_sockets(f".rsg={run.proc.pid}")
    assert not any(path.exists() for path in sockets.run_socket_paths(f".rsg={run.proc.pid}"))
