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
from . import corpus
from . import sweep

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

_DISCOVER_TOP_KEYS = ("schema_version", "generated_by", "meta", "frontend", "endpoints", "tap")
_DISCOVER_TAP_KEYS = ("checked", "patched", "user_records", "detok_finished")

_CONCURRENT_RECORD_EXTRA_KEYS = ("python_vs_sequential", "rust_vs_sequential")
_CONCURRENT_SUMMARY_EXTRA_KEYS = (
    "python_vs_sequential_matched",
    "rust_vs_sequential_matched",
    "unmatched_tap",
)

_ABORT_TIMINGS = ("immediate", "deferred")
_FAILURE_MODES = ("crash", "wedge", "corrupted_requests", "double_free", "none", "setup_failed")
_WATCH_KEYS = (
    "pid",
    "samples",
    "crashed",
    "zombie",
    "restarts",
    "gpu_unlisted",
    "nvsmi_errors",
    "verdict",
)
_ABORT_ANALYSIS_KEYS = (
    "requests_total",
    "aborts_total",
    "aborts_by_class",
    "late_tokens_after_abort",
    "frees_total",
    "double_free_uids",
    "double_free_in_prefill_window",
    "dup_free_slot_events",
    "collisions",
    "collision_uids",
)
_STRESS_RUN_KEYS = (
    "abort_timing",
    "stress_rc",
    "stress_timed_out",
    "stress_output_tail",
    "canary_ok",
    "watch",
    "integrity_error",
    "analysis",
    "failure_mode",
)
_PROBE_KEYS = (
    "status",
    "reason",
    "delays_ms",
    "repeats",
    "trials",
    "prompt_source",
    "prefill_window_hits",
    "double_free_total",
    "collisions_total",
    "by_delay",
)
_PROBE_STATUSES = ("ok", "skipped")


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
        "gpu": _gpu_name(),
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


def _validate_endpoint_entry(entry: Any, path: str, errors: "list[str]", expected_names: set) -> None:
    if not isinstance(entry, dict):
        errors.append(f"{path}: must be an object")
        return
    name = entry.get("name")
    if name not in expected_names:
        errors.append(f"{path}.name: {name!r} not in {sorted(expected_names)}")
    if not isinstance(entry.get("ok"), bool):
        errors.append(f"{path}.ok: must be a bool")


