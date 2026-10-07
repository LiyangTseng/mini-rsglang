"""Ids-first, text-second comparison for the parity sweep (PAR-01/PAR-02,
D-04/D-06).

Task 1 implements request_error, backend and detokenization_or_api. Plan
06-03 inserts tokenization, sampling_params and incomplete ahead of backend
in LAYERS without changing compare_prompt's signature or the record shape.
"""

from __future__ import annotations

from typing import Any

LAYERS = (
    "request_error",
    "tokenization",
    "sampling_params",
    "backend",
    "incomplete",
    "detokenization_or_api",
)

_WINDOW = 4


def _window(ids: "list[int]", center: int) -> "list[int]":
    lo = max(0, center - _WINDOW)
    hi = min(len(ids), center + _WINDOW + 1)
    return ids[lo:hi]


def _first_diff(a: "list", b: "list") -> int:
    for i in range(min(len(a), len(b))):
        if a[i] != b[i]:
            return i
    return min(len(a), len(b))


def compare_prompt(item: Any, python_side: dict, rust_side: dict) -> dict:
    python_status = python_side.get("status")
    rust_status = rust_side.get("status")

    divergence: "dict | None" = None
    ids_match: "bool | None" = None
    text_match: "bool | None" = None

    if python_status == "error" or rust_status == "error":
        divergence = {
            "layer": "request_error",
            "first_index": None,
            "python_window": None,
            "rust_window": None,
            "text_offset": None,
            "note": f"python status={python_status!r} rust status={rust_status!r}",
        }
    else:
        python_ids = python_side.get("output_ids") or []
        rust_ids = rust_side.get("output_ids") or []
        ids_match = python_ids == rust_ids

        if not ids_match:
            first_index = _first_diff(python_ids, rust_ids)
            divergence = {
                "layer": "backend",
                "first_index": first_index,
                "python_window": _window(python_ids, first_index),
                "rust_window": _window(rust_ids, first_index),
                "text_offset": None,
                "note": "output token ids differ",
            }
        else:
            python_text = python_side.get("text") or ""
            rust_text = rust_side.get("text") or ""
            text_match = python_text == rust_text

            if not text_match:
                text_offset = _first_diff(python_text, rust_text)
                divergence = {
                    "layer": "detokenization_or_api",
                    "first_index": None,
                    "python_window": None,
                    "rust_window": None,
                    "text_offset": text_offset,
                    "note": "detokenized text differs though output ids matched",
                }

    return {
        "prompt_id": item.id,
        "category": item.category,
        "kind": item.kind,
        "python": python_side,
        "rust": rust_side,
        "ids_match": ids_match,
        "text_match": text_match,
        "match": divergence is None,
        "divergence": divergence,
    }


def summarize(records: "list[dict]") -> dict:
    n = len(records)
    matched = sum(1 for r in records if r["match"])
    ids_matched = sum(1 for r in records if r["ids_match"] is True)
    text_matched = sum(1 for r in records if r["text_match"] is True)

    by_layer = {layer: 0 for layer in LAYERS}
    for r in records:
        if r["divergence"] is not None:
            by_layer[r["divergence"]["layer"]] += 1

    by_category: "dict[str, dict[str, int]]" = {}
    for r in records:
        entry = by_category.setdefault(r["category"], {"n": 0, "matched": 0})
        entry["n"] += 1
        if r["match"]:
            entry["matched"] += 1

    return {
        "n": n,
        "matched": matched,
        "ids_matched": ids_matched,
        "text_matched": text_matched,
        "by_layer": by_layer,
        "by_category": by_category,
    }
