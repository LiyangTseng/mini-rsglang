"""Launch the profiled server, discover and role-identify its children via
py-spy, tear it down cleanly, and sum RSS/PSS across the process tree (BENCH-01).

The GPU box runs the real `python -m rsglang.launch --frontend python` server
and the real py-spy binary; the Mac runs rsglang.testing.fake_profile_env's
stand-ins for both. This module's code path is identical either way -- only
the binaries resolved from PATH differ.
"""

from __future__ import annotations

import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Mapping, Sequence

import psutil

from . import sidecar

ROLES = sidecar.ROLES
ROLE_MARKERS = {"scheduler": "_run_scheduler", "tokenizer": "tokenize_worker"}

_MINISGL_IPC_INDICES = range(5)
_PERMISSION_RE = re.compile(r"permission denied|operation not permitted", re.IGNORECASE)


@dataclass
class ServerHandle:
    proc: "subprocess.Popen"
    pgid: int
    t_launch: float
    log_path: Path


class ServerExited(RuntimeError):
    pass


class RoleError(RuntimeError):
    pass


class PySpyMissingError(RuntimeError):
    pass


class PySpyPermissionError(RuntimeError):
    pass


def server_argv(template: str | None, *, python: str, model: str, port: int) -> list[str]:
    if template is None:
        return [python, "-m", "rsglang.launch", "--frontend", "python", "--model", model, "--port", str(port)]
    return shlex.split(template.format(python=python, model=model, port=port))


def launch_server(argv: Sequence[str], *, env: Mapping[str, str], log_path: Path) -> ServerHandle:
    log_path = Path(log_path)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    log_file = open(log_path, "ab")
    try:
        t_launch = time.perf_counter()
        proc = subprocess.Popen(
            list(argv),
            env=dict(env),
            stdout=log_file,
            stderr=log_file,
            start_new_session=True,
        )
    finally:
        # The child holds its own duplicated fd; closing the parent's copy is safe.
        log_file.close()
    return ServerHandle(proc=proc, pgid=proc.pid, t_launch=t_launch, log_path=log_path)


