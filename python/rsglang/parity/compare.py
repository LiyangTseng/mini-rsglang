"""Ids-first, text-second comparison for the parity sweep (PAR-01/PAR-02,
D-04/D-06), extended to full D-05 localization.

Task 1 (06-01) implemented request_error, backend and detokenization_or_api
only. Task 2 (06-03) inserts tokenization and sampling_params ahead of
backend, and incomplete ahead of detokenization_or_api, in the fixed
precedence the first matching layer wins: request_error, tokenization,
sampling_params, backend, incomplete, detokenization_or_api. It also adds
annotate_sequence (wired into the run flow by plan 06-05) and a module CLI
(`python -m rsglang.parity.compare explain ...`) for tracing a single
prompt's divergence.

compare_prompt's signature and the record shape (prompt_id, category, kind,
python, rust, ids_match, text_match, match, divergence) are unchanged from
06-01. ids_match/text_match keep their 06-01 meaning: ids_match compares
output_ids regardless of which layer's divergence is reported; text_match is
computed only when ids_match is True, independent of which layer "wins" the
first-match precedence.

Importing this module never imports transformers; the explain CLI's
--tokenizer option imports transformers.AutoTokenizer lazily.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
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
_CONTEXT_WIDTH = 10

# Order fixed here, not dict iteration order, so a sampling_params note's
# "differs: <field>=... vs ..." listing is reproducible across runs.
_SAMPLING_FIELDS = ("temperature", "top_k", "top_p", "ignore_eos", "max_tokens")


def _window(ids: "list[int]", center: int) -> "list[int]":
    lo = max(0, center - _WINDOW)
    hi = min(len(ids), center + _WINDOW + 1)
    return ids[lo:hi]


def _first_diff(a: "list", b: "list") -> int:
    for i in range(min(len(a), len(b))):
        if a[i] != b[i]:
            return i
    return min(len(a), len(b))


def _context(text: str, offset: int, width: int = _CONTEXT_WIDTH) -> str:
    lo = max(0, offset - width)
    hi = min(len(text), offset + width)
    return text[lo:hi]


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
        python_input_ids = python_side.get("input_ids") or []
        rust_input_ids = rust_side.get("input_ids") or []
        python_sampling = python_side.get("sampling") or {}
        rust_sampling = rust_side.get("sampling") or {}
        python_out_ids = python_side.get("output_ids") or []
        rust_out_ids = rust_side.get("output_ids") or []

        # ids_match/text_match are independent summary fields: computed the
        # same way regardless of which layer ends up reported below (06-01
        # meaning preserved).
        ids_match = python_out_ids == rust_out_ids
        python_text = python_side.get("text") or ""
        rust_text = rust_side.get("text") or ""
        if ids_match:
            text_match = python_text == rust_text

        if python_input_ids != rust_input_ids:
            first_index = _first_diff(python_input_ids, rust_input_ids)
            divergence = {
                "layer": "tokenization",
                "first_index": first_index,
                "python_window": _window(python_input_ids, first_index),
                "rust_window": _window(rust_input_ids, first_index),
                "text_offset": None,
                "note": None,
            }
        elif python_sampling != rust_sampling:
            diffs = [
                f"{field}={python_sampling.get(field)!r} vs {rust_sampling.get(field)!r}"
                for field in _SAMPLING_FIELDS
                if python_sampling.get(field) != rust_sampling.get(field)
            ]
            divergence = {
                "layer": "sampling_params",
                "first_index": None,
                "python_window": None,
                "rust_window": None,
                "text_offset": None,
                "note": "differs: " + ", ".join(diffs),
            }
        elif not ids_match:
            first_index = _first_diff(python_out_ids, rust_out_ids)
            divergence = {
                "layer": "backend",
                "first_index": first_index,
                "python_window": _window(python_out_ids, first_index),
                "rust_window": _window(rust_out_ids, first_index),
                "text_offset": None,
                "note": None,
            }
        elif python_side.get("finished") is False or rust_side.get("finished") is False:
            divergence = {
                "layer": "incomplete",
                "first_index": None,
                "python_window": None,
                "rust_window": None,
                "text_offset": None,
                "note": None,
            }
        elif not text_match:
            text_offset = _first_diff(python_text, rust_text)
            divergence = {
                "layer": "detokenization_or_api",
                "first_index": None,
                "python_window": None,
                "rust_window": None,
                "text_offset": text_offset,
                "note": (
                    f"python {_context(python_text, text_offset)!r} vs "
                    f"rust {_context(rust_text, text_offset)!r}"
                ),
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


def annotate_sequence(records: "list[dict]") -> "list[dict]":
    """Walks records in order, remembering the first prompt_id whose layer is
    request_error, tokenization or sampling_params (an input-level
    divergence). Every later backend divergence gets a note that radix-cache
    contents may differ between the two sessions, so it is not misread as
    backend nondeterminism. Mutates and returns the same list.
    """
    earliest_input_level_prompt_id: "str | None" = None
    for record in records:
        divergence = record.get("divergence")
        if divergence is None:
            continue
        layer = divergence.get("layer")
        if layer in ("request_error", "tokenization", "sampling_params"):
            if earliest_input_level_prompt_id is None:
                earliest_input_level_prompt_id = record.get("prompt_id")
        elif layer == "backend" and earliest_input_level_prompt_id is not None:
            divergence["note"] = (
                f"preceded by input-level divergence at {earliest_input_level_prompt_id}; "
                "radix-cache contents may differ between the two sessions"
            )
    return records


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


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m rsglang.parity.compare")
    sub = parser.add_subparsers(dest="command", required=True)

    explain = sub.add_parser(
        "explain", help="Print one prompt's divergence from a parity-report.json sidecar"
    )
    explain.add_argument("file", type=Path, metavar="FILE")
    explain.add_argument("prompt_id", metavar="PROMPT_ID")
    explain.add_argument("--model", required=True, metavar="MODEL")
    explain.add_argument("--block", default="sequential", choices=("sequential", "concurrent"))
    explain.add_argument(
        "--tokenizer",
        default=None,
        metavar="NAME_OR_PATH",
        help="Decode the id windows with this tokenizer (imports transformers lazily)",
    )

    return parser


def _cmd_explain(ns: argparse.Namespace) -> int:
    try:
        doc = json.loads(ns.file.read_text(encoding="utf-8"))
    except OSError as exc:
        print(f"explain: could not read {ns.file}: {exc}", file=sys.stderr)
        return 2

    block = doc.get(ns.block)
    if not isinstance(block, dict):
        print(f"explain: block {ns.block!r} not found in {ns.file}", file=sys.stderr)
        return 2

    model_block = block.get(ns.model)
    if not isinstance(model_block, dict):
        print(f"explain: model {ns.model!r} not found in block {ns.block!r}", file=sys.stderr)
        return 2

    prompts = model_block.get("prompts")
    if not isinstance(prompts, list):
        print(f"explain: no prompts found for model {ns.model!r}", file=sys.stderr)
        return 2

    record = next((r for r in prompts if isinstance(r, dict) and r.get("prompt_id") == ns.prompt_id), None)
    if record is None:
        print(f"explain: prompt {ns.prompt_id!r} not found", file=sys.stderr)
        return 2

    print(f"prompt: {record.get('prompt_id')}")
    print(f"category: {record.get('category')}")

    divergence = record.get("divergence")
    if divergence is None:
        print("match: no divergence")
        return 1

    python_window = divergence.get("python_window")
    rust_window = divergence.get("rust_window")

    if ns.tokenizer:
        from transformers import AutoTokenizer  # lazy: keep a plain `import compare` transformers-free

        tokenizer = AutoTokenizer.from_pretrained(ns.tokenizer)
        if python_window is not None:
            python_window = tokenizer.convert_ids_to_tokens(python_window)
        if rust_window is not None:
            rust_window = tokenizer.convert_ids_to_tokens(rust_window)

    print(f"layer: {divergence.get('layer')}")
    print(f"first_index: {divergence.get('first_index')}")
    print(f"python_window: {python_window}")
    print(f"rust_window: {rust_window}")
    print(f"note: {divergence.get('note')}")
    return 0


def main(argv: "list[str] | None" = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.command == "explain":
        return _cmd_explain(ns)
    return 2


if __name__ == "__main__":
    sys.exit(main())
