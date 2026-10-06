"""Sidecar schema, provenance metadata, and a validated atomic writer for
BENCH-01's docs/benchmarks/baseline-profile.json (D-13/D-14).

Standard library only. This plan fixes schema_version 1, the top-level shape,
and the discover-mode "discovery" block; 02-04/02-05/02-06/02-07 add scenario
payloads under "scenarios" without changing any of this.
"""

from __future__ import annotations

import json
import math
import os
import platform
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Iterable, Mapping

from .. import handshake

SCHEMA_VERSION = 1
GENERATED_BY = "scripts/baseline_profile.py"
ROLES = ("api_server", "scheduler", "tokenizer")
SCENARIOS = ("s1_cancel", "s2_saturation", "s3_coldstart")
CANONICAL_OUT = "docs/benchmarks/baseline-profile.json"

_META_KEYS = (
    "created_utc",
    "platform",
    "python",
    "git_commit",
    "git_dirty",
    "upstream_sha",
    "model",
    "gpu",
    "py_spy",
    "clock",
    "mode",
)


class SidecarError(ValueError):
    """Raised by write_sidecar() when validate_sidecar() returns any problems."""

    def __init__(self, errors: Iterable[str]):
        self.errors = list(errors)
        super().__init__("; ".join(self.errors))


def _git_commit(repo_root: Path) -> str | None:
    try:
        out = subprocess.run(
            ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except OSError:
        return None
    if out.returncode != 0:
        return None
    return out.stdout.strip() or None


def _git_dirty(repo_root: Path) -> bool | None:
    try:
        out = subprocess.run(
            ["git", "-C", str(repo_root), "status", "--porcelain", "--untracked-files=no"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except OSError:
        return None
    if out.returncode != 0:
        return None
    return bool(out.stdout.strip())


def _gpu_name() -> str | None:
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=name", "--format=csv,noheader"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, FileNotFoundError):
        return None
    if out.returncode != 0:
        return None
    lines = [line.strip() for line in out.stdout.splitlines() if line.strip()]
    return lines[0] if lines else None


def build_meta(
    *,
    mode: str,
    model: str,
    py_spy_version: str | None,
    rate_hz: int,
    flags: list[str],
) -> dict[str, Any]:
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
        "model": model,
        "gpu": _gpu_name(),
        "py_spy": {"version": py_spy_version, "rate_hz": rate_hz, "flags": list(flags)},
        "clock": time.get_clock_info("perf_counter").implementation,
        "mode": mode,
    }


def _is_plain_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _find_nan_inf(value: Any, path: str, errors: list[str]) -> None:
    if isinstance(value, float):
        if math.isnan(value) or math.isinf(value):
            errors.append(f"{path}: NaN or Infinity not allowed")
    elif isinstance(value, dict):
        for key, sub in value.items():
            _find_nan_inf(sub, f"{path}.{key}", errors)
    elif isinstance(value, list):
        for i, sub in enumerate(value):
            _find_nan_inf(sub, f"{path}[{i}]", errors)


def validate_sidecar(doc: Any, *, require_scenarios: Iterable[str] = ()) -> list[str]:
    errors: list[str] = []
    if not isinstance(doc, dict):
        return ["$: document must be a dict"]

    if doc.get("schema_version") != SCHEMA_VERSION:
        errors.append(f"schema_version: must be {SCHEMA_VERSION}")
    if doc.get("generated_by") != GENERATED_BY:
        errors.append(f"generated_by: must be {GENERATED_BY!r}")

    meta = doc.get("meta")
    mode = None
    if not isinstance(meta, dict):
        errors.append("meta: must be a dict")
    else:
        for key in _META_KEYS:
            if key not in meta:
                errors.append(f"meta.{key}: missing")
        mode = meta.get("mode")

    if mode not in ("discover", "run"):
        errors.append("meta.mode: must be 'discover' or 'run'")

    scenarios = doc.get("scenarios")
    if not isinstance(scenarios, dict):
        errors.append("scenarios: must be a dict")
        scenarios = {}
    else:
        for key in scenarios:
            if key not in SCENARIOS:
                errors.append(f"scenarios.{key}: unknown scenario")
    for key in require_scenarios:
        if key not in scenarios:
            errors.append(f"scenarios.{key}: required but missing")

    warnings = doc.get("warnings")
    if not isinstance(warnings, list) or not all(isinstance(w, str) for w in warnings):
        errors.append("warnings: must be a list of str")

    if mode == "discover":
        discovery = doc.get("discovery")
        if not isinstance(discovery, dict):
            errors.append("discovery: must be a dict")
        else:
            processes = discovery.get("processes")
            if not isinstance(processes, dict):
                errors.append("discovery.processes: must be a dict")
                processes = {}
            pids = []
            for role in ROLES:
                pid = processes.get(role)
                if not _is_plain_int(pid) or pid <= 0:
                    errors.append(f"discovery.processes.{role}: must be a positive int")
                else:
                    pids.append(pid)
            if len(pids) != len(ROLES) or len(set(pids)) != len(pids):
                errors.append("discovery.processes: role pids must be three distinct positive ints")
            other = processes.get("other")
            if not isinstance(other, list) or not all(_is_plain_int(p) for p in other):
                errors.append("discovery.processes.other: must be a list of ints")

            hook_active = discovery.get("hook_active")
            if not isinstance(hook_active, dict):
                errors.append("discovery.hook_active: must be a dict")
            else:
                for role in ROLES:
                    if not isinstance(hook_active.get(role), bool):
                        errors.append(f"discovery.hook_active.{role}: must be a bool")

            gc_count = discovery.get("gc_count")
            if not isinstance(gc_count, dict):
                errors.append("discovery.gc_count: must be a dict")
            else:
                for role in ROLES:
                    value = gc_count.get(role)
                    if not _is_plain_int(value) or value < 0:
                        errors.append(f"discovery.gc_count.{role}: must be a non-negative int")

            ready_s = discovery.get("ready_s")
            if isinstance(ready_s, bool) or not isinstance(ready_s, (int, float)):
                errors.append("discovery.ready_s: must be a number")
            elif math.isnan(ready_s) or math.isinf(ready_s) or ready_s < 0:
                errors.append("discovery.ready_s: must be finite and >= 0")

    _find_nan_inf(doc, "$", errors)
    return errors


def write_sidecar(doc: Mapping[str, Any], path: Path, *, require_scenarios: Iterable[str] = ()) -> None:
    errors = validate_sidecar(doc, require_scenarios=require_scenarios)
    if errors:
        raise SidecarError(errors)

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.parent / f".{path.name}.tmp-{os.getpid()}"
    text = json.dumps(doc, indent=2, allow_nan=False) + "\n"
    tmp_path.write_text(text, encoding="utf-8")
    os.replace(tmp_path, path)
