"""Mac-side tests for scripts/baseline_profile.py's `discover` subcommand
(BENCH-01). Task 1's tracer (test_discover_end_to_end) proves the whole
discovery and instrumentation pipeline end to end against a stand-in server
tree that mimics minisgl's exact process topology; nothing else in the phase
depends on this path until that test is green. Task 2 covers role-ID edge
cases, pure helpers and the three failure-exit-code paths.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

import psutil
import pytest

from rsglang.profiling import procs, sidecar

REPO_ROOT = Path(__file__).resolve().parents[2]


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _write_pyspy_stub(tmp_path: Path) -> Path:
    """Write an executable `py-spy` that execs the fake_profile_env py-spy stand-in."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / "py-spy"
    stub.write_text(
        f'#!/bin/bash\nexec "{sys.executable}" -m rsglang.testing.fake_profile_env py-spy "$@"\n'
    )
    stub.chmod(0o755)
    return bin_dir


def _write_hyperfine_stub(bin_dir: Path) -> Path:
    """Write an executable `hyperfine` into bin_dir that execs the fake_profile_env
    hyperfine stand-in."""
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / "hyperfine"
    stub.write_text(
        f'#!/bin/bash\nexec "{sys.executable}" -m rsglang.testing.fake_profile_env hyperfine "$@"\n'
    )
    stub.chmod(0o755)
    return bin_dir


def _pid_alive(pid: int) -> bool:
    if not psutil.pid_exists(pid):
        return False
    try:
        return psutil.Process(pid).status() != psutil.STATUS_ZOMBIE
    except psutil.NoSuchProcess:
        return False


def _wait_until_dead(pids, timeout_s: float = 15.0) -> set:
    remaining = set(pids)
    deadline = time.monotonic() + timeout_s
    while remaining and time.monotonic() < deadline:
        remaining = {pid for pid in remaining if _pid_alive(pid)}
        if remaining:
            time.sleep(0.5)
    return remaining