def _tail_log(log_path: Path, n: int) -> str:
    try:
        text = Path(log_path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    lines = text.splitlines()
    return "\n".join(lines[-n:])


def wait_ready(handle: ServerHandle, *, port: int, timeout_s: float) -> float:
    deadline = time.perf_counter() + timeout_s
    url = f"http://127.0.0.1:{port}/v1/models"
    while True:
        try:
            urllib.request.urlopen(url, timeout=1)
            return time.perf_counter()
        except (urllib.error.URLError, OSError, ConnectionError):
            pass

        if handle.proc.poll() is not None:
            tail = _tail_log(handle.log_path, 40)
            raise ServerExited(
                f"server exited with code {handle.proc.returncode} before ready; last 40 log lines:\n{tail}"
            )

        if time.perf_counter() > deadline:
            raise TimeoutError(f"server not ready after {timeout_s:g}s")

        time.sleep(0.5)


def discover_children(top_pid: int) -> list[int]:
    return [c.pid for c in psutil.Process(top_pid).children(recursive=False)]


def py_spy_base(*, sudo: bool = False) -> list[str]:
    path = shutil.which("py-spy")
    if not path:
        raise PySpyMissingError("py-spy not found on PATH")
    path = os.path.abspath(path)
    if sudo:
        # sudo resets PATH, which is why an absolute path is required here.
        return ["sudo", "-n", path]
    return [path]


def _without_sudo(base: Sequence[str]) -> list[str]:
    if base and base[0] == "sudo":
        return list(base[2:])
    return list(base)


def py_spy_version(base: Sequence[str]) -> str | None:
    argv = _without_sudo(base)
    try:
        out = subprocess.run([*argv, "--version"], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if out.returncode != 0:
        return None
    text = (out.stdout or out.stderr).strip()
    return text or None


def py_spy_dump(pid: int, *, base: Sequence[str]) -> str:
    argv = [*base, "dump", "--nonblocking", "--pid", str(pid)]
    try:
        out = subprocess.run(argv, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    if out.returncode != 0:
        combined = (out.stdout or "") + (out.stderr or "")
        if _PERMISSION_RE.search(combined):
            raise PySpyPermissionError(
                f"py-spy was denied permission to attach to pid {pid}. Remediation options: "
                "(1) sudo setcap CAP_SYS_PTRACE+ep <py-spy binary>, "
                "(2) pass --py-spy-sudo with a NOPASSWD sudo rule for py-spy, "
                "or (3) set kernel.yama.ptrace_scope=0 for this run."
            )
        return ""
    return out.stdout


def classify_dump(text: str) -> str:
    has_scheduler = ROLE_MARKERS["scheduler"] in text
    has_tokenizer = ROLE_MARKERS["tokenizer"] in text
    if has_scheduler and has_tokenizer:
        raise ValueError(
            f"dump contains both {ROLE_MARKERS['scheduler']!r} and {ROLE_MARKERS['tokenizer']!r} markers"
        )
    if has_scheduler:
        return "scheduler"
    if has_tokenizer:
        return "tokenizer"
    return "other"


def identify_roles(top_pid: int, dumps: Mapping[int, str]) -> dict:
    scheduler_pids: list[int] = []
    tokenizer_pids: list[int] = []
    other: list[int] = []
    for pid, text in dumps.items():
        role = classify_dump(text)
        if role == "scheduler":
            scheduler_pids.append(pid)
        elif role == "tokenizer":
            tokenizer_pids.append(pid)
        else:
            other.append(pid)

    if len(scheduler_pids) != 1 or len(tokenizer_pids) != 1:
        raise RoleError(
            "Phase 2 profiles the default topology (--num-tokenizer 0, --tp-size 1), which has "
            f"exactly one scheduler child and one tokenizer child; found {len(scheduler_pids)} "
            f"scheduler dump(s) and {len(tokenizer_pids)} tokenizer dump(s)"
        )

    return {
        "api_server": top_pid,
        "scheduler": scheduler_pids[0],
        "tokenizer": tokenizer_pids[0],
        "other": other,
    }


def _pid_alive(pid: int) -> bool:
    if not psutil.pid_exists(pid):
        return False
    try:
        return psutil.Process(pid).status() != psutil.STATUS_ZOMBIE
    except psutil.NoSuchProcess:
        return False


def teardown(handle: ServerHandle, *, extra_pids: Iterable[int] = (), grace_s: float = 60.0) -> list[int]:
    extra_pids = list(extra_pids)
    pgid = handle.pgid

    try:
        os.killpg(pgid, signal.SIGINT)
    except (ProcessLookupError, PermissionError):
        pass

    deadline = time.monotonic() + grace_s
    try:
        handle.proc.wait(timeout=max(0.0, deadline - time.monotonic()))
    except subprocess.TimeoutExpired:
        pass
    for pid in extra_pids:
        while time.monotonic() < deadline and _pid_alive(pid):
            time.sleep(0.2)

    try:
        os.killpg(pgid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass

    kill_deadline = time.monotonic() + 10.0
    try:
        handle.proc.wait(timeout=max(0.0, kill_deadline - time.monotonic()))
    except subprocess.TimeoutExpired:
        pass
    for pid in extra_pids:
        while time.monotonic() < kill_deadline and _pid_alive(pid):
            time.sleep(0.2)

    for i in _MINISGL_IPC_INDICES:
        try:
            Path(f"/tmp/minisgl_{i}.pid={pgid}").unlink()
        except OSError:
            pass

    return [pid for pid in (handle.proc.pid, *extra_pids) if _pid_alive(pid)]


def tree_memory(top_pid: int) -> dict:
    pids_to_check = [top_pid]
    try:
        top = psutil.Process(top_pid)
        pids_to_check.extend(c.pid for c in top.children(recursive=True))
    except psutil.NoSuchProcess:
        pass

    seen_pids: list[int] = []
    rss_total = 0
    pss_total = 0
    vanished = 0
    is_linux = sys.platform.startswith("linux")
    pss_ok = True

    for pid in pids_to_check:
        try:
            proc = psutil.Process(pid)
            rss_total += proc.memory_info().rss
        except (psutil.NoSuchProcess, psutil.ZombieProcess):
            vanished += 1
            continue
        seen_pids.append(pid)
        if is_linux and pss_ok:
            try:
                pss_total += proc.memory_full_info().pss
            except psutil.AccessDenied:
                pss_ok = False
            except (psutil.NoSuchProcess, psutil.ZombieProcess):
                pass

    pss_bytes = pss_total if (is_linux and pss_ok) else None
    return {"pids": seen_pids, "rss_bytes": rss_total, "pss_bytes": pss_bytes, "vanished": vanished}
