"""Mac-runnable tests for scripts/gpu_phase7_bench.sh's wrapper CLI, dry-run
command sequence, and preflight failure path (BENCH-02..BENCH-08).

Every test runs the script directly (it is never sourced: there is no
helper-function-level test here, unlike test_gpu_profile_script.py). The
preflight-failure test places a PATH with no `nvidia-smi` first, so the
real GPU-only steps never run.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.slow  # every test spawns bash

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "gpu_phase7_bench.sh"


def _base_env(tmp_path: Path) -> dict:
    env = dict(os.environ)
    env["TMPDIR"] = str(tmp_path)
    env["PYTHON"] = sys.executable
    env["PYTHONPATH"] = f"{ROOT / 'python'}{os.pathsep}{env.get('PYTHONPATH', '')}"
    return env


def _run(tmp_path: Path, *args: str, **env_vars: object) -> subprocess.CompletedProcess[str]:
    env = _base_env(tmp_path)
    env.update({k: str(v) for k, v in env_vars.items()})
    return subprocess.run(
        ["bash", str(SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=60,
        env=env,
        cwd=ROOT,
    )


# --- CLI shape (help / unknown arg) ---------------------------------------------


def test_help(tmp_path):
    result = _run(tmp_path, "--help")
    assert result.returncode == 0, result.stderr
    for token in (
        "--model",
        "--port",
        "--runs",
        "--candidates",
        "--out-dir",
        "--dry-run",
        "VLLM_BIN",
        "SGLANG_PYTHON",
        "BACKEND_EXTRA",
        "RUST_CMD_EXTRA",
    ):
        assert token in result.stdout, result.stdout


def test_unknown_arg(tmp_path):
    result = _run(tmp_path, "--bogus")
    assert result.returncode == 2, result.stdout + result.stderr


# --- --dry-run: the exact command sequence --------------------------------------


def test_dry_run_sequence(tmp_path):
    import time

    before = time.monotonic()
    result = _run(tmp_path, "--dry-run", "--runs", "3")
    elapsed = time.monotonic() - before
    assert result.returncode == 0, result.stdout + result.stderr
    assert elapsed < 5.0, f"dry run took {elapsed:.2f}s"

    run_lines = [line for line in result.stdout.splitlines() if line.startswith("RUN: ")]

    def first_index(needle: str) -> int:
        for i, line in enumerate(run_lines):
            if needle in line:
                return i
        raise AssertionError(f"no RUN: line contains {needle!r}; lines:\n" + "\n".join(run_lines))

    order = [
        "cargo build --release",
        "sweep-num-tokenizer",
        " s1 ",
        " s2 ",
        " s3 ",
        " throughput ",
        " report ",
        "check_upstream.py",
    ]
    indices = [first_index(n) for n in order]
    assert indices == sorted(indices), (order, run_lines)

    sweep_line = run_lines[indices[1]]
    assert "--candidates 0,1,2,4" in sweep_line, sweep_line
    assert "--runs 1" in sweep_line, sweep_line
    assert "--backend-kind real" not in sweep_line, sweep_line
    assert "--python-best-num-tokenizer" not in sweep_line, sweep_line

    for name, idx in zip(order[2:6], indices[2:6]):
        line = run_lines[idx]
        assert "--backend-kind real" in line, (name, line)
        assert "--runs 3" in line, (name, line)
        assert "--python-best-num-tokenizer" in line and "<best-from-sweep>" in line, (name, line)

    report_line = run_lines[indices[6]]
    assert "docs/benchmarks/frontend-benchmarks.json" in report_line, report_line
    assert "docs/benchmarks/frontend-benchmarks.md" in report_line, report_line

    assert subprocess.run(
        ["git", "status", "--porcelain", "docs/benchmarks"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    ).stdout == ""


def test_dry_run_crosschecks_optional(tmp_path):
    result = _run(tmp_path, "--dry-run")
    assert result.returncode == 0, result.stdout + result.stderr
    run_lines = [line for line in result.stdout.splitlines() if line.startswith("RUN: ")]
    assert not any("crosscheck" in line for line in run_lines), run_lines

    result = _run(tmp_path, "--dry-run", VLLM_BIN="/fake/vllm", SGLANG_PYTHON="/fake/python")
    assert result.returncode == 0, result.stdout + result.stderr
    run_lines = [line for line in result.stdout.splitlines() if line.startswith("RUN: ")]

    def first_index(needle: str) -> int:
        for i, line in enumerate(run_lines):
            if needle in line:
                return i
        raise AssertionError(f"no RUN: line contains {needle!r}")

    s2_idx = first_index(" s2 ")
    s3_idx = first_index(" s3 ")
    vllm_idx = first_index("crosscheck --tool vllm")
    sglang_idx = first_index("crosscheck --tool sglang")
    assert s2_idx < vllm_idx < s3_idx, run_lines
    assert s2_idx < sglang_idx < s3_idx, run_lines
    assert "--vllm-bin \"/fake/vllm\"" in run_lines[vllm_idx], run_lines[vllm_idx]
    assert "--sglang-python \"/fake/python\"" in run_lines[sglang_idx], run_lines[sglang_idx]


def test_extra_args_placement(tmp_path):
    result = _run(
        tmp_path,
        "--dry-run",
        BACKEND_EXTRA="--max-running-req 64",
        RUST_CMD_EXTRA="--abort-timing immediate",
    )
    assert result.returncode == 0, result.stdout + result.stderr
    run_lines = [line for line in result.stdout.splitlines() if line.startswith("RUN: ")]
    cmd_value_lines = [
        line for line in run_lines if "--python-cmd" in line or "--rust-cmd" in line
    ]
    assert cmd_value_lines, run_lines

    for line in cmd_value_lines:
        # Every --python-cmd and --rust-cmd value on this line carries the
        # backend extra (it has no --python-cmd/--rust-cmd-only split).
        assert "--max-running-req 64" in line, line

    rust_cmd_lines = [line for line in run_lines if "--rust-cmd" in line]
    python_only_segments = [
        line.split("--rust-cmd", 1)[0] for line in rust_cmd_lines if "--python-cmd" in line
    ]
    for segment in python_only_segments:
        assert "--abort-timing immediate" not in segment, segment
    for line in rust_cmd_lines:
        rust_segment = line.split("--rust-cmd", 1)[1]
        assert "--abort-timing immediate" in rust_segment, line


# --- preflight ---------------------------------------------------------------


def test_preflight_fails_without_gpu(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    env = _base_env(tmp_path)
    # No nvidia-smi stub: a PATH made only of this empty dir plus the
    # minimum needed to run bash/cargo/git themselves.
    import shutil

    keep = []
    for tool in ("bash", "cargo", "git", "grep", "awk", "mktemp", "cut", "tail", "mkdir", "cd"):
        found = shutil.which(tool)
        if found:
            keep.append(str(Path(found).parent))
    env["PATH"] = os.pathsep.join([str(bin_dir), *dict.fromkeys(keep)])

    result = subprocess.run(
        ["bash", str(SCRIPT)],
        capture_output=True,
        text=True,
        timeout=60,
        env=env,
        cwd=ROOT,
    )
    assert result.returncode != 0, result.stdout + result.stderr
    assert "FAIL step preflight" in result.stdout, result.stdout
