"""Mac pre-flight and behavior tests for rsglang.profiling.hook (BENCH-01).

Task 1's tracer (test_hook_propagates_to_spawn_child) is the RESEARCH Pitfall 3 /
Open Question 2 pre-flight: does an env-gated sitecustomize shim reach a
multiprocessing spawn child, and does it survive rsglang.launch's os.execv?
Nothing else in the phase depends on the hook until this test is green.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

from rsglang.profiling.hook import hook_env, load_hook_records, write_shim

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
