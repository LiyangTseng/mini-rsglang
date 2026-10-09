"""Tests for rsglang.profiling.hook's GC-only mode (BENCH-08, D-14/D-15).

D-15 requires gc.callbacks active but tracemalloc, mem sampling and snapshot
serving off during timed benchmark runs. These tests exercise the
RSGLANG_PROFILE_MODE=gc_only path end to end, plus the process-name records
that let the harness attribute GC pauses to a role without py-spy, and
confirm Phase 2's full-mode behavior (python/tests/test_profile_hook.py)
stays byte-for-byte unchanged.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

from rsglang.profiling.hook import (
    MODE_FULL,
    MODE_GC_ONLY,
    _mode_from_env,
    hook_env,
    load_hook_records,
    write_shim,
)

# --- gc_only_tracer -----------------------------------------------------------------

_GC_ONLY_CHILD_SCRIPT = """
import gc
import time
import tracemalloc

from rsglang.profiling.hook import request_snapshot

gc.collect()
print("TRACING=" + str(tracemalloc.is_tracing()))
time.sleep(0.5)
request_snapshot({profile_dir!r}, "t1")
time.sleep(0.5)
"""


@pytest.mark.slow
def test_gc_only_tracer(tmp_path: Path) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"

    env = hook_env(
        os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1, mode="gc_only"
    )

    child_script = _GC_ONLY_CHILD_SCRIPT.format(profile_dir=str(profile_dir))
    result = subprocess.run(
        [sys.executable, "-c", child_script],
        env=env,
        timeout=30,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    assert "TRACING=False" in result.stdout, result.stdout

    records = load_hook_records(profile_dir)
    assert len(records.by_pid) == 1, records.by_pid.keys()
    (pid,) = records.by_pid.keys()
    pid_records = records.by_pid[pid]

    assert any(r["kind"] == "start" for r in pid_records)
    gc_records = [r for r in pid_records if r["kind"] == "gc"]
    assert gc_records, pid_records
    assert any(r["generation"] == 2 for r in gc_records)

    assert [r for r in pid_records if r["kind"] == "mem"] == []
    assert [r for r in pid_records if r["kind"] == "alloc_top"] == []


# --- proc_records_name_spawn_child ---------------------------------------------------

_PROC_CHILD_MODULE_SOURCE = '''"""Spawn-child target for test_proc_records_name_spawn_child."""
import time


def work() -> None:
    time.sleep(1.5)
'''

_PROC_PARENT_SCRIPT = """
import multiprocessing as mp
import os
import time

import proc_child

if __name__ == "__main__":
    mp.set_start_method("spawn", force=True)
    p = mp.Process(target=proc_child.work, name="rsgbench-TP0-scheduler")
    p.start()
    print(f"PARENT_PID={os.getpid()}")
    print(f"CHILD_PID={p.pid}")
    p.join()
    time.sleep(0.3)
"""


def _line_value(stdout: str, prefix: str) -> str:
    for line in stdout.splitlines():
        if line.startswith(prefix):
            return line[len(prefix):]
    raise AssertionError(f"no {prefix!r} line in stdout: {stdout!r}")


@pytest.mark.slow
def test_proc_records_name_spawn_child(tmp_path: Path) -> None:
    mods_dir = tmp_path / "mods"
    mods_dir.mkdir()
    (mods_dir / "proc_child.py").write_text(_PROC_CHILD_MODULE_SOURCE, encoding="utf-8")

    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"

    env = hook_env(
        os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1, mode="gc_only"
    )
    env["PYTHONPATH"] = os.pathsep.join([str(mods_dir), env["PYTHONPATH"]])

    result = subprocess.run(
        [sys.executable, "-c", _PROC_PARENT_SCRIPT],
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
    parent_proc_records = [r for r in records.by_pid[parent_pid] if r["kind"] == "proc"]
    assert parent_proc_records, records.by_pid[parent_pid]
    assert parent_proc_records[-1]["name"] == "MainProcess", parent_proc_records

    assert child_pid in records.by_pid, (records.by_pid.keys(), result.stderr)
    child_proc_records = [r for r in records.by_pid[child_pid] if r["kind"] == "proc"]
    assert child_proc_records, records.by_pid[child_pid]
    assert child_proc_records[-1]["name"] == "rsgbench-TP0-scheduler", child_proc_records

    for record in parent_proc_records + child_proc_records:
        assert isinstance(record["pid"], int)
        assert isinstance(record["name"], str)
        assert isinstance(record["t"], float)
        assert isinstance(record["wall"], float)


# --- full_mode_writes_no_proc_records -------------------------------------------------


@pytest.mark.slow
def test_full_mode_writes_no_proc_records(tmp_path: Path) -> None:
    shim_dir = tmp_path / "shim"
    write_shim(shim_dir)
    profile_dir = tmp_path / "prof"

    # mode=None: full mode, Phase 2's exact default (RSGLANG_PROFILE_MODE unset).
    env = hook_env(os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=0.1)

    child_script = "import gc, time\ngc.collect()\ntime.sleep(0.5)\n"
    result = subprocess.run(
        [sys.executable, "-c", child_script],
        env=env,
        timeout=30,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr

    records = load_hook_records(profile_dir)
    assert len(records.by_pid) == 1, records.by_pid.keys()
    (pid,) = records.by_pid.keys()
    proc_records = [r for r in records.by_pid[pid] if r["kind"] == "proc"]
    assert proc_records == [], proc_records


# --- mode_from_env ----------------------------------------------------------------


@pytest.mark.parametrize(
    "value,expected",
    [
        (None, MODE_FULL),
        ("", MODE_FULL),
        ("full", MODE_FULL),
        ("gc_only", MODE_GC_ONLY),
        ("GC_ONLY", MODE_FULL),
        ("bogus", MODE_FULL),
    ],
)
def test_mode_from_env(value, expected):
    assert _mode_from_env(value) == expected
