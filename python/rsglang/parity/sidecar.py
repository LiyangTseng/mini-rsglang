"""Sidecar schema and validated atomic writer for PAR-01/PAR-02's
docs/benchmarks/parity-report.json (D-07).

Mirrors python/rsglang/profiling/sidecar.py's schema/meta/validated-writer
pattern. Task 1 implements the sequential-only shape; plan 06-04 extends
validate_sidecar's require_gpu check and adds concurrent/abort_stress
payloads without changing these names.

Standard library only, aside from rsglang.profiling.sidecar's provenance
helpers (imported, not copied).
"""

from __future__ import annotations

import json
import math
import os
import platform
import sys
import time
from pathlib import Path
from typing import Any, Iterable, Mapping

from .. import handshake
from ..profiling.sidecar import _git_commit, _git_dirty, _gpu_name
from . import compare

SCHEMA_VERSION = 1
GENERATED_BY = "scripts/parity_check.py"
CANONICAL_OUT = "docs/benchmarks/parity-report.json"

_TOP_KEYS = (
    "schema_version",
    "generated_by",
    "meta",
    "endpoints",
    "sequential",
    "concurrent",
    "abort_stress",
    "warnings",
)

_SIDE_KEYS = (
    "status",
    "error",
    "uid",
    "input_ids",
    "sampling",
    "output_ids",
    "finished",
    "text",
)

_RECORD_KEYS = (
    "prompt_id",
    "category",
    "kind",
    "python",
    "rust",
    "ids_match",
    "text_match",
    "match",
    "divergence",
)

_BLOCK_STATUSES = ("ok", "unavailable", "failed")

_CONCURRENT_RECORD_EXTRA_KEYS = ("python_vs_sequential", "rust_vs_sequential")
_CONCURRENT_SUMMARY_EXTRA_KEYS = (
    "python_vs_sequential_matched",
    "rust_vs_sequential_matched",
    "unmatched_tap",
)


class SidecarError(ValueError):
    """Raised by write_sidecar() when validate_sidecar() returns any problems."""

    def __init__(self, errors: Iterable[str]):
        self.errors = list(errors)
        super().__init__("; ".join(self.errors))


def build_meta(
    *,
    mode: str,
    models: "list[str]",
    gate_model: str,
    corpus_path: str,
    corpus_sha256: str,
    corpus_n: int,
    concurrency: "int | None",
) -> "dict[str, Any]":
    repo_root = handshake.repo_root()
    try:
        upstream_sha = handshake.read_upstream_sha()
    except (OSError, ValueError):
        upstream_sha = None
    return {
        "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platform": sys.platform,
        "python": platform.python_version(),
        "git_commit": _git_commit(repo_root),
        "git_dirty": _git_dirty(repo_root),
        "upstream_sha": upstream_sha,
        "models": list(models),
        "gate_model": gate_model,
        "corpus": {"path": corpus_path, "sha256": corpus_sha256, "n": corpus_n},
        "concurrency": concurrency,
        "mode": mode,
    }


def _find_nan_inf(value: Any, path: str, errors: "list[str]") -> None:
    if isinstance(value, float):
        if math.isnan(value) or math.isinf(value):
            errors.append(f"{path}: NaN or Infinity not allowed")
    elif isinstance(value, dict):
        for key, sub in value.items():
            _find_nan_inf(sub, f"{path}.{key}", errors)
    elif isinstance(value, list):
        for i, sub in enumerate(value):
            _find_nan_inf(sub, f"{path}[{i}]", errors)


