"""Mac pre-flight and behavior tests for rsglang.profiling.hook (BENCH-01).

Task 1's tracer (test_hook_propagates_to_spawn_child) is the RESEARCH Pitfall 3 /
Open Question 2 pre-flight: does an env-gated sitecustomize shim reach a
multiprocessing spawn child, and does it survive rsglang.launch's os.execv?
Nothing else in the phase depends on the hook until this test is green.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

from rsglang.profiling.hook import (
    DEFAULT_INTERVAL_S,
    PROFILE_DIR_ENV,
    _interval_from_env,
    hook_env,
    load_hook_records,
    request_snapshot,
    write_shim,
)

# --- spawn_child ------------------------------------------------------------------

_CHILD_MODULE_SOURCE = '''"""Spawn-child target for test_hook_propagates_to_spawn_child."""
import gc
import time


def work() -> None:
    gc.collect()
    time.sleep(0.3)
'''

_PARENT_SCRIPT = """
import multiprocessing as mp
import os

import preflight_child

if __name__ == "__main__":
    mp.set_start_method("spawn", force=True)
    p = mp.Process(target=preflight_child.work)
    p.start()
    p.join()
    print(f"PARENT_PID={os.getpid()}")
    print(f"CHILD_PID={p.pid}")
"""


def _line_value(stdout: str, prefix: str) -> str:
    for line in stdout.splitlines():
        if line.startswith(prefix):
            return line[len(prefix):]
    raise AssertionError(f"no {prefix!r} line in stdout: {stdout!r}")


@pytest.mark.slow
def test_hook_propagates_to_spawn_child(tmp_path: Path) -> None:
    mods_dir = tmp_path / "mods"
    mods_dir.mkdir()
    (mods_dir / "preflight_child.py").write_text(_CHILD_MODULE_SOURCE, encoding="utf-8")

    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"

    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)
    env["PYTHONPATH"] = os.pathsep.join([str(mods_dir), env["PYTHONPATH"]])

    result = subprocess.run(
        [sys.executable, "-c", _PARENT_SCRIPT],
        env=env,
        timeout=60,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr

    parent_pid = int(_line_value(result.stdout, "PARENT_PID="))
    child_pid = int(_line_value(result.stdout, "CHILD_PID="))

    records = load_hook_records(profile_dir)

    assert parent_pid in records.by_pid, (records.by_pid.keys(), result.stderr)
    assert any(r["kind"] == "start" for r in records.by_pid[parent_pid])

    assert child_pid in records.by_pid, (records.by_pid.keys(), result.stderr)
    child_records = records.by_pid[child_pid]
    child_start = next(r for r in child_records if r["kind"] == "start")
    assert child_start["ppid"] == parent_pid

    gc_records = [r for r in child_records if r["kind"] == "gc"]
    assert gc_records, child_records
    assert any(r["generation"] == 2 for r in gc_records)
    assert all(r["duration_s"] >= 0 for r in gc_records)


# --- exec_hop ---------------------------------------------------------------------


@pytest.mark.slow
def test_hook_survives_exec(tmp_path: Path) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"
    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)

    # A script file (not -c), so only the post-exec image's orig_argv contains "-c".
    parent_file = tmp_path / "exec_parent.py"
    parent_file.write_text(
        "import os, sys\n"
        "os.execv(sys.executable, [sys.executable, '-c', "
        "'import gc, time; gc.collect(); time.sleep(0.3)'])\n",
        encoding="utf-8",
    )

    result = subprocess.run(
        [sys.executable, str(parent_file)], env=env, timeout=30, capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr

    records = load_hook_records(profile_dir)
    assert len(records.by_pid) == 1, records.by_pid.keys()
    (pid,) = records.by_pid.keys()
    start_records = [r for r in records.by_pid[pid] if r["kind"] == "start"]
    assert len(start_records) == 2, start_records
    assert any("-c" in r["orig_argv"] for r in start_records)
    assert any("-c" not in r["orig_argv"] for r in start_records)

    hook_files = sorted(profile_dir.glob(f"hook-{pid}-*.jsonl"))
    assert len(hook_files) == 2, hook_files


# --- env_gate_off ------------------------------------------------------------------


@pytest.mark.slow
def test_env_gate_off_is_inert(tmp_path: Path) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"

    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)
    env.pop(PROFILE_DIR_ENV, None)  # the env gate: off

    child_script = (
        "import gc, threading, tracemalloc\n"
        "from rsglang.profiling import hook\n"
        "has_cb = any(getattr(cb, '__module__', '') == hook.__name__ for cb in gc.callbacks)\n"
        "print('CB=' + str(has_cb))\n"
        "print('TRACING=' + str(tracemalloc.is_tracing()))\n"
        "names = [t.name for t in threading.enumerate()]\n"
        "print('THREAD=' + str(hook.THREAD_NAME in names))\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", child_script], env=env, timeout=30, capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    assert "CB=False" in result.stdout, result.stdout
    assert "TRACING=False" in result.stdout, result.stdout
    assert "THREAD=False" in result.stdout, result.stdout
    assert list(tmp_path.rglob("hook-*.jsonl")) == []


# --- chain_load --------------------------------------------------------------------


@pytest.mark.slow
@pytest.mark.parametrize("enabled", [True, False])
def test_shadowed_sitecustomize_still_runs(tmp_path: Path, enabled: bool) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    other_dir = tmp_path / "other"
    other_dir.mkdir()
    marker = tmp_path / "marker.txt"
    (other_dir / "sitecustomize.py").write_text(
        f"from pathlib import Path\nPath({str(marker)!r}).write_text('loaded')\n",
        encoding="utf-8",
    )

    profile_dir = tmp_path / "prof"
    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)
    env["PYTHONPATH"] = os.pathsep.join([str(shim_dir), str(other_dir), env["PYTHONPATH"]])
    if not enabled:
        env.pop(PROFILE_DIR_ENV, None)

    result = subprocess.run(
        [sys.executable, "-c", "print('ok')"], env=env, timeout=30, capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    assert marker.exists(), result.stderr

    if enabled:
        records = load_hook_records(profile_dir)
        assert any(
            any(r["kind"] == "start" for r in recs) for recs in records.by_pid.values()
        ), records.by_pid


# --- mem_and_alloc_top ---------------------------------------------------------------


@pytest.mark.slow
def test_mem_and_alloc_top(tmp_path: Path) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"
    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)

    child_script = (
        "import time\n"
        "data = []\n"
        "deadline = time.monotonic() + 5.0\n"
        "while time.monotonic() < deadline:\n"
        "    data.append(bytearray(4096))\n"
        "    if len(data) > 4000:\n"
        "        data = data[-1000:]\n"
        "    time.sleep(0.05)\n"
    )
    proc = subprocess.Popen(
        [sys.executable, "-c", child_script],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        child_pid = proc.pid
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            records = load_hook_records(profile_dir)
            if child_pid in records.by_pid and any(
                r["kind"] == "start" for r in records.by_pid[child_pid]
            ):
                break
            time.sleep(0.1)
        else:
            raise AssertionError("child never installed the hook")

        request_snapshot(profile_dir, "s1_cancel")

        alloc_record = None
        child_records: list = []
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            records = load_hook_records(profile_dir)
            child_records = records.by_pid.get(child_pid, [])
            alloc_records = [
                r for r in child_records if r["kind"] == "alloc_top" and r["tag"] == "s1_cancel"
            ]
            mem_records = [r for r in child_records if r["kind"] == "mem"]
            if alloc_records and len(mem_records) >= 2:
                alloc_record = alloc_records[0]
                break
            time.sleep(0.1)
        assert alloc_record is not None, "no alloc_top record (or <2 mem records) observed"

        sites = alloc_record["sites"]
        assert 1 <= len(sites) <= 10, sites
        for site in sites:
            assert isinstance(site["file"], str)
            assert isinstance(site["line"], int)
            assert isinstance(site["size_bytes"], int) and site["size_bytes"] >= 0
            assert isinstance(site["count"], int) and site["count"] >= 0

        mem_records = [r for r in child_records if r["kind"] == "mem"]
        assert len(mem_records) >= 2, child_records
        for r in mem_records:
            assert isinstance(r["traced_current"], int)
            assert isinstance(r["traced_peak"], int)
    finally:
        proc.wait(timeout=10)


# --- request_snapshot_validation -------------------------------------------------------


def test_request_snapshot_rejects_bad_tag(tmp_path: Path) -> None:
    for tag in ("../x", ""):
        with pytest.raises(ValueError):
            request_snapshot(tmp_path, tag)
    assert list(tmp_path.glob("snapshot-*.request")) == []


# --- malformed_lines ---------------------------------------------------------------


def test_malformed_lines_counted(tmp_path: Path) -> None:
    hook_file = tmp_path / "hook-123-456.jsonl"
    good1 = {
        "kind": "start",
        "pid": 123,
        "ppid": 1,
        "orig_argv": [],
        "t": 0.0,
        "wall": 0.0,
        "clock": "monotonic",
    }
    good2 = {
        "kind": "gc",
        "pid": 123,
        "t": 0.1,
        "duration_s": 0.0,
        "generation": 2,
        "collected": 0,
        "uncollectable": 0,
    }
    truncated = '{"kind": "gc", "pid": 123, "t": 0.2, "dur'
    hook_file.write_text(json.dumps(good1) + "\n" + json.dumps(good2) + "\n" + truncated, encoding="utf-8")

    records = load_hook_records(tmp_path)
    assert records.malformed_lines == 1
    assert len(records.by_pid[123]) == 2


# --- interval_env_parsing ---------------------------------------------------------


@pytest.mark.parametrize(
    "value,expected",
    [
        (None, DEFAULT_INTERVAL_S),
        ("0", DEFAULT_INTERVAL_S),
        ("-1", DEFAULT_INTERVAL_S),
        ("abc", DEFAULT_INTERVAL_S),
        ("0.25", 0.25),
    ],
)
def test_interval_env_parsing(value, expected):
    assert _interval_from_env(value) == expected
