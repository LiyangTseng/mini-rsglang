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


# --- Task 3: backend window probe, conclusive/reproduced refinement, verdict 4 --------


@pytest.mark.slow
def test_probe_wire_delivery(tmp_path):
    import msgpack
    import zmq
    from minisgl.message import BaseBackendMsg

    from rsglang.parity import probe

    # ipc:// paths are limited to sizeof(sockaddr_un.sun_path) (103 chars on
    # macOS); pytest's tmp_path is often too long, so use a short /tmp path.
    sock_path = Path(f"/tmp/rsglang-probe-test-{os.getpid()}.sock")
    sock_path.unlink(missing_ok=True)
    addr = f"ipc://{sock_path}"
    context = zmq.Context()
    sock = context.socket(zmq.PULL)
    sock.bind(addr)
    try:
        trials = probe.run_window_probe(addr, delays_ms=[0, 5], repeats=2, prompt_ids=[1, 2, 3])
        assert len(trials) == 4

        frames = []
        for _ in range(8):
            raw = sock.recv()
            frames.append(BaseBackendMsg.decoder(msgpack.unpackb(raw, raw=False)))

        uids_seen = set()
        for i in range(0, 8, 2):
            user_msg = frames[i]
            abort_msg = frames[i + 1]
            assert type(user_msg).__name__ == "UserMsg"
            assert type(abort_msg).__name__ == "AbortBackendMsg"
            assert user_msg.uid == abort_msg.uid
            assert user_msg.uid >= 1 << 40
            uids_seen.add(user_msg.uid)
            assert user_msg.input_ids.tolist() == [1, 2, 3]
            assert str(user_msg.input_ids.dtype) == "torch.int32"
            assert user_msg.sampling_params.temperature == 0.0
            assert user_msg.sampling_params.max_tokens == 4

        assert len(uids_seen) == 4
        assert {t["uid"] for t in trials} == uids_seen
        for t in trials:
            assert t["delay_ms"] in (0, 5)
    finally:
        sock.close()
        context.term()
        sock_path.unlink(missing_ok=True)


def test_analyze_probe():
    trials = [
        {"uid": 100, "delay_ms": 0},
        {"uid": 101, "delay_ms": 0},
        {"uid": 102, "delay_ms": 5},
        {"uid": 103, "delay_ms": 5},
    ]
    records = [
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 0, "uid": 100, "in_pending": False, "in_running": True, "chunked": False},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 1, "uid": 101, "in_pending": True, "in_running": False, "chunked": False},
        {"kind": tap.KIND_DETOK, "pid": 1, "seq": 2, "uid": 102, "next_token": 1, "finished": False},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 3, "uid": 102, "in_pending": False, "in_running": True, "chunked": False},
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 4, "uid": 103, "in_pending": False, "in_running": False, "chunked": False},
        {"kind": tap.KIND_FREE, "pid": 1, "seq": 5, "uid": 103, "table_idx": 2, "dup_free_slots": False},
        {"kind": tap.KIND_FREE, "pid": 1, "seq": 6, "uid": 103, "table_idx": 2, "dup_free_slots": True},
        {"kind": tap.KIND_COLLISION, "pid": 1, "seq": 7, "table_idx": 9, "uids": [100, 999]},
        # non-probe uid -- must be ignored entirely.
        {"kind": tap.KIND_ABORT, "pid": 1, "seq": 8, "uid": 999, "in_pending": True, "in_running": False, "chunked": False},
    ]

    result = abort_analysis.analyze_probe(records, trials)

    by_delay = {entry["delay_ms"]: entry for entry in result["by_delay"]}
    assert by_delay[0]["trials"] == 2
    assert by_delay[0]["by_class"]["prefill_window"] == 1
    assert by_delay[0]["by_class"]["pending"] == 1
    assert by_delay[0]["collisions"] == 1
    assert by_delay[5]["trials"] == 2
    assert by_delay[5]["by_class"]["decode"] == 1
    assert by_delay[5]["by_class"]["not_found"] == 1
    assert by_delay[5]["double_free"] == 1

    assert result["prefill_window_hits"] == 1
    assert result["double_free_total"] == 1
    assert result["collisions_total"] == 1


@pytest.mark.slow
def test_stress_with_probe_against_fake(tmp_path):
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
            "--stress-server-cmd", _STRESS_SERVER_CMD + " --bind-backend",
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "0.5",
            "--probe-delays-ms", "0,5",
            "--probe-repeats", "2",
        ],
        extra_path=bin_dir,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    probe = doc["abort_stress"]["probe"]
    assert probe["status"] == "ok"
    assert probe["trials"] == 4


