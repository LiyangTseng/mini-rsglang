"""Mac-side tests for the `stress` run part (ROADMAP criterion 4, D-11/D-12).

Task 1's tracer (test_tracer_stress_part_both_timings) proves the whole
session/watcher/stress-cmd/tap-evidence path end to end against the fake
rust-flavor parity server and the fake 128-agent stress driver; nothing else
in this plan's abort-stress surface depends on this path until it is green.
test_analyze_classification proves abort_analysis.analyze's classification
and evidence fields directly against synthetic tap records, independent of
any subprocess.
"""

from __future__ import annotations

import json
import os
import socket
import stat
import subprocess
import sys
from pathlib import Path

import pytest

from rsglang.parity import abort_analysis, sidecar, tap

REPO_ROOT = Path(__file__).resolve().parents[2]


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _write_stub_nvidia_smi(bin_dir: Path) -> Path:
    """A stub `nvidia-smi` that lists the fake scheduler child's pid via
    `pgrep -f`, so the watcher's --query-compute-apps check has something
    to find without a real GPU (06-02's watcher treats nvsmi failures as
    non-fatal anyway, but this keeps the sample lines meaningful)."""
    bin_dir.mkdir(parents=True, exist_ok=True)
    script = bin_dir / "nvidia-smi"
    script.write_text("#!/usr/bin/env bash\npgrep -f fake-parity-scheduler || true\nexit 0\n")
    script.chmod(script.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    return script


def _run_parity_check(
    args: list, *, extra_path: "Path | None" = None, timeout: int = 120
) -> "subprocess.CompletedProcess[str]":
    env = dict(os.environ)
    if extra_path is not None:
        env["PATH"] = f"{extra_path}{os.pathsep}{env.get('PATH', '')}"
    return subprocess.run(
        [sys.executable, "scripts/parity_check.py", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )


# --- test_analyze_classification (fast, synthetic records) -----------------------------


def test_analyze_classification():
    records = [
        {"kind": tap.KIND_USER, "pid": 1, "seq": 0, "uid": 1, "input_ids": [1], "sampling": {}},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 1, "uid": 1, "in_pending": True, "in_running": False, "chunked": False},
        {"kind": tap.KIND_USER, "pid": 1, "seq": 2, "uid": 2, "input_ids": [1], "sampling": {}},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 3, "uid": 2, "in_pending": True, "in_running": False, "chunked": True},
        {"kind": tap.KIND_USER, "pid": 1, "seq": 4, "uid": 3, "input_ids": [1], "sampling": {}},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 5, "uid": 3, "in_pending": False, "in_running": True, "chunked": False},
        {"kind": tap.KIND_USER, "pid": 1, "seq": 6, "uid": 4, "input_ids": [1], "sampling": {}},
        {"kind": tap.KIND_DETOK, "pid": 1, "seq": 7, "uid": 4, "next_token": 5, "finished": False},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 8, "uid": 4, "in_pending": False, "in_running": True, "chunked": False},
        {"kind": tap.KIND_DETOK, "pid": 1, "seq": 9, "uid": 4, "next_token": 6, "finished": True},
        {"kind": tap.KIND_USER, "pid": 1, "seq": 10, "uid": 5, "input_ids": [1], "sampling": {}},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 11, "uid": 5, "in_pending": False, "in_running": False, "chunked": False},
        {"kind": tap.KIND_FREE, "pid": 1, "seq": 12, "uid": 3, "table_idx": 7, "dup_free_slots": False},
        {"kind": tap.KIND_FREE, "pid": 1, "seq": 13, "uid": 3, "table_idx": 7, "dup_free_slots": True},
        {"kind": tap.KIND_FREE, "pid": 1, "seq": 14, "uid": 1, "table_idx": 2, "dup_free_slots": False},
        {"kind": tap.KIND_COLLISION, "pid": 1, "seq": 15, "table_idx": 9, "uids": [3, 4]},
    ]

    result = abort_analysis.analyze(records)

    assert result["requests_total"] == 5
    assert result["aborts_total"] == 5
    assert result["aborts_by_class"] == {
        "pending": 1,
        "pending_chunked": 1,
        "prefill_window": 1,
        "decode": 1,
        "not_found": 1,
    }
    assert result["late_tokens_after_abort"] == 1
    assert result["frees_total"] == 3
    assert result["double_free_uids"] == [3]
    assert result["double_free_in_prefill_window"] == [3]
    assert result["dup_free_slot_events"] == 1
    assert result["collisions"] == 1
    assert result["collision_uids"] == [3, 4]


def test_failure_mode_precedence():
    base_analysis = abort_analysis.analyze([])
    crashed_run = {
        "watch": {"crashed": 1, "zombie": 0, "restarts": 0},
        "stress_timed_out": False,
        "canary_ok": True,
        "analysis": base_analysis,
    }
    assert abort_analysis.failure_mode(crashed_run) == "crash"

    wedged_run = {
        "watch": {"crashed": 0, "zombie": 0, "restarts": 0},
        "stress_timed_out": True,
        "canary_ok": True,
        "analysis": base_analysis,
    }
    assert abort_analysis.failure_mode(wedged_run) == "wedge"

    corrupted_analysis = dict(base_analysis, collisions=1)
    corrupted_run = {
        "watch": {"crashed": 0, "zombie": 0, "restarts": 0},
        "stress_timed_out": False,
        "canary_ok": True,
        "analysis": corrupted_analysis,
    }
    assert abort_analysis.failure_mode(corrupted_run) == "corrupted_requests"

    double_free_analysis = dict(base_analysis, double_free_uids=[3])
    double_free_run = {
        "watch": {"crashed": 0, "zombie": 0, "restarts": 0},
        "stress_timed_out": False,
        "canary_ok": True,
        "analysis": double_free_analysis,
    }
    assert abort_analysis.failure_mode(double_free_run) == "double_free"

    healthy_run = {
        "watch": {"crashed": 0, "zombie": 0, "restarts": 0},
        "stress_timed_out": False,
        "canary_ok": True,
        "analysis": base_analysis,
    }
    assert abort_analysis.failure_mode(healthy_run) == "none"


