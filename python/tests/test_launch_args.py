"""Launcher flag parsing and the --frontend python passthrough (fast, no processes spawned)."""

from __future__ import annotations

import sys

import pytest

from rsglang import launch


class ExecRecorder:
    def __init__(self):
        self.calls = []

    def __call__(self, path, argv):
        self.calls.append((path, list(argv)))


def _python_mode(argv):
    rec = ExecRecorder()
    launch.main(["--frontend", "python", *argv], execv=rec)
    assert len(rec.calls) == 1
    return rec.calls[0]


def test_python_mode_execs_upstream_launcher():
    path, argv = _python_mode(["--model", "Qwen/Qwen3-0.6B", "--port", "1920"])
    assert path == sys.executable
    assert argv == [sys.executable, "-m", "minisgl", "--model", "Qwen/Qwen3-0.6B", "--port", "1920"]


def test_launcher_only_flags_are_not_forwarded():
    _, argv = _python_mode([
        "--rust-bin", "/nonexistent/rsg-server",
        "--model", "M",
        "--ready-timeout", "5",
        "--rust-log", "debug",
        "--port", "1920",
    ])
    assert argv == [sys.executable, "-m", "minisgl", "--model", "M", "--port", "1920"]


def test_upstream_flags_keep_order_and_values():
    upstream = ["--num-tokenizer", "2", "--model", "M", "--shell-mode", "--dtype", "bfloat16"]
    _, argv = _python_mode(upstream)
    assert argv[3:] == upstream


def test_frontend_is_required():
    with pytest.raises(SystemExit) as exc:
        launch.main(["--model", "M"], execv=ExecRecorder())
    assert exc.value.code == 2


def test_frontend_flag_cannot_be_abbreviated():
    with pytest.raises(SystemExit) as exc:
        launch.main(["--front", "python", "--model", "M"], execv=ExecRecorder())
    assert exc.value.code == 2


def test_python_mode_does_not_parse_upstream_args(monkeypatch):
    import minisgl.server.args

    def boom(*args, **kwargs):
        raise AssertionError("python mode must not call upstream parse_args in the launcher")

    monkeypatch.setattr(minisgl.server.args, "parse_args", boom)
    _, argv = _python_mode(["--model", "M"])
    assert argv[3:] == ["--model", "M"]


def test_rust_mode_rejects_shell_mode_without_spawning(monkeypatch):
    import subprocess
    import multiprocessing

    def no_spawn(*args, **kwargs):
        raise AssertionError("nothing may be spawned")

    monkeypatch.setattr(subprocess, "Popen", no_spawn)
    monkeypatch.setattr(multiprocessing, "Process", no_spawn)
    rc = launch.main(["--frontend", "rust", "--shell-mode", "--model", "M", "--dtype", "bfloat16"])
    assert rc == 2
