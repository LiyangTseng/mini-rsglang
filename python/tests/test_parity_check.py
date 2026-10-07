"""Mac-side tests for scripts/parity_check.py (PAR-01/PAR-02). Task 1's
tracer (test_tracer_sequential_identical, test_tracer_divergence_exits_1,
test_validate_cli) proves the whole sweep/tap-join/compare/sidecar path end
to end against the fake parity server; nothing else in the phase depends on
this path until these are green.
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
from pathlib import Path

import pytest

from rsglang.parity import sidecar
from rsglang.parity.compare import LAYERS

REPO_ROOT = Path(__file__).resolve().parents[2]


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _write_corpus(path: Path) -> None:
    doc = {
        "schema_version": 1,
        "items": [
            {
                "id": "chat-1",
                "category": "short",
                "kind": "chat",
                "messages": [
                    {"role": "system", "content": "You are helpful."},
                    {"role": "user", "content": "Hello there"},
                ],
                "max_tokens": 8,
                "source": "test",
            },
            {
                "id": "raw-1",
                "category": "raw",
                "kind": "raw",
                "prompt": "Once upon a time, this has a unique raw prompt marker",
                "max_tokens": 8,
                "source": "test",
            },
        ],
    }
    path.write_text(json.dumps(doc), encoding="utf-8")


def _write_concurrent_corpus(path: Path, n: int = 8) -> None:
    items = []
    for i in range(n):
        items.append(
            {
                "id": f"item-{i}",
                "category": "short",
                "kind": "raw",
                "prompt": f"distinct concurrent prompt marker number {i}",
                "max_tokens": 4,
                "source": "test",
            }
        )
    path.write_text(json.dumps({"schema_version": 1, "items": items}), encoding="utf-8")


def _run_parity_check(args: list, timeout: int = 120) -> "subprocess.CompletedProcess[str]":
    return subprocess.run(
        [sys.executable, "scripts/parity_check.py", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


@pytest.mark.slow
def test_tracer_sequential_identical(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_corpus(corpus_path)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "sequential fake/model: 2/2 identical" in result.stdout

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_sidecar(doc) == []

    block = doc["sequential"]["fake/model"]
    assert block["summary"]["n"] == 2
    assert block["summary"]["matched"] == 2

    for record in block["prompts"]:
        assert record["python"]["output_ids"] == record["rust"]["output_ids"]
        assert record["python"]["output_ids"]
        assert record["python"]["input_ids"]


@pytest.mark.slow
def test_tracer_divergence_exits_1(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_corpus(corpus_path)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    python_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"
    rust_cmd = (
        "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model} "
        "--diverge-when 'unique raw prompt marker' --diverge-output-at 3"
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
    assert sidecar.validate_sidecar(doc) == []

    prompts = {r["prompt_id"]: r for r in doc["sequential"]["fake/model"]["prompts"]}
    assert prompts["raw-1"]["match"] is False
    assert prompts["raw-1"]["divergence"]["layer"] == "backend"
    assert prompts["raw-1"]["divergence"]["first_index"] == 3
    assert prompts["chat-1"]["match"] is True


def test_validate_cli(tmp_path):
    doc = {
        "schema_version": sidecar.SCHEMA_VERSION,
        "generated_by": sidecar.GENERATED_BY,
        "meta": {},
        "endpoints": None,
        "sequential": {
            "fake/model": {
                "status": "ok",
                "reason": None,
                "summary": {
                    "n": 1,
                    "matched": 1,
                    "ids_matched": 1,
                    "text_matched": 1,
                    "by_layer": {layer: 0 for layer in LAYERS},
                    "by_category": {},
                },
                "prompts": [
                    {
                        "prompt_id": "p1",
                        "category": "x",
                        "kind": "raw",
                        "python": {
                            "status": "ok",
                            "error": None,
                            "uid": 0,
                            "input_ids": [1],
                            "sampling": {},
                            "output_ids": [1],
                            "finished": True,
                            "text": "a",
                        },
                        "rust": {
                            "status": "ok",
                            "error": None,
                            "uid": 0,
                            "input_ids": [1],
                            "sampling": {},
                            "output_ids": [1],
                            "finished": True,
                            "text": "a",
                        },
                        "ids_match": True,
                        "text_match": True,
                        "match": True,
                        "divergence": None,
                    }
                ],
            }
        },
        "concurrent": None,
        "abort_stress": None,
        "warnings": [],
    }
    good_path = tmp_path / "good.json"
    good_path.write_text(json.dumps(doc), encoding="utf-8")

    result = _run_parity_check(["validate", str(good_path)])
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "valid" in result.stdout

    bad_doc = json.loads(json.dumps(doc))
    bad_doc["sequential"]["fake/model"]["summary"]["matched"] = 0
    bad_path = tmp_path / "bad.json"
    bad_path.write_text(json.dumps(bad_doc), encoding="utf-8")

    result = _run_parity_check(["validate", str(bad_path)])
    assert result.returncode == 1


@pytest.mark.slow
def test_concurrent_tracer(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_concurrent_corpus(corpus_path, n=8)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential,concurrent",
            "--concurrency", "8",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "(informational)" in result.stdout

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_sidecar(doc) == []

    block = doc["concurrent"]["fake/model"]
    assert block["status"] == "ok"
    assert block["concurrency"] == 8
    summary = block["summary"]
    assert summary["n"] == 8
    assert summary["matched"] == 8
    assert summary["python_vs_sequential_matched"] == 8
    assert summary["rust_vs_sequential_matched"] == 8
    assert summary["unmatched_tap"] == 0

    # Same corpus/concurrency, but --diverge-under-load 4 on the Rust cmd only.
    python_cmd = server_cmd
    rust_cmd = server_cmd + " --diverge-under-load 4"
    out_path2 = tmp_path / "out2.json"
    work_dir2 = tmp_path / "w2"
    port2 = _free_port()

    result2 = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential,concurrent",
            "--concurrency", "8",
            "--port", str(port2),
            "--out", str(out_path2),
            "--work-dir", str(work_dir2),
            "--python-server-cmd", python_cmd,
            "--rust-server-cmd", rust_cmd,
        ]
    )
    assert result2.returncode == 0, f"stdout={result2.stdout!r} stderr={result2.stderr!r}"
    assert "(informational)" in result2.stdout

    doc2 = json.loads(out_path2.read_text())
    assert sidecar.validate_sidecar(doc2) == []

    seq_block2 = doc2["sequential"]["fake/model"]
    assert seq_block2["summary"]["matched"] == 8

    conc_block2 = doc2["concurrent"]["fake/model"]
    summary2 = conc_block2["summary"]
    assert summary2["matched"] < 8
    assert summary2["rust_vs_sequential_matched"] < 8
    assert summary2["python_vs_sequential_matched"] == 8


def test_concurrent_requires_sequential(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_concurrent_corpus(corpus_path, n=8)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"
    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "concurrent",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 2
    assert "concurrent needs sequential" in result.stderr


@pytest.mark.slow
def test_concurrency_bounds(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_concurrent_corpus(corpus_path, n=8)
    port = _free_port()
    work_dir = tmp_path / "w"
    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    zero_result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential,concurrent",
            "--concurrency", "0",
            "--port", str(port),
            "--out", str(work_dir / "zero.json"),
            "--work-dir", str(work_dir / "zero"),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert zero_result.returncode == 2

    out_path = work_dir / "clamped.json"
    clamp_result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "sequential,concurrent",
            "--concurrency", "50",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir / "clamp"),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert clamp_result.returncode == 0, f"stdout={clamp_result.stdout!r} stderr={clamp_result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert doc["concurrent"]["fake/model"]["concurrency"] == 8
    assert any("clamped" in w for w in doc["warnings"])


@pytest.mark.slow
def test_discover_rust_endpoints(tmp_path):
    port = _free_port()
    out_path = tmp_path / "discover.json"
    work_dir = tmp_path / "w"
    server_cmd = (
        "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model} "
        "--flavor rust"
    )

    result = _run_parity_check(
        [
            "discover",
            "--frontend", "rust",
            "--model", "fake/model",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_discover(doc) == []
    assert len(doc["endpoints"]) == 8
    assert all(e["ok"] for e in doc["endpoints"])
    assert doc["tap"]["patched"] is True


@pytest.mark.slow
def test_discover_python_endpoints(tmp_path):
    port = _free_port()
    out_path = tmp_path / "discover.json"
    work_dir = tmp_path / "w"
    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = _run_parity_check(
        [
            "discover",
            "--frontend", "python",
            "--model", "fake/model",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert sidecar.validate_discover(doc) == []
    assert len(doc["endpoints"]) == 5


@pytest.mark.slow
def test_discover_missing_endpoint_fails(tmp_path):
    port = _free_port()
    out_path = tmp_path / "discover.json"
    work_dir = tmp_path / "w"
    # Python-flavor fake server, but we ask discover to check Rust's endpoints.
    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = _run_parity_check(
        [
            "discover",
            "--frontend", "rust",
            "--model", "fake/model",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 1

    doc = json.loads(out_path.read_text())
    by_name = {e["name"]: e for e in doc["endpoints"]}
    assert by_name["GET /health"]["ok"] is False


@pytest.mark.slow
def test_run_endpoints_part_and_verdict_c1(tmp_path):
    corpus_path = tmp_path / "corpus.json"
    _write_corpus(corpus_path)
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"
    server_cmd = (
        "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model} "
        "--flavor rust"
    )

    result = _run_parity_check(
        [
            "run",
            "--models", "fake/model",
            "--corpus", str(corpus_path),
            "--parts", "endpoints",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ]
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text())
    assert len(doc["endpoints"]["python"]) == 5
    assert len(doc["endpoints"]["rust"]) == 8

    verdict_result = _run_parity_check(["verdict", str(out_path), "--criterion", "1"])
    assert verdict_result.returncode == 0, f"stdout={verdict_result.stdout!r}"
    assert "criterion 1: PASS" in verdict_result.stdout

    bad_doc = json.loads(json.dumps(doc))
    for entry in bad_doc["endpoints"]["rust"]:
        if entry["name"] == "GET /health":
            entry["ok"] = False
    bad_path = tmp_path / "bad.json"
    bad_path.write_text(json.dumps(bad_doc), encoding="utf-8")

    bad_verdict_result = _run_parity_check(["verdict", str(bad_path), "--criterion", "1"])
    assert bad_verdict_result.returncode == 1
    assert "criterion 1: FAIL" in bad_verdict_result.stdout


def test_port_in_use_exits_2(tmp_path):
    port = _free_port()
    out_path = tmp_path / "discover.json"
    work_dir = tmp_path / "w"
    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as blocker:
        blocker.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        blocker.bind(("127.0.0.1", port))
        blocker.listen(1)

        result = _run_parity_check(
            [
                "discover",
                "--frontend", "python",
                "--model", "fake/model",
                "--port", str(port),
                "--out", str(out_path),
                "--work-dir", str(work_dir),
                "--server-cmd", server_cmd,
            ]
        )
        assert result.returncode == 2
        assert "in use" in result.stderr