@pytest.mark.slow
def test_probe_skipped(tmp_path):
    bin_dir = tmp_path / "bin"
    _write_stub_nvidia_smi(bin_dir)

    port1 = _free_port()
    out_path1 = tmp_path / "out1.json"
    work_dir1 = tmp_path / "w1"
    result1 = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--abort-timings", "immediate",
            "--port", str(port1),
            "--out", str(out_path1),
            "--work-dir", str(work_dir1),
            "--stress-server-cmd", _STRESS_SERVER_CMD,
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "0.5",
            "--probe-delays-ms", "",
        ],
        extra_path=bin_dir,
    )
    assert result1.returncode == 0, f"stdout={result1.stdout!r} stderr={result1.stderr!r}"
    probe1 = json.loads(out_path1.read_text())["abort_stress"]["probe"]
    assert probe1["status"] == "skipped"
    assert probe1["reason"] == "disabled"

    port2 = _free_port()
    out_path2 = tmp_path / "out2.json"
    work_dir2 = tmp_path / "w2"
    result2 = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--parts", "stress",
            "--abort-timings", "immediate",
            "--port", str(port2),
            "--out", str(out_path2),
            "--work-dir", str(work_dir2),
            "--stress-server-cmd", _STRESS_SERVER_CMD,
            "--stress-cmd", _STRESS_CMD,
            "--settle-s", "0.5",
        ],
        extra_path=bin_dir,
    )
    assert result2.returncode == 0, f"stdout={result2.stdout!r} stderr={result2.stderr!r}"
    probe2 = json.loads(out_path2.read_text())["abort_stress"]["probe"]
    assert probe2["status"] == "skipped"
    assert "not found" in probe2["reason"]


def _synthetic_watch(crashed: int = 0) -> dict:
    return {
        "pid": 1,
        "samples": 1,
        "crashed": crashed,
        "zombie": 0,
        "restarts": 0,
        "gpu_unlisted": 0,
        "nvsmi_errors": 0,
        "verdict": "unhealthy" if crashed else "healthy",
    }


def _synthetic_stress_run(timing: str, *, failure_mode: str = "none") -> dict:
    return {
        "abort_timing": timing,
        "stress_rc": 0,
        "stress_timed_out": False,
        "stress_output_tail": "",
        "canary_ok": True,
        "watch": _synthetic_watch(crashed=1 if failure_mode == "crash" else 0),
        "integrity_error": None,
        "analysis": abort_analysis.analyze([]),
        "failure_mode": failure_mode,
    }


def _synthetic_probe_block(*, status: str = "ok", trials: int = 72, reason: "str | None" = None) -> dict:
    return {
        "status": status,
        "reason": reason,
        "delays_ms": [0, 1, 2],
        "repeats": 8,
        "trials": trials,
        "prompt_source": "synthetic",
        "prefill_window_hits": 1,
        "double_free_total": 0,
        "collisions_total": 0,
        "by_delay": [],
    }


def _synthetic_sidecar_doc(abort_stress: dict) -> dict:
    return {
        "schema_version": sidecar.SCHEMA_VERSION,
        "generated_by": sidecar.GENERATED_BY,
        "meta": {"models": ["fake/gate"], "gate_model": "fake/gate"},
        "endpoints": None,
        "sequential": None,
        "concurrent": None,
        "abort_stress": abort_stress,
        "warnings": [],
    }


def test_verdict_c4_synthetic(tmp_path):
    good_abort_stress = {
        "model": "fake/gate",
        "runs": [_synthetic_stress_run("immediate"), _synthetic_stress_run("deferred")],
        "probe": _synthetic_probe_block(),
        "reproduced": False,
        "conclusive": True,
    }
    good_path = tmp_path / "good.json"
    good_path.write_text(json.dumps(_synthetic_sidecar_doc(good_abort_stress)), encoding="utf-8")
    r = _run_parity_check(["verdict", str(good_path), "--criterion", "4"])
    assert r.returncode == 0, f"stdout={r.stdout!r} stderr={r.stderr!r}"
    assert "criterion 4: PASS" in r.stdout

    bad = json.loads(json.dumps(good_abort_stress))
    bad["runs"][1] = _synthetic_stress_run("deferred", failure_mode="crash")
    bad_path = tmp_path / "bad.json"
    bad_path.write_text(json.dumps(_synthetic_sidecar_doc(bad)), encoding="utf-8")
    r2 = _run_parity_check(["verdict", str(bad_path), "--criterion", "4"])
    assert r2.returncode == 1
    assert "criterion 4: FAIL" in r2.stdout

    skipped = json.loads(json.dumps(good_abort_stress))
    skipped["probe"] = _synthetic_probe_block(status="skipped", trials=0, reason="disabled")
    skipped_path = tmp_path / "skipped.json"
    skipped_path.write_text(json.dumps(_synthetic_sidecar_doc(skipped)), encoding="utf-8")
    r3 = _run_parity_check(["verdict", str(skipped_path), "--criterion", "4"])
    assert r3.returncode == 1
    assert "criterion 4: FAIL" in r3.stdout

    immediate_crash = json.loads(json.dumps(good_abort_stress))
    immediate_crash["runs"][0] = _synthetic_stress_run("immediate", failure_mode="crash")
    immediate_crash["reproduced"] = True
    immediate_path = tmp_path / "immediate_crash.json"
    immediate_path.write_text(json.dumps(_synthetic_sidecar_doc(immediate_crash)), encoding="utf-8")
    r4 = _run_parity_check(["verdict", str(immediate_path), "--criterion", "4"])
    assert r4.returncode == 0, f"stdout={r4.stdout!r} stderr={r4.stderr!r}"
    assert "criterion 4: PASS" in r4.stdout
    assert "reproduced=yes" in r4.stdout

    missing_deferred = json.loads(json.dumps(good_abort_stress))
    missing_deferred["runs"] = [missing_deferred["runs"][0]]
    missing_path = tmp_path / "missing.json"
    missing_path.write_text(json.dumps(_synthetic_sidecar_doc(missing_deferred)), encoding="utf-8")
    r5 = _run_parity_check(["verdict", str(missing_path), "--criterion", "4"])
    assert r5.returncode == 1
    assert "criterion 4: FAIL" in r5.stdout
