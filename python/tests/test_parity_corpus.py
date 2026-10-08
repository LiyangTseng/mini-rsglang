"""Tests for the curated 128-prompt canonical parity corpus (06-03, D-01/D-03).

test_canonical_counts and test_validate_canonical_rejects prove the shape
enforcement (corpus.py's CATEGORIES/CANONICAL_COUNTS/validate_canonical).
test_load_corpus_noncanonical_path_skips_counts proves that enforcement is
scoped to the canonical path only (06-01's generic loader still works for
any other corpus). test_tracer_canonical_corpus_through_sweep is the
end-to-end proof that the real 128-item file flows through the sweep.
"""

from __future__ import annotations

import json
import socket
import subprocess
import sys
from collections import Counter
from pathlib import Path

import pytest

from rsglang.parity.corpus import (
    CANONICAL_CORPUS,
    CANONICAL_COUNTS,
    corpus_sha256,
    load_corpus,
    validate_canonical,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
CANONICAL_PATH = REPO_ROOT / CANONICAL_CORPUS


def _load_canonical_doc() -> dict:
    return json.loads(CANONICAL_PATH.read_text(encoding="utf-8"))


def _write_doc(doc: dict, tmp_path: Path) -> Path:
    path = tmp_path / "corpus.json"
    path.write_text(json.dumps(doc, ensure_ascii=False), encoding="utf-8")
    return path


def _mutate_drop_one_short(doc: dict) -> dict:
    items = doc["items"]
    idx = next(i for i, it in enumerate(items) if it["category"] == "short")
    del items[idx]
    return doc


def _mutate_duplicate_item(doc: dict) -> dict:
    items = doc["items"]
    original = items[0]
    dup = json.loads(json.dumps(original))
    dup["id"] = f"dup-of-{original['id']}"
    items.append(dup)
    return doc


def _mutate_raw_kind_to_chat(doc: dict) -> dict:
    items = doc["items"]
    idx = next(i for i, it in enumerate(items) if it["category"] == "raw")
    raw_item = items[idx]
    raw_item["kind"] = "chat"
    raw_item["messages"] = [{"role": "user", "content": raw_item.get("prompt") or "x"}]
    return doc


def _mutate_strip_assistant_from_multi_turn(doc: dict) -> dict:
    items = doc["items"]
    idx = next(i for i, it in enumerate(items) if it["category"] == "multi_turn")
    mt_item = items[idx]
    mt_item["messages"] = [m for m in mt_item["messages"] if m["role"] != "assistant"]
    return doc


def test_canonical_counts():
    items = load_corpus(CANONICAL_PATH)
    assert validate_canonical(items) == []
    counts = Counter(item.category for item in items)
    assert dict(counts) == CANONICAL_COUNTS


@pytest.mark.parametrize(
    "mutate,expected_substring",
    [
        (_mutate_drop_one_short, "short"),
        (_mutate_duplicate_item, "duplicate"),
        (_mutate_raw_kind_to_chat, "raw"),
        (_mutate_strip_assistant_from_multi_turn, "assistant"),
    ],
    ids=["drop-short", "duplicate-item", "raw-kind-to-chat", "strip-assistant"],
)
def test_validate_canonical_rejects(tmp_path, mutate, expected_substring):
    doc = _load_canonical_doc()
    doc = mutate(doc)
    path = _write_doc(doc, tmp_path)

    items = load_corpus(path)  # tmp path != canonical path: generic checks only
    errors = validate_canonical(items)

    assert errors, "expected validate_canonical to report at least one problem"
    assert any(expected_substring in err for err in errors), errors


def test_load_corpus_noncanonical_path_skips_counts(tmp_path):
    doc = {
        "schema_version": 1,
        "items": [
            {
                "id": "x-1",
                "category": "short",
                "kind": "chat",
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 8,
                "source": "test",
            },
            {
                "id": "x-2",
                "category": "raw",
                "kind": "raw",
                "prompt": "once upon a time",
                "max_tokens": 8,
                "source": "test",
            },
        ],
    }
    path = _write_doc(doc, tmp_path)

    # Must not raise: canonical category-count enforcement only applies when
    # the resolved path is the repo's canonical fixtures/parity/corpus.json.
    items = load_corpus(path)
    assert len(items) == 2


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


@pytest.mark.slow
def test_tracer_canonical_corpus_through_sweep(tmp_path):
    port = _free_port()
    out_path = tmp_path / "out.json"
    work_dir = tmp_path / "w"

    server_cmd = "{python} -m rsglang.testing.fake_parity_server server --port {port} --model {model}"

    result = subprocess.run(
        [
            sys.executable,
            "scripts/parity_check.py",
            "run",
            "--models", "fake/model",
            "--corpus", str(CANONICAL_PATH),
            "--parts", "sequential",
            "--port", str(port),
            "--out", str(out_path),
            "--work-dir", str(work_dir),
            "--python-server-cmd", server_cmd,
            "--rust-server-cmd", server_cmd,
        ],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        timeout=600,
    )
    assert result.returncode == 0, f"stdout={result.stdout!r} stderr={result.stderr!r}"

    doc = json.loads(out_path.read_text(encoding="utf-8"))
    block = doc["sequential"]["fake/model"]

    assert block["summary"]["n"] == 128
    assert block["summary"]["matched"] == 128

    expected_by_category = {
        cat: {"n": n, "matched": n} for cat, n in CANONICAL_COUNTS.items()
    }
    assert block["summary"]["by_category"] == expected_by_category

    assert doc["meta"]["corpus"]["sha256"] == corpus_sha256(CANONICAL_PATH)
    assert doc["meta"]["corpus"]["n"] == 128