# --- slow tracer / double-free tests ----------------------------------------------------

_STRESS_SERVER_CMD = (
    "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model} "
    "--flavor rust --emit-scheduler-child --abort-timing {abort_timing}"
)
_STRESS_CMD = (
    "{python} -m rsglang.testing.fake_parity_server stress --base-url {base_url} "
    "--requests 16 --abort-fraction 0.5 --seed 7"
)


@pytest.mark.slow
def test_tracer_stress_part_both_timings(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)

    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--stress-server-cmd", _STRESS_SERVER_CMD,
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "0.5",
        ],
        extra_path=bin_dir,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_sidecar(doc) == []

    abort_stress = doc["abort_stress"]
    timings = [r["abort_timing"] for r in abort_stress["runs"]]
    assert timings == ["immediate", "deferred"]

    for run in abort_stress["runs"]:
        assert run["stress_rc"] == 0, run
        assert run["stress_timed_out"] is False
        assert run["canary_ok"] is True
        assert run["watch"]["verdict"] == "healthy"
        assert run["failure_mode"] == "none"
        assert run["analysis"]["aborts_total"] > 0

    assert abort_stress["reproduced"] is False


@pytest.mark.slow
def test_double_free_reproduced(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)

    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--stress-server-cmd", _STRESS_SERVER_CMD + " --double-free-on-abort",
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "0.5",
        ],
        extra_path=bin_dir,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    abort_stress = doc["abort_stress"]
    immediate_run = next(r for r in abort_stress["runs"] if r["abort_timing"] == "immediate")

    assert immediate_run["failure_mode"] == "double_free"
    assert immediate_run["analysis"]["double_free_uids"]
    assert immediate_run["analysis"]["dup_free_slot_events"] > 0
    assert abort_stress["reproduced"] is True


def test_stress_requires_stress_cmd(tmp_path):
    result = _run_parity_check(["run", "--parts", "stress"])
    assert result.returncode == 2
    assert "--stress-cmd" in result.stderr


# --- Task 2: crash/wedge/integrity failure modes and annotate_sequence wiring ----------


@pytest.mark.slow
def test_crash_failure_mode(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--abort-timings", "immediate",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--stress-server-cmd", _STRESS_SERVER_CMD + " --crash-after-requests 4",
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "1.0",
            "--watch-interval-s", "0.1",
        ],
        extra_path=bin_dir,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    run = doc["abort_stress"]["runs"][0]
    assert run["abort_timing"] == "immediate"
    # The fake server never reaps the killed child, so the watcher may see it
    # as a zombie (state Z) rather than fully "gone" -- either way the
    # watcher's own crash/zombie detection (not liveness alone) is what
    # failure_mode reduces to "crash" (06-02's watcher contract).
    assert run["watch"]["crashed"] == 1 or run["watch"]["zombie"] == 1
    assert run["failure_mode"] == "crash"


@pytest.mark.slow
def test_wedge_failure_mode(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--abort-timings", "immediate",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--stress-server-cmd", _STRESS_SERVER_CMD + " --hang-after-requests 4",
            "--stress-cmd", _STRESS_CMD,
            "--stress-timeout", "5",
            "--canary-timeout-s", "3",
            "--settle-s", "0.2",
        ],
        extra_path=bin_dir,
        timeout=55,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    run = doc["abort_stress"]["runs"][0]
    assert run["stress_timed_out"] is True
    assert run["canary_ok"] is False
    assert run["failure_mode"] == "wedge"


@pytest.mark.slow
def test_integrity_error_recorded(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--abort-timings", "immediate",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--stress-server-cmd", _STRESS_SERVER_CMD + " --log-integrity-error",
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "2",
        ],
        extra_path=bin_dir,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    run = doc["abort_stress"]["runs"][0]
    assert run["integrity_error"] is not None
    assert "Integrity check failed" in run["integrity_error"]


def test_annotate_wired(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    item1_substr = "uniqueperturbmarkerone"
    item3_substr = "uniquedivergemarkerthree"
    corpus_doc = {
        "schema_version": 1,
        "items": [
            {
                "id": "item1",
                "category": "short",
                "kind": "raw",
                "prompt": f"prompt with {item1_substr} inside",
                "max_tokens": 4,
                "source": "test",
            },
            {
                "id": "item2",
                "category": "short",
                "kind": "raw",
                "prompt": "a plain unrelated prompt",
                "max_tokens": 4,
                "source": "test",
            },
            {
                "id": "item3",
                "category": "short",
                "kind": "raw",
                "prompt": f"prompt with {item3_substr} inside",
                "max_tokens": 4,
                "source": "test",
            },
        ],
    }
    corpus_path.write_text(json.dumps(corpus_doc), encoding="utf-8")

    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    python_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"
    rust_cmd = (
        "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model} "
        f"--perturb-input-when {item1_substr} --diverge-when {item3_substr} --diverge-output-at 0"
    )

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", python_cmd,
            "--rust-server-cmd", rust_cmd,
        ]
    )
    assert result.returncode == 1, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    prompts = {r["prompt_id"]: r for r in doc["sequential"]["fake/model"]["prompts"]}

    assert prompts["item1"]["divergence"]["layer"] == "tokenization"
    assert prompts["item3"]["divergence"]["layer"] == "backend"
    note = prompts["item3"]["divergence"]["note"]
    assert "item1" in note
    assert "radix-cache" in note