def _validate_endpoints_block(block: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(block, dict):
        errors.append(f"{path}: must be an object")
        return
    for frontend_name, expected in (("python", sweep.PYTHON_ENDPOINTS), ("rust", sweep.RUST_ENDPOINTS)):
        entries = block.get(frontend_name)
        if not isinstance(entries, list):
            errors.append(f"{path}.{frontend_name}: must be a list")
            continue
        for i, entry in enumerate(entries):
            _validate_endpoint_entry(entry, f"{path}.{frontend_name}[{i}]", errors, set(expected))


def _validate_watch(watch: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(watch, dict):
        errors.append(f"{path}: must be an object")
        return
    for key in _WATCH_KEYS:
        if key not in watch:
            errors.append(f"{path}.{key}: missing")


def _validate_abort_analysis(analysis: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(analysis, dict):
        errors.append(f"{path}: must be an object")
        return
    for key in _ABORT_ANALYSIS_KEYS:
        if key not in analysis:
            errors.append(f"{path}.{key}: missing")


def _validate_stress_run(run: Any, path: str, errors: "list[str]") -> None:
    if not isinstance(run, dict):
        errors.append(f"{path}: must be an object")
        return
    for key in _STRESS_RUN_KEYS:
        if key not in run:
            errors.append(f"{path}.{key}: missing")
    if run.get("abort_timing") not in _ABORT_TIMINGS:
        errors.append(f"{path}.abort_timing: must be one of {_ABORT_TIMINGS}, got {run.get('abort_timing')!r}")
    if run.get("failure_mode") not in _FAILURE_MODES:
        errors.append(f"{path}.failure_mode: must be one of {_FAILURE_MODES}, got {run.get('failure_mode')!r}")
    if "watch" in run:
        _validate_watch(run["watch"], f"{path}.watch", errors)
    if "analysis" in run:
        _validate_abort_analysis(run["analysis"], f"{path}.analysis", errors)


def _validate_probe_block(block: Any, path: str, errors: "list[str]") -> None:
    if block is None:
        return
    if not isinstance(block, dict):
        errors.append(f"{path}: must be an object or null")
        return
    for key in _PROBE_KEYS:
        if key not in block:
            errors.append(f"{path}.{key}: missing")
    if block.get("status") not in _PROBE_STATUSES:
        errors.append(f"{path}.status: must be one of {_PROBE_STATUSES}, got {block.get('status')!r}")


def _validate_abort_stress_block(block: Any, path: str, errors: "list[str]") -> None:
    if block is None:
        return
    if not isinstance(block, dict):
        errors.append(f"{path}: must be an object or null")
        return
    if "model" not in block:
        errors.append(f"{path}.model: missing")

    runs = block.get("runs")
    if not isinstance(runs, list):
        errors.append(f"{path}.runs: must be a list")
        runs = []
    else:
        for i, run in enumerate(runs):
            _validate_stress_run(run, f"{path}.runs[{i}]", errors)

    _validate_probe_block(block.get("probe"), f"{path}.probe", errors)

    if not isinstance(block.get("reproduced"), bool):
        errors.append(f"{path}.reproduced: must be a bool")
    if not isinstance(block.get("conclusive"), bool):
        errors.append(f"{path}.conclusive: must be a bool")


def validate_discover(doc: Any) -> "list[str]":
    """Validates a `discover` subcommand document: {schema_version,
    generated_by, meta (mode "discover"), frontend, endpoints, tap}."""
    errors: "list[str]" = []
    if not isinstance(doc, dict):
        return ["doc: top level must be an object"]

    for key in _DISCOVER_TOP_KEYS:
        if key not in doc:
            errors.append(f"missing top-level key {key!r}")

    if "schema_version" in doc and doc["schema_version"] != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}, got {doc['schema_version']!r}")

    meta = doc.get("meta")
    if isinstance(meta, dict):
        if meta.get("mode") != "discover":
            errors.append("meta.mode: must be 'discover'")
    elif meta is not None:
        errors.append("meta: must be an object")

    frontend = doc.get("frontend")
    if frontend not in ("python", "rust"):
        errors.append(f"frontend: must be 'python' or 'rust', got {frontend!r}")

    endpoints = doc.get("endpoints")
    expected_names = set(sweep.RUST_ENDPOINTS if frontend == "rust" else sweep.PYTHON_ENDPOINTS)
    if not isinstance(endpoints, list):
        errors.append("endpoints: must be a list")
    else:
        for i, entry in enumerate(endpoints):
            _validate_endpoint_entry(entry, f"endpoints[{i}]", errors, expected_names)

    tap_block = doc.get("tap")
    if isinstance(tap_block, dict):
        for key in _DISCOVER_TAP_KEYS:
            if key not in tap_block:
                errors.append(f"tap.{key}: missing")
            elif not isinstance(tap_block[key], bool):
                errors.append(f"tap.{key}: must be a bool")
    elif tap_block is not None:
        errors.append("tap: must be an object")

    _find_nan_inf(doc, "$", errors)
    return errors


def write_discover(doc: Mapping[str, Any], path: Path) -> None:
    errors = validate_discover(doc)
    if errors:
        raise SidecarError(errors)

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.parent / f".{path.name}.tmp-{os.getpid()}"
    text = json.dumps(doc, indent=2, allow_nan=False) + "\n"
    tmp_path.write_text(text, encoding="utf-8")
    os.replace(tmp_path, path)


def validate_sidecar(doc: Any, *, require_gpu: bool = False) -> "list[str]":
    errors: "list[str]" = []
    if not isinstance(doc, dict):
        return ["doc: top level must be an object"]

    for key in _TOP_KEYS:
        if key not in doc:
            errors.append(f"missing top-level key {key!r}")

    if "schema_version" in doc and doc["schema_version"] != SCHEMA_VERSION:
        errors.append(f"schema_version must be {SCHEMA_VERSION}, got {doc['schema_version']!r}")

    endpoints = doc.get("endpoints")
    if endpoints is not None:
        _validate_endpoints_block(endpoints, "endpoints", errors)

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

    abort_stress = doc.get("abort_stress")
    if abort_stress is not None:
        _validate_abort_stress_block(abort_stress, "abort_stress", errors)

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

        mode_value = meta.get("mode") if isinstance(meta, dict) else None
        if mode_value != "run":
            errors.append(f"meta.mode: require_gpu needs 'run', got {mode_value!r}")

        corpus_meta = meta.get("corpus") if isinstance(meta, dict) else None
        if isinstance(corpus_meta, dict):
            if corpus_meta.get("path") != corpus.CANONICAL_CORPUS:
                errors.append(
                    f"meta.corpus.path: require_gpu needs {corpus.CANONICAL_CORPUS!r}, "
                    f"got {corpus_meta.get('path')!r}"
                )
            try:
                canonical_sha = corpus.corpus_sha256(handshake.repo_root() / corpus.CANONICAL_CORPUS)
            except OSError:
                canonical_sha = None
            if canonical_sha is None or corpus_meta.get("sha256") != canonical_sha:
                errors.append("meta.corpus.sha256: require_gpu needs the canonical corpus's sha256")
            n_value = corpus_meta.get("n")
            if not (isinstance(n_value, int) and not isinstance(n_value, bool) and n_value >= 100):
                errors.append(f"meta.corpus.n: require_gpu needs an int >= 100, got {n_value!r}")
        else:
            errors.append("meta.corpus: require_gpu needs a corpus object")

        if not (
            isinstance(endpoints, dict)
            and isinstance(endpoints.get("python"), list)
            and isinstance(endpoints.get("rust"), list)
        ):
            errors.append("endpoints: require_gpu needs both python and rust endpoint lists present")

        models_value = meta.get("models") if isinstance(meta, dict) else None
        gate_model = meta.get("gate_model") if isinstance(meta, dict) else None
        if isinstance(models_value, list):
            if not isinstance(sequential, dict):
                errors.append("sequential: require_gpu needs a block for every meta.models entry")
            else:
                for model in models_value:
                    if model not in sequential:
                        errors.append(
                            f"sequential[{model!r}]: require_gpu needs a block for every meta.models entry"
                        )
        if gate_model is not None:
            if not isinstance(concurrent, dict) or gate_model not in concurrent:
                errors.append(
                    f"concurrent[{gate_model!r}]: require_gpu needs a block for meta.gate_model"
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
