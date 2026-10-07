"""Tests for the full D-05 layer-precedence localization in compare.py
(06-03 Task 2, tdd="true"). RED phase: these exercise behavior that does not
exist yet in 06-01's compare.py (tokenization/sampling_params precedence,
annotate_sequence, the explain CLI).

All tests are fast except test_explain_cli (marked slow: it shells out to a
subprocess). A local `side(**overrides)` helper builds plain dicts matching
the record "side" shape; `item(**overrides)` builds a minimal stand-in with
the id/category/kind attributes compare_prompt reads.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest

from rsglang.parity.compare import LAYERS, annotate_sequence, compare_prompt, summarize

REPO_ROOT = Path(__file__).resolve().parents[2]


def side(**overrides) -> dict:
    base = {
        "status": "ok",
        "error": None,
        "uid": 0,
        "input_ids": [1, 2, 3],
        "sampling": {
            "temperature": 0.0,
            "top_k": -1,
            "top_p": 1.0,
            "ignore_eos": False,
            "max_tokens": 128,
        },
        "output_ids": [5, 6, 7],
        "finished": True,
        "text": "abc",
    }
    base.update(overrides)
    return base


def item(**overrides) -> SimpleNamespace:
    base = {"id": "p1", "category": "cat", "kind": "chat"}
    base.update(overrides)
    return SimpleNamespace(**base)


def test_layer_precedence_request_error():
    record = compare_prompt(item(), side(status="error"), side())
    assert record["divergence"]["layer"] == "request_error"
    assert record["match"] is False


def test_layer_precedence_tokenization():
    python_side = side(input_ids=[1, 2, 3], output_ids=[10, 11])
    rust_side = side(input_ids=[1, 2, 4], output_ids=[20, 21])
    record = compare_prompt(item(), python_side, rust_side)
    divergence = record["divergence"]
    assert divergence["layer"] == "tokenization"
    assert divergence["first_index"] == 2
    assert divergence["python_window"] == [1, 2, 3]
    assert divergence["rust_window"] == [1, 2, 4]


def test_layer_precedence_sampling():
    python_side = side(sampling={
        "temperature": 0.0, "top_k": -1, "top_p": 1.0, "ignore_eos": False, "max_tokens": 128,
    })
    rust_side = side(sampling={
        "temperature": 0.0, "top_k": -1, "top_p": 1.0, "ignore_eos": False, "max_tokens": 64,
    })
    record = compare_prompt(item(), python_side, rust_side)
    divergence = record["divergence"]
    assert divergence["layer"] == "sampling_params"
    assert divergence["first_index"] is None
    assert "max_tokens" in divergence["note"]


def test_backend_first_index():
    record = compare_prompt(item(), side(output_ids=[5, 6, 7]), side(output_ids=[5, 6, 8]))
    divergence = record["divergence"]
    assert divergence["layer"] == "backend"
    assert divergence["first_index"] == 2
    assert divergence["python_window"] == [5, 6, 7]

    # prefix case
    record2 = compare_prompt(item(), side(output_ids=[5, 6]), side(output_ids=[5, 6, 7]))
    assert record2["divergence"]["layer"] == "backend"
    assert record2["divergence"]["first_index"] == 2


def test_backend_window_clipping():
    base_ids = list(range(20))
    python_ids = list(base_ids)
    rust_ids = list(base_ids)
    rust_ids[1] += 100
    record = compare_prompt(item(), side(output_ids=python_ids), side(output_ids=rust_ids))
    divergence = record["divergence"]
    assert divergence["first_index"] == 1
    assert divergence["python_window"] == list(range(0, 6))

    python_ids2 = list(base_ids)
    rust_ids2 = list(base_ids)
    rust_ids2[18] += 100
    record2 = compare_prompt(item(), side(output_ids=python_ids2), side(output_ids=rust_ids2))
    divergence2 = record2["divergence"]
    assert divergence2["first_index"] == 18
    assert divergence2["python_window"] == list(range(14, 20))


def test_incomplete():
    record = compare_prompt(item(), side(finished=True), side(finished=False))
    assert record["divergence"]["layer"] == "incomplete"


def test_detok_layer():
    record = compare_prompt(item(), side(text="abc"), side(text="abd"))
    divergence = record["divergence"]
    assert divergence["layer"] == "detokenization_or_api"
    assert divergence["text_offset"] == 2
    assert "abc" in divergence["note"]
    assert "abd" in divergence["note"]


def test_match():
    record = compare_prompt(item(), side(), side())
    assert record["match"] is True
    assert record["ids_match"] is True
    assert record["text_match"] is True
    assert record["divergence"] is None


def test_annotate_sequence():
    tokenization_record = compare_prompt(
        item(id="p1"), side(input_ids=[1, 2]), side(input_ids=[1, 3])
    )
    match_record = compare_prompt(item(id="p2"), side(), side())
    backend_record = compare_prompt(
        item(id="p3"), side(output_ids=[1, 2]), side(output_ids=[1, 3])
    )

    records = [tokenization_record, match_record, backend_record]
    annotated = annotate_sequence(records)

    assert annotated is records
    note = annotated[2]["divergence"]["note"]
    assert "p1" in note
    assert "radix-cache" in note

    # A backend divergence with no earlier input-level divergence keeps note None.
    only_backend = compare_prompt(
        item(id="q1"), side(output_ids=[1, 2]), side(output_ids=[1, 3])
    )
    annotated2 = annotate_sequence([only_backend])
    assert annotated2[0]["divergence"]["note"] is None


def test_summarize_by_layer():
    records = [compare_prompt(item(), side(), side())]
    summary = summarize(records)
    assert set(summary["by_layer"].keys()) == set(LAYERS)
    for layer in LAYERS:
        assert summary["by_layer"][layer] == 0


def _write_sidecar_fixture(path: Path) -> None:
    doc = {
        "schema_version": 1,
        "generated_by": "test",
        "meta": {},
        "endpoints": None,
        "sequential": {
            "fake/model": {
                "status": "ok",
                "reason": None,
                "summary": None,
                "prompts": [
                    {
                        "prompt_id": "p1",
                        "category": "cat",
                        "kind": "chat",
                        "python": side(output_ids=[5, 6, 7]),
                        "rust": side(output_ids=[5, 6, 8]),
                        "ids_match": False,
                        "text_match": None,
                        "match": False,
                        "divergence": {
                            "layer": "backend",
                            "first_index": 2,
                            "python_window": [5, 6, 7],
                            "rust_window": [5, 6, 8],
                            "text_offset": None,
                            "note": None,
                        },
                    }
                ],
            }
        },
        "concurrent": None,
        "abort_stress": None,
        "warnings": [],
    }
    path.write_text(json.dumps(doc), encoding="utf-8")


@pytest.mark.slow
def test_explain_cli(tmp_path):
    sidecar_path = tmp_path / "report.json"
    _write_sidecar_fixture(sidecar_path)

    result = subprocess.run(
        [sys.executable, "-m", "rsglang.parity.compare", "explain", str(sidecar_path), "p1",
         "--model", "fake/model"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        env={"PYTHONPATH": str(REPO_ROOT / "python")},
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"
    assert "layer: backend" in result.stdout
    assert "first_index: 2" in result.stdout
    assert "[5, 6, 7]" in result.stdout
    assert "[5, 6, 8]" in result.stdout

    result2 = subprocess.run(
        [sys.executable, "-m", "rsglang.parity.compare", "explain", str(sidecar_path), "unknown-id",
         "--model", "fake/model"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        env={"PYTHONPATH": str(REPO_ROOT / "python")},
    )
    assert result2.returncode == 2
