"""Mac-runnable tests for scripts/gpu_phase2_profile.sh's wrapper CLI and
preflight helpers (BENCH-01).

Each test either runs the script directly (for --help/argument-parsing
behavior) or sources it under bash with stub `py-spy`/`hyperfine` executables
placed first on PATH, so the real GPU-only preflight and profiling steps
never run (the source guard returns before the "# --- Preflight" section).
The py-spy stub execs rsglang.testing.fake_profile_env's py-spy stand-in, the
same stand-in python/tests/test_baseline_profile.py already relies on.
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.slow  # every test spawns bash

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "gpu_phase2_profile.sh"


def _path_without_real_hyperfine(bin_dir: Path) -> str:
    """`bin_dir` first, then every other `PATH` entry that does not itself
    contain a real `hyperfine` binary.

    Simulating "hyperfine is missing" by deleting it from `bin_dir` only
    works if nothing else on `PATH` has a real one. On a machine (like
    this Mac) that has `hyperfine` installed globally per CLAUDE.md's own
    recommendation (e.g. `~/.cargo/bin/hyperfine`), the default
    `{bin_dir}{PATH}` composition still finds that real binary further
    down `PATH`, so the "missing" case never actually fires. This is the
    PATH-stubbing bug, not the preflight helper under test."""
    rest = [
        p
        for p in os.environ.get("PATH", "").split(os.pathsep)
        if p and not (Path(p) / "hyperfine").is_file()
    ]
    return os.pathsep.join([str(bin_dir), *rest])


def _rc(result: subprocess.CompletedProcess[str]) -> int:
    """Extract the `rc=<N>` line a snippet echoes after calling a helper."""
    match = re.search(r"^rc=(-?\d+)$", result.stdout, re.MULTILINE)
    assert match, f"no rc= line in stdout: {result.stdout!r} (stderr: {result.stderr!r})"
    return int(match.group(1))


def _write_stub(tmp_path: Path, name: str, body: str) -> Path:
    """Write an executable `#!/bin/bash` stub named `name` into tmp_path/bin."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / name
    stub.write_text(f"#!/bin/bash\n{body}\n")
    stub.chmod(0o755)
    return stub


def _write_pyspy_stub(tmp_path: Path) -> Path:
    """Write a stub `py-spy` that execs rsglang.testing.fake_profile_env's
    py-spy stand-in, per this plan's interfaces contract."""
    return _write_stub(
        tmp_path,
        "py-spy",
        f'exec "{sys.executable}" -m rsglang.testing.fake_profile_env py-spy "$@"',
    )


def _base_env(tmp_path: Path) -> dict:
    env = dict(os.environ)
    env["TMPDIR"] = str(tmp_path)
    env["PYTHON"] = sys.executable
    env["PYTHONPATH"] = f"{ROOT / 'python'}{os.pathsep}{env.get('PYTHONPATH', '')}"
    return env


def _run(tmp_path: Path, *args: str, **env_vars: object) -> subprocess.CompletedProcess[str]:
    """Run SCRIPT directly (not sourced) with the given CLI args."""
    env = _base_env(tmp_path)
    env.update({k: str(v) for k, v in env_vars.items()})
    return subprocess.run(
        ["bash", str(SCRIPT), *args],
        capture_output=True,
        text=True,
        timeout=60,
        env=env,
    )


