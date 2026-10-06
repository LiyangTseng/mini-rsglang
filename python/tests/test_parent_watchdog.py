"""Unit tests for rsglang.backend.start_parent_watchdog (G-01-3 / WR-02)."""

from __future__ import annotations

import os
import queue
import signal
import subprocess
import sys
import time
from types import SimpleNamespace

import pytest

pytestmark = pytest.mark.slow  # each test spawns a process


def _run_child(program: str, *args: object) -> tuple[subprocess.CompletedProcess[str], float]:
    start = time.monotonic()
    result = subprocess.run(
        [sys.executable, "-c", program, *(str(a) for a in args)],
        capture_output=True,
        text=True,
        timeout=20,
    )
    return result, time.monotonic() - start


def test_exits_at_once_when_parent_is_not_the_launcher():
    program = (
        "import sys, time\n"
        "from rsglang.backend import start_parent_watchdog\n"
        "print('calling', flush=True)\n"
        "start_parent_watchdog(int(sys.argv[1]), poll_interval=0.1)\n"
        "print('returned', flush=True)\n"
        "time.sleep(30)\n"
        "print('survived')\n"
    )
    # The child's parent is this process, so this process's own parent is never its parent.
    result, elapsed = _run_child(program, os.getppid())
    assert result.returncode == 1, result.stderr
    assert "calling" in result.stdout
    assert "returned" not in result.stdout
    assert "survived" not in result.stdout
    assert "Traceback" not in result.stderr
    assert elapsed < 5


def test_stays_alive_while_parent_is_the_launcher():
    program = (
        "import sys, time\n"
        "from rsglang.backend import start_parent_watchdog\n"
        "start_parent_watchdog(int(sys.argv[1]), poll_interval=0.1)\n"
        "print('armed', flush=True)\n"
        "time.sleep(0.5)\n"
        "print('survived')\n"
    )
    result, _ = _run_child(program, os.getpid())
    assert result.returncode == 0, result.stderr
    assert "armed" in result.stdout
    assert "survived" in result.stdout
    assert "Traceback" not in result.stderr


@pytest.mark.parametrize("mode", ["prctl-fails", "cdll-fails", "prctl-missing"])
def test_prctl_failure_degrades_to_polling(mode):
    program = (
        "import ctypes, errno, sys, threading, time, types\n"
        "import rsglang.backend as backend\n"
        "calls = []\n"
        "if sys.argv[2] == 'prctl-fails':\n"
        "    def _prctl(*a):\n"
        "        calls.append(a)\n"
        "        ctypes.set_errno(errno.EPERM)\n"
        "        return -1\n"
        "    def _cdll(*a, **kw):\n"
        "        return types.SimpleNamespace(prctl=_prctl)\n"
        "    ctypes.CDLL = _cdll\n"
        "elif sys.argv[2] == 'cdll-fails':\n"
        "    def _cdll(*a, **kw):\n"
        "        raise OSError(errno.ENOENT, 'no libc')\n"
        "    ctypes.CDLL = _cdll\n"
        "elif sys.argv[2] == 'prctl-missing':\n"
        "    def _cdll(*a, **kw):\n"
        "        return types.SimpleNamespace()\n"
        "    ctypes.CDLL = _cdll\n"
        "sys.platform = 'linux'\n"
        "backend.start_parent_watchdog(int(sys.argv[1]), poll_interval=0.1)\n"
        "print('armed')\n"
        "names = [t.name for t in threading.enumerate() if t.is_alive()]\n"
        "if 'rsglang-parent-watchdog' in names:\n"
        "    print('polling')\n"
        "if sys.argv[2] == 'prctl-fails':\n"
        "    print('CALLS:' + repr(calls))\n"
        "    print('ARGTYPES_LEN:' + str(len(_prctl.argtypes) if hasattr(_prctl, 'argtypes') else -1))\n"
        "time.sleep(0.5)\n"
        "print('survived')\n"
    )
    result, _ = _run_child(program, os.getpid(), mode)
    assert result.returncode == 0, result.stderr
    assert "armed" in result.stdout
    assert "polling" in result.stdout
    assert "survived" in result.stdout
    assert "PDEATHSIG unavailable" in result.stderr
    assert "Traceback" not in result.stderr
    if mode == "prctl-fails":
        calls_line = next(line for line in result.stdout.splitlines() if line.startswith("CALLS:"))
        argtypes_line = next(
            line for line in result.stdout.splitlines() if line.startswith("ARGTYPES_LEN:")
        )
        assert calls_line == "CALLS:[(1, " + str(int(signal.SIGKILL)) + ", 0, 0, 0)]"
        assert argtypes_line == "ARGTYPES_LEN:5"
    if mode == "prctl-missing":
        pdeathsig_line = next(
            line for line in result.stderr.splitlines() if "PDEATHSIG unavailable" in line
        )
        assert "prctl" in pdeathsig_line


def test_watchdog_startup_failure_reaches_launcher_as_error_envelope(monkeypatch):
    import rsglang.backend as backend

    def _raise(*args, **kwargs):
        raise RuntimeError("watchdog setup failed")

    monkeypatch.setattr(backend, "start_parent_watchdog", _raise)
    q: "queue.Queue[dict]" = queue.Queue()
    args = SimpleNamespace(tp_info=SimpleNamespace(rank=0))
    with pytest.raises(RuntimeError, match="watchdog setup failed"):
        backend.run_scheduler(args, q, "sha", os.getpid())
    envelope = q.get_nowait()
    assert envelope["kind"] == "error"
    assert envelope["rank"] == 0
    assert "watchdog setup failed" in envelope["traceback"]
    assert q.empty()


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="PR_SET_PDEATHSIG is Linux-only")
def test_linux_arms_pdeathsig_sigkill():
    program = (
        "import ctypes, os\n"
        "from rsglang.backend import start_parent_watchdog\n"
        "start_parent_watchdog(os.getppid())\n"
        "value = ctypes.c_int(-1)\n"
        "PR_GET_PDEATHSIG = 2\n"
        "rc = ctypes.CDLL(None, use_errno=True).prctl(PR_GET_PDEATHSIG, ctypes.byref(value), 0, 0, 0)\n"
        "assert rc == 0, ctypes.get_errno()\n"
        "print(value.value)\n"
    )
    result, _ = _run_child(program)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == str(int(signal.SIGKILL))