def _validate_side(side: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(side, dict):
        errors.append(f"{path}: must be an object")
        return
    for key in _SIDE_KEYS:
        if key not in side:
            errors.append(f"{path}: missing key {key!r}")


def _validate_record(record: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(record, dict):
        errors.append(f"{path}: must be an object")
        return
    for key in _RECORD_KEYS:
        if key not in record:
            errors.append(f"{path}: missing key {key!r}")
    _validate_side(record.get("python"), f"{path}.python", errors)
    _validate_side(record.get("rust"), f"{path}.rust", errors)
    divergence = record.get("divergence")
    if divergence is not None:
        if not isinstance(divergence, dict):
            errors.append(f"{path}.divergence: must be an object or null")
        elif divergence.get("layer") not in compare.LAYERS:
            errors.append(f"{path}.divergence.layer: must be one of {compare.LAYERS}")


def _is_plain_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _validate_concurrent_record(record: Any, path: str, errors: "list[str]") -> None:
    _validate_record(record, path, errors)
    if not isinstance(record, dict):
        return
    for key in _CONCURRENT_RECORD_EXTRA_KEYS:
        if key not in record:
            errors.append(f"{path}: missing key {key!r}")
        elif not isinstance(record[key], bool):
            errors.append(f"{path}.{key}: must be a bool")


def _validate_concurrent_block(block: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(block, dict):
        errors.append(f"{path}: must be an object")
        return

    status = block.get("status")
    if status not in _BLOCK_STATUSES:
        errors.append(f"{path}.status: must be one of {_BLOCK_STATUSES}, got {status!r}")

    reason = block.get("reason")
    if reason is not None and not isinstance(reason, str):
        errors.append(f"{path}.reason: must be a string or null")

    concurrency = block.get("concurrency")
    if not (_is_plain_int(concurrency) and concurrency >= 1):
        errors.append(f"{path}.concurrency: must be an int >= 1, got {concurrency!r}")

    prompts = block.get("prompts")
    if not isinstance(prompts, list):
        errors.append(f"{path}.prompts: must be a list")
        prompts = []
    else:
        for i, record in enumerate(prompts):
            _validate_concurrent_record(record, f"{path}.prompts[{i}]", errors)

    summary = block.get("summary")
    if summary is not None:
        if not isinstance(summary, dict):
            errors.append(f"{path}.summary: must be an object or null")
        else:
            if summary.get("n") != len(prompts):
                errors.append(f"{path}.summary.n: must equal len(prompts)")
            matched_count = sum(
                1 for r in prompts if isinstance(r, dict) and r.get("match") is True
            )
            if summary.get("matched") != matched_count:
                errors.append(
                    f"{path}.summary.matched: must equal the number of prompts with match true"
                )
            for key in _CONCURRENT_SUMMARY_EXTRA_KEYS:
                if key not in summary:
                    errors.append(f"{path}.summary.{key}: missing")
                elif not _is_plain_int(summary[key]):
                    errors.append(f"{path}.summary.{key}: must be an int")


def _validate_block(block: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(block, dict):
        errors.append(f"{path}: must be an object")
        return

    status = block.get("status")
    if status not in _BLOCK_STATUSES:
        errors.append(f"{path}.status: must be one of {_BLOCK_STATUSES}, got {status!r}")

    reason = block.get("reason")
    if reason is not None and not isinstance(reason, str):
        errors.append(f"{path}.reason: must be a string or null")

    prompts = block.get("prompts")
    if not isinstance(prompts, list):
        errors.append(f"{path}.prompts: must be a list")
        prompts = []
    else:
        for i, record in enumerate(prompts):
            _validate_record(record, f"{path}.prompts[{i}]", errors)

    summary = block.get("summary")
    if summary is not None:
        if not isinstance(summary, dict):
            errors.append(f"{path}.summary: must be an object or null")
        else:
            if summary.get("n") != len(prompts):
                errors.append(f"{path}.summary.n: must equal len(prompts)")
            matched_count = sum(
                1 for r in prompts if isinstance(r, dict) and r.get("match") is True
            )
            if summary.get("matched") != matched_count:
                errors.append(
                    f"{path}.summary.matched: must equal the number of prompts with match true"
                )


def validate_sidecar(doc: Any, *, require_gpu: bool = False) -> "list[str]":
    errors: "list[str]" = []
    if not isinstance(doc, dict):
        return ["doc: top level must be an object"]

    for key in _TOP_KEYS:
        if key not in doc:
            errors.append(f"missing top-level key {key!r}")

    if "schema_version" in doc and doc["schema_version"] != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}, got {doc['schema_version']!r}")

    sequential = doc.get("sequential")
    if sequential is not None:
        if not isinstance(sequential, dict):
            errors.append("sequential: must be an object")
        else:
            for model, block in sequential.items():
                _validate_block(block, f"sequential[{model!r}]", errors)

    concurrent = doc.get("concurrent")
    if concurrent is not None:
        if not isinstance(concurrent, dict):
            errors.append("concurrent: must be an object")
        else:
            for model, block in concurrent.items():
                _validate_concurrent_block(block, f"concurrent[{model!r}]", errors)

    if require_gpu:
        meta = doc.get("meta")
        platform_value = meta.get("platform") if isinstance(meta, dict) else None
        gpu_value = meta.get("gpu") if isinstance(meta, dict) else None
        platform_ok = isinstance(platform_value, str) and platform_value.startswith("linux")
        gpu_ok = isinstance(gpu_value, str) and gpu_value != ""
        if not (platform_ok and gpu_ok):
            errors.append(
                "meta: require_gpu needs platform starting with 'linux' and a non-empty gpu name"
            )

    _find_nan_inf(doc, "$", errors)
    return errors


def write_sidecar(doc: Mapping[str, Any], path: Path, *, require_gpu: bool = False) -> None:
    errors = validate_sidecar(doc, require_gpu=require_gpu)
    if errors:
        raise SidecarError(errors)

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.parent / f".{path.name}.tmp-{os.getpid()}"
    text = json.dumps(doc, indent=2, allow_nan=False) + "\n"
    tmp_path.write_text(text, encoding="utf-8")
    os.replace(tmp_path, path)