def _bash(snippet: str, tmp_path: Path, **env_vars: object) -> subprocess.CompletedProcess[str]:
    """Source SCRIPT, then run `snippet`, with tmp_path/bin first on PATH."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    env = _base_env(tmp_path)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env.update({k: str(v) for k, v in env_vars.items()})
    script = f'source "{SCRIPT}"\n{snippet}'
    return subprocess.run(
        ["bash", "-c", script],
        capture_output=True,
        text=True,
        timeout=60,
        start_new_session=True,
        env=env,
    )


def _sleeper_pids() -> list[str]:
    """PIDs of any leftover `python -c 'import time; time.sleep(30)'` probe
    sleepers pyspy_can_attach starts -- used to assert clean teardown."""
    result = subprocess.run(
        ["pgrep", "-f", "import time; time.sleep(30)"],
        capture_output=True,
        text=True,
    )
    return [line for line in result.stdout.splitlines() if line.strip()]


# --- CLI shape (help / unknown arg) ---------------------------------------------


def test_help_anywhere(tmp_path):
    result = _run(tmp_path, "--help")
    assert result.returncode == 0, result.stderr
    for token in ("Usage:", "--model", "--port", "--timeout", "--out", "--py-spy-sudo", "setcap"):
        assert token in result.stdout, result.stdout


def test_unknown_arg_exits_2(tmp_path):
    result = _run(tmp_path, "--bogus")
    assert result.returncode == 2, result.stdout + result.stderr


# --- version_ge ------------------------------------------------------------------


def test_version_ge(tmp_path):
    cases = [
        ("1.19.0", "1.19.0", 0),
        ("1.20.0", "1.19.0", 0),
        ("1.100.0", "1.19.0", 0),
        ("1.18.9", "1.19.0", 1),
        ("2.0", "1.19.0", 0),
    ]
    for a, b, expected_rc in cases:
        result = _bash(f'rc=0; version_ge "{a}" "{b}" || rc=$?; echo "rc=$rc"', tmp_path)
        assert _rc(result) == expected_rc, (a, b, result.stdout, result.stderr)


# --- hyperfine_ok ------------------------------------------------------------------


def test_hyperfine_ok(tmp_path):
    _write_stub(tmp_path, "hyperfine", 'echo "hyperfine 1.20.0"')
    result = _bash('rc=0; hyperfine_ok || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 0, result.stdout + result.stderr

    _write_stub(tmp_path, "hyperfine", 'echo "hyperfine 1.18.1"')
    result = _bash('rc=0; out="$(hyperfine_ok 2>&1)" || rc=$?; echo "$out"; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 1, result.stdout
    assert "1.19.0" in result.stdout, result.stdout

    (tmp_path / "bin" / "hyperfine").unlink()
    result = _bash(
        'rc=0; hyperfine_ok || rc=$?; echo "rc=$rc"',
        tmp_path,
        PATH=_path_without_real_hyperfine(tmp_path / "bin"),
    )
    assert _rc(result) == 1, result.stdout + result.stderr


# --- pyspy_can_attach --------------------------------------------------------------


def test_pyspy_can_attach_ok_and_denied(tmp_path):
    _write_pyspy_stub(tmp_path)

    result = _bash('rc=0; pyspy_can_attach || rc=$?; echo "rc=$rc"', tmp_path)
    assert _rc(result) == 0, result.stdout + result.stderr
    assert not _sleeper_pids(), "probe sleeper process leaked after the ok case"

    result = _bash(
        'rc=0; out="$(pyspy_can_attach 2>&1)" || rc=$?; echo "$out"; echo "rc=$rc"',
        tmp_path,
        RSGLANG_FAKE_PYSPY_MODE="denied",
    )
    assert _rc(result) == 1, result.stdout
    for token in ("setcap", "--py-spy-sudo", "ptrace_scope"):
        assert token in result.stdout, result.stdout
    assert not _sleeper_pids(), "probe sleeper process leaked after the denied case"


# --- preflight / driver references (Task 2) ---------------------------------------


def test_preflight_missing_tool_fails_before_launch(tmp_path):
    _write_stub(tmp_path, "curl", "exit 0")
    _write_pyspy_stub(tmp_path)
    _write_stub(tmp_path, "hyperfine", 'echo "hyperfine 1.20.0"')
    bin_dir = tmp_path / "bin"
    import shutil

    bash_dir = str(Path(shutil.which("bash") or "/bin/bash").parent)
    env = _base_env(tmp_path)
    env["PATH"] = os.pathsep.join([str(bin_dir), bash_dir, "/bin", "/usr/bin"])
    result = subprocess.run(
        ["bash", str(SCRIPT)],
        capture_output=True,
        text=True,
        timeout=60,
        env=env,
    )
    assert result.returncode == 1, result.stdout + result.stderr
    assert "FAIL preflight: not on PATH: nvidia-smi" in result.stdout, result.stdout
    assert "step 1" not in result.stdout, result.stdout


def test_steps_reference_driver(tmp_path):
    text = SCRIPT.read_text()
    for token in (
        "baseline_profile.py discover",
        "baseline_profile.py run",
        "baseline_profile.py validate",
        "--require-gpu",
        "scripts/check_upstream.py",
    ):
        assert token in text, token
