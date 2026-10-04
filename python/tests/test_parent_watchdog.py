"""Unit tests for rsglang.backend.start_parent_watchdog (G-01-3 / WR-02)."""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import time

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
        "start_parent_watchdog(int(sys.argv[1]), poll_interval=0.1)\n"
        "time.sleep(30)\n"
        "print('survived')\n"
    )
    # The child's parent is this process, so this process's own parent is never its parent.
    result, elapsed = _run_child(program, os.getppid())
    assert result.returncode == 1, result.stderr
    assert "survived" not in result.stdout
    assert elapsed < 5


def test_stays_alive_while_parent_is_the_launcher():
    program = (
        "import sys, time\n"
        "from rsglang.backend import start_parent_watchdog\n"
        "start_parent_watchdog(int(sys.argv[1]), poll_interval=0.1)\n"
        "time.sleep(0.5)\n"
        "print('survived')\n"
    )
    result, _ = _run_child(program, os.getpid())
    assert result.returncode == 0, result.stderr
    assert "survived" in result.stdout


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