def _run_discover(args: list, env: dict, timeout: int = 180) -> "subprocess.CompletedProcess[str]":
    return subprocess.run(
        [sys.executable, "scripts/baseline_profile.py", "discover", *args],
        env=env,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def _run_cli(command: str, args: list, env: dict | None = None, timeout: int = 180) -> "subprocess.CompletedProcess[str]":
    return subprocess.run(
        [sys.executable, "scripts/baseline_profile.py", command, *args],
        env=env if env is not None else dict(os.environ),
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


# --- discover_end_to_end -----------------------------------------------------------


@pytest.mark.slow
def test_discover_end_to_end(tmp_path):
    bin_dir = _write_pyspy_stub(tmp_path)
    role_map_path = tmp_path / "roles.json"
    port = _free_port()
    out_path = tmp_path / "d.json"
    work_dir = tmp_path / "w"

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["RSGLANG_FAKE_PROFILE_ROLE_MAP"] = str(role_map_path)

    result = _run_discover(
        [
            "--port", str(port),
            "--timeout", "60",
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--settle-s", "1.5",
            "--sample-interval-s", "0.2",
            "--server-cmd",
            "{python} -m rsglang.testing.fake_profile_env server --port {port}",
        ],
        env,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert doc["meta"]["mode"] == "discover"
    assert sidecar.validate_sidecar(doc) == []

    role_map = json.loads(role_map_path.read_text())
    processes = doc["discovery"]["processes"]
    pids = {processes["api_server"], processes["scheduler"], processes["tokenizer"]}
    assert len(pids) == 3

    api_server_pid = next(int(pid) for pid, role in role_map.items() if role == "api_server")
    assert processes["api_server"] == api_server_pid

    hook_active = doc["discovery"]["hook_active"]
    assert all(hook_active.values()), hook_active
    gc_count = doc["discovery"]["gc_count"]
    assert gc_count["scheduler"] >= 1
    assert gc_count["tokenizer"] >= 1

    still_alive = _wait_until_dead(pids)
    assert not still_alive, f"still alive: {still_alive}"


# --- role_identification ------------------------------------------------------------


def test_role_identification():
    sched_text = "_run_scheduler (minisgl/server/launch.py:31)"
    tok_text = "tokenize_worker (minisgl/tokenizer/server.py:60)"
    other_text = "main (multiprocessing/resource_tracker.py:200)"

    assert procs.classify_dump(sched_text) == "scheduler"
    assert procs.classify_dump(tok_text) == "tokenizer"
    assert procs.classify_dump(other_text) == "other"
    assert procs.classify_dump("") == "other"
    with pytest.raises(ValueError):
        procs.classify_dump(sched_text + tok_text)

    roles = procs.identify_roles(10, {11: sched_text, 12: tok_text, 13: other_text})
    assert roles == {"api_server": 10, "scheduler": 11, "tokenizer": 12, "other": [13]}

    with pytest.raises(procs.RoleError, match="--num-tokenizer 0"):
        procs.identify_roles(10, {11: other_text})

    with pytest.raises(procs.RoleError):
        procs.identify_roles(10, {11: tok_text, 12: tok_text})


# --- server_argv ---------------------------------------------------------------------


def test_server_argv():
    assert procs.server_argv(None, python="/p", model="M", port=7) == [
        "/p", "-m", "rsglang.launch", "--frontend", "python", "--model", "M", "--port", "7",
    ]
    assert procs.server_argv("{python} -m x --port {port}", python="/p", model="M", port=7) == [
        "/p", "-m", "x", "--port", "7",
    ]


# --- tree_memory_current_process ------------------------------------------------------


def test_tree_memory_current_process(monkeypatch):
    result = procs.tree_memory(os.getpid())
    assert isinstance(result["rss_bytes"], int) and result["rss_bytes"] > 0
    assert os.getpid() in result["pids"]
    if sys.platform.startswith("linux"):
        assert isinstance(result["pss_bytes"], int) and result["pss_bytes"] >= 0
    else:
        assert result["pss_bytes"] is None

    vanished_pid = 999999999
    real_process_cls = psutil.Process

    class _FakeChild:
        pid = vanished_pid

    def fake_children(self, recursive=False):
        return [_FakeChild()]

    def fake_process_ctor(pid=None):
        if pid == vanished_pid:
            raise psutil.NoSuchProcess(pid)
        return real_process_cls(pid)

    # Patch children() first, while psutil.Process still refers to the real class.
    monkeypatch.setattr(psutil.Process, "children", fake_children)
    monkeypatch.setattr(psutil, "Process", fake_process_ctor)

    result2 = procs.tree_memory(os.getpid())
    assert result2["vanished"] == 1
    assert vanished_pid not in result2["pids"]


# --- discover_missing_py_spy_exits_2 ---------------------------------------------------


@pytest.mark.slow
def test_discover_missing_py_spy_exits_2(tmp_path):
    empty_bin = tmp_path / "emptybin"
    empty_bin.mkdir()
    role_map_path = tmp_path / "roles.json"
    port = _free_port()

    env = dict(os.environ)
    env["PATH"] = str(empty_bin)
    env["RSGLANG_FAKE_PROFILE_ROLE_MAP"] = str(role_map_path)

    result = _run_discover(
        [
            "--port", str(port),
            "--timeout", "10",
            "--out", str(tmp_path / "d.json"),
            "--work-dir", str(tmp_path / "w"),
            "--server-cmd",
            "{python} -m rsglang.testing.fake_profile_env server --port {port}",
        ],
        env,
        timeout=60,
    )
    assert result.returncode == 2, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "py-spy" in result.stderr
    assert not role_map_path.exists()


# --- discover_permission_denied_exits_2 -------------------------------------------------


@pytest.mark.slow
def test_discover_permission_denied_exits_2(tmp_path):
    bin_dir = _write_pyspy_stub(tmp_path)
    role_map_path = tmp_path / "roles.json"
    port = _free_port()

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["RSGLANG_FAKE_PROFILE_ROLE_MAP"] = str(role_map_path)
    env["RSGLANG_FAKE_PYSPY_MODE"] = "denied"

    result = _run_discover(
        [
            "--port", str(port),
            "--timeout", "30",
            "--out", str(tmp_path / "d.json"),
            "--work-dir", str(tmp_path / "w"),
            "--settle-s", "0.5",
            "--sample-interval-s", "0.2",
            "--server-cmd",
            "{python} -m rsglang.testing.fake_profile_env server --port {port}",
        ],
        env,
        timeout=90,
    )
    assert result.returncode == 2, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "CAP_SYS_PTRACE" in result.stderr
    assert "--py-spy-sudo" in result.stderr

    alive_pids: set = set()
    if role_map_path.exists():
        role_map = json.loads(role_map_path.read_text())
        alive_pids = {int(pid) for pid in role_map}
    still_alive = _wait_until_dead(alive_pids)
    assert not still_alive, f"still alive: {still_alive}"


# --- discover_server_exits_early ---------------------------------------------------------


@pytest.mark.slow
def test_discover_server_exits_early(tmp_path):
    bin_dir = _write_pyspy_stub(tmp_path)
    port = _free_port()

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

    result = _run_discover(
        [
            "--port", str(port),
            "--timeout", "10",
            "--out", str(tmp_path / "d.json"),
            "--work-dir", str(tmp_path / "w"),
            "--server-cmd",
            "{python} -c \"import sys; print('boom'); sys.exit(3)\"",
        ],
        env,
        timeout=60,
    )
    assert result.returncode == 1, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "boom" in result.stderr


# --- run_s1_end_to_end (Plan 02-08, Task 1) -----------------------------------------


@pytest.mark.slow
def test_run_s1_end_to_end(tmp_path):
    from rsglang.testing.fake_profile_env import (
        FAKE_ACTIVE_SAMPLES,
        FAKE_GIL_SAMPLES,
        FAKE_RADIX_SAMPLES,
    )

    bin_dir = _write_pyspy_stub(tmp_path)
    role_map_path = tmp_path / "roles.json"
    port = _free_port()
    out_path = tmp_path / "p.json"
    work_dir = tmp_path / "w"

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["RSGLANG_FAKE_PROFILE_ROLE_MAP"] = str(role_map_path)

    result = _run_cli(
        "run",
        [
            "--scenarios", "s1",
            "--s1-agents", "8",
            "--s1-duration-s", "2",
            "--s1-max-tokens", "16",
            "--s1-think-max-s", "0.05",
            "--sample-interval-s", "0.2",
            "--server-cmd",
            "{python} -m rsglang.testing.fake_profile_env server --port {port}",
            "--port", str(port),
            "--timeout", "60",
            "--out", str(out_path),
            "--work-dir", str(work_dir),
        ],
        env=env,
        timeout=180,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_sidecar(doc, require_scenarios=("s1_cancel",)) == []
    assert doc["meta"]["mode"] == "run"

    entry = doc["scenarios"]["s1_cancel"]
    assert entry["radix"]["share"] == FAKE_RADIX_SAMPLES / FAKE_ACTIVE_SAMPLES
    assert entry["cpu"]["scheduler"]["active_samples"] == FAKE_ACTIVE_SAMPLES
    assert entry["cpu"]["api_server"]["buckets"]["ipc_zmq"]["samples"] == 2
    assert entry["cpu"]["tokenizer"]["buckets"]["tokenize"]["samples"] == 2
    for role in ("api_server", "scheduler", "tokenizer"):
        assert entry["cpu"][role]["gil_samples"] == FAKE_GIL_SAMPLES

    assert entry["gc"]["scheduler"]["count"] >= 1
    assert entry["gc"]["tokenizer"]["count"] >= 1

    assert entry["requests"]["sent"] >= 8
    assert entry["requests"]["failed"] == 0

    for role in ("api_server", "scheduler", "tokenizer"):
        per_role = entry["memory"]["per_role"][role]
        assert len(per_role["rss_curve"]) > 0
        assert isinstance(per_role["top_alloc_sites"], list)

    role_map = json.loads(role_map_path.read_text())
    alive_pids = {int(pid) for pid in role_map}
    still_alive = _wait_until_dead(alive_pids)
    assert not still_alive, f"still alive: {still_alive}"


# --- run_refuses_canonical_out_off_gpu (Plan 02-08, Task 1) ---------------------------


@pytest.mark.slow
@pytest.mark.skipif(
    sys.platform.startswith("linux") and shutil.which("nvidia-smi") is not None,
    reason="this host would actually attempt a canonical GPU write",
)
def test_run_refuses_canonical_out_off_gpu(tmp_path):
    bin_dir = _write_pyspy_stub(tmp_path)
    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

    canonical_path = REPO_ROOT / sidecar.CANONICAL_OUT
    existed_before = canonical_path.exists()
    before_mtime = canonical_path.stat().st_mtime if existed_before else None

    start = time.monotonic()
    result = _run_cli(
        "run",
        ["--scenarios", "s1", "--work-dir", str(tmp_path / "w")],
        env=env,
        timeout=20,
    )
    elapsed = time.monotonic() - start

    assert result.returncode == 2, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert elapsed < 20
    assert "--out" in result.stderr

    if existed_before:
        assert canonical_path.stat().st_mtime == before_mtime
    else:
        assert not canonical_path.exists()


# --- validate_cli (Plan 02-08, Task 1) -------------------------------------------------


@pytest.mark.slow
def test_validate_cli(tmp_path):
    from test_profile_sidecar import make_valid_doc

    valid_path = tmp_path / "valid.json"
    doc = make_valid_doc()
    valid_path.write_text(json.dumps(doc))

    result = _run_cli("validate", [str(valid_path)], timeout=30)
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    gpu_path = tmp_path / "gpu.json"
    gpu_doc = json.loads(json.dumps(doc))
    gpu_doc["meta"]["platform"] = "darwin"
    gpu_path.write_text(json.dumps(gpu_doc))

    result2 = _run_cli("validate", [str(gpu_path), "--require-gpu"], timeout=30)
    assert result2.returncode == 1, f"stdout={result2.stdout!r} stderr={result2.stderr!r}"
    assert "meta" in (result2.stdout + result2.stderr)

    bad_path = tmp_path / "bad.json"
    bad_doc = json.loads(json.dumps(doc))
    bad_doc["scenarios"]["s1_cancel"]["radix"]["share"] = 2.0
    bad_path.write_text(json.dumps(bad_doc))

    result3 = _run_cli("validate", [str(bad_path)], timeout=30)
    assert result3.returncode == 1, f"stdout={result3.stdout!r} stderr={result3.stderr!r}"


# --- run_s3_end_to_end (Plan 02-08, Task 2) -------------------------------------------


@pytest.mark.slow
def test_run_s3_end_to_end(tmp_path):
    from rsglang.testing.fake_profile_env import FAKE_ACTIVE_SAMPLES, FAKE_RADIX_SAMPLES

    bin_dir = _write_pyspy_stub(tmp_path)
    _write_hyperfine_stub(bin_dir)
    role_map_path = tmp_path / "roles.json"
    port = _free_port()
    out_path = tmp_path / "p3.json"
    work_dir = tmp_path / "w3"

    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["RSGLANG_FAKE_PROFILE_ROLE_MAP"] = str(role_map_path)

    result = _run_cli(
        "run",
        [
            "--scenarios", "s3",
            "--s3-runs", "2",
            "--s3-warmup", "1",
            "--s3-sample-s", "1",
            "--s3-max-tokens", "4",
            "--sample-interval-s", "0.2",
            "--server-cmd",
            "{python} -m rsglang.testing.fake_profile_env server --port {port}",
            "--port", str(port),
            "--timeout", "60",
            "--out", str(out_path),
            "--work-dir", str(work_dir),
        ],
        env=env,
        timeout=180,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_sidecar(doc, require_scenarios=("s3_coldstart",)) == []

    entry = doc["scenarios"]["s3_coldstart"]
    coldstart = entry["coldstart"]
    assert coldstart["hyperfine"]["runs"] == 2
    assert len(coldstart["ready_s_self_timed"]) == 2
    assert all(v > 0 for v in coldstart["ready_s_self_timed"])
    assert len(coldstart["rss_tree_bytes_at_ready"]) == 2
    assert all(isinstance(v, int) and v > 0 for v in coldstart["rss_tree_bytes_at_ready"])
    if not sys.platform.startswith("linux"):
        assert coldstart["pss_tree_bytes_at_ready"] == [None, None]

    assert entry["requests"]["sent"] == 1
    assert entry["requests"]["completed"] == 1

    assert entry["radix"]["share"] == FAKE_RADIX_SAMPLES / FAKE_ACTIVE_SAMPLES

    scheduler_events = entry["gc"]["scheduler"]["events"]
    assert scheduler_events, "expected at least one boot-window GC event"
    first_t_rel_s = min(e[0] for e in scheduler_events)
    assert first_t_rel_s < entry["params"]["instrumented_ready_s"]

    work_dir_path = Path(work_dir)
    pgid_files = list(work_dir_path.glob("**/pgid")) + list(work_dir_path.glob("**/*.pgid"))
    assert not pgid_files, f"leftover pgid file(s): {pgid_files}"

    role_map = json.loads(role_map_path.read_text())
    alive_pids = {int(pid) for pid in role_map}
    still_alive = _wait_until_dead(alive_pids)
    assert not still_alive, f"still alive: {still_alive}"


# --- run_s3_hyperfine_missing_exits_2 (Plan 02-08, Task 2) ----------------------------


@pytest.mark.slow
def test_run_s3_hyperfine_missing_exits_2(tmp_path):
    bin_dir = _write_pyspy_stub(tmp_path)
    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"

    result = _run_cli(
        "run",
        ["--scenarios", "s3", "--out", str(tmp_path / "x.json"), "--work-dir", str(tmp_path / "w")],
        env=env,
        timeout=30,
    )
    assert result.returncode == 2, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "hyperfine" in result.stderr
