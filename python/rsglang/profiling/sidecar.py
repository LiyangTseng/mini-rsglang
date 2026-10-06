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

# Required cpu.<role>.buckets names (plan 02-05 produces exactly these).
REQUIRED_BUCKETS = (
    "radix",
    "ipc_zmq",
    "serde",
    "tokenize",
    "detokenize",
    "http_stack",
    "api_handlers",
)

# Cap on memory.per_role.<role>.top_alloc_sites entries, mirrored from
# rsglang.profiling.hook.TOP_ALLOC_SITES.
TOP_ALLOC_SITES = 10

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


def _is_num_type(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _int(
    value: Any,
    path: str,
    errors: list[str],
    *,
    min_value: int | None = None,
    max_value: int | None = None,
    nullable: bool = False,
) -> None:
    """Appends "path: ..." to errors unless value is a plain (non-bool) int
    within [min_value, max_value], or None when nullable."""
    if value is None:
        if not nullable:
            errors.append(f"{path}: must be an int, got None")
        return
    if not _is_plain_int(value):
        errors.append(f"{path}: must be an int, got {type(value).__name__}")
        return
    if min_value is not None and value < min_value:
        errors.append(f"{path}: must be >= {min_value}, got {value}")
    if max_value is not None and value > max_value:
        errors.append(f"{path}: must be <= {max_value}, got {value}")


def _num(
    value: Any,
    path: str,
    errors: list[str],
    *,
    min_value: float | None = None,
    max_value: float | None = None,
    nullable: bool = False,
) -> None:
    """Appends "path: ..." to errors unless value is a finite (non-bool)
    int/float within [min_value, max_value], or None when nullable."""
    if value is None:
        if not nullable:
            errors.append(f"{path}: must be a number, got None")
        return
    if not _is_num_type(value):
        errors.append(f"{path}: must be a number, got {type(value).__name__}")
        return
    if isinstance(value, float) and not math.isfinite(value):
        errors.append(f"{path}: must be finite, got {value}")
        return
    if min_value is not None and value < min_value:
        errors.append(f"{path}: must be >= {min_value}, got {value}")
    if max_value is not None and value > max_value:
        errors.append(f"{path}: must be <= {max_value}, got {value}")


def _rate01(value: Any, path: str, errors: list[str], *, nullable: bool = False) -> None:
    """A num in [0, 1], nullable per caller."""
    _num(value, path, errors, min_value=0, max_value=1, nullable=nullable)


def _validate_pair_list(
    value: Any, path: str, errors: list[str], *, checkers: tuple
) -> None:
    """A list of fixed-length lists, each element checked by checkers[i]."""
    if not isinstance(value, list):
        errors.append(f"{path}: must be a list")
        return
    for i, item in enumerate(value):
        ipath = f"{path}[{i}]"
        if not (isinstance(item, list) and len(item) == len(checkers)):
            errors.append(f"{ipath}: must be a list of length {len(checkers)}")
            continue
        for j, checker in enumerate(checkers):
            checker(item[j], f"{ipath}[{j}]", errors)


def _validate_scalar_list(value: Any, path: str, errors: list[str], *, checker) -> None:
    """A flat list of scalars, each checked by checker(item, item_path, errors)."""
    if not isinstance(value, list):
        errors.append(f"{path}: must be a list")
        return
    for i, item in enumerate(value):
        checker(item, f"{path}[{i}]", errors)


def _validate_processes(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    pids = []
    for role in ROLES:
        pid = value.get(role)
        if not _is_plain_int(pid) or pid <= 0:
            errors.append(f"{path}.{role}: must be a positive int")
        else:
            pids.append(pid)
    other = value.get("other")
    if not isinstance(other, list) or not all(_is_plain_int(p) and p > 0 for p in other):
        errors.append(f"{path}.other: must be a list of positive ints")
    if len(pids) == len(ROLES) and len(set(pids)) != len(pids):
        errors.append(f"{path}: role pids must be three distinct positive ints")


def _validate_requests(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    sent = value.get("sent")
    completed = value.get("completed")
    cancelled = value.get("cancelled")
    failed = value.get("failed")
    _int(sent, f"{path}.sent", errors, min_value=0)
    _int(completed, f"{path}.completed", errors, min_value=0)
    _int(cancelled, f"{path}.cancelled", errors, min_value=0)
    _int(failed, f"{path}.failed", errors, min_value=0)
    if all(_is_plain_int(x) for x in (sent, completed, cancelled, failed)):
        if completed + cancelled + failed != sent:
            errors.append(f"{path}: completed + cancelled + failed must equal sent")

    ttft = value.get("ttft_ms")
    if not isinstance(ttft, dict):
        errors.append(f"{path}.ttft_ms: must be a dict")
    else:
        for key in ("p50", "p90", "p99", "max"):
            _num(ttft.get(key), f"{path}.ttft_ms.{key}", errors, min_value=0, nullable=True)

    _num(value.get("rps"), f"{path}.rps", errors, min_value=0, nullable=True)


def _validate_gc_role(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    count = value.get("count")
    _int(count, f"{path}.count", errors, min_value=0)

    by_gen = value.get("by_generation")
    if not isinstance(by_gen, dict):
        errors.append(f"{path}.by_generation: must be a dict")
    else:
        gen_values = []
        for gen in ("0", "1", "2"):
            gen_value = by_gen.get(gen)
            _int(gen_value, f"{path}.by_generation.{gen}", errors, min_value=0)
            if _is_plain_int(gen_value):
                gen_values.append(gen_value)
        for gen in by_gen:
            if gen not in ("0", "1", "2"):
                errors.append(f"{path}.by_generation.{gen}: unknown generation")
        if len(gen_values) == 3 and _is_plain_int(count) and sum(gen_values) != count:
            errors.append(f"{path}.by_generation: must sum to count")

    _num(value.get("total_pause_ms"), f"{path}.total_pause_ms", errors, min_value=0)

    pause_ms = value.get("pause_ms")
    if not isinstance(pause_ms, dict):
        errors.append(f"{path}.pause_ms: must be a dict")
    else:
        pause_values = {}
        for key in ("p50", "p99", "max"):
            pause_values[key] = pause_ms.get(key)
            _num(pause_values[key], f"{path}.pause_ms.{key}", errors, min_value=0, nullable=True)
        if _is_plain_int(count):
            all_null = all(pause_values[key] is None for key in pause_values)
            if count == 0 and not all_null:
                errors.append(f"{path}.pause_ms: must be all null when count == 0")
            elif count > 0 and any(pause_values[key] is None for key in pause_values):
                errors.append(f"{path}.pause_ms: must not be null when count > 0")

    _int(value.get("collected"), f"{path}.collected", errors, min_value=0)

    _validate_pair_list(
        value.get("events"),
        f"{path}.events",
        errors,
        checkers=(
            lambda v, p, e: _num(v, p, e),
            lambda v, p, e: _num(v, p, e, min_value=0),
            lambda v, p, e: _int(v, p, e, min_value=0, max_value=2),
        ),
    )


def _validate_gc_ttft_x(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    _num(value.get("p99_ttft_ms"), f"{path}.p99_ttft_ms", errors, min_value=0, nullable=True)

    spike_requests = value.get("spike_requests")
    _int(spike_requests, f"{path}.spike_requests", errors, min_value=0)
    spike_with_gc = value.get("spike_with_gc")
    _int(spike_with_gc, f"{path}.spike_with_gc", errors, min_value=0)
    if (
        _is_plain_int(spike_requests)
        and _is_plain_int(spike_with_gc)
        and spike_with_gc > spike_requests
    ):
        errors.append(f"{path}.spike_with_gc: must be <= spike_requests")

    nonspike_requests = value.get("nonspike_requests")
    _int(nonspike_requests, f"{path}.nonspike_requests", errors, min_value=0)
    nonspike_with_gc = value.get("nonspike_with_gc")
    _int(nonspike_with_gc, f"{path}.nonspike_with_gc", errors, min_value=0)
    if (
        _is_plain_int(nonspike_requests)
        and _is_plain_int(nonspike_with_gc)
        and nonspike_with_gc > nonspike_requests
    ):
        errors.append(f"{path}.nonspike_with_gc: must be <= nonspike_requests")

    spike_overlap = value.get("spike_overlap_rate")
    _rate01(spike_overlap, f"{path}.spike_overlap_rate", errors, nullable=True)
    if _is_plain_int(spike_requests):
        if spike_requests == 0 and spike_overlap is not None:
            errors.append(f"{path}.spike_overlap_rate: must be null when spike_requests == 0")
        elif spike_requests > 0 and spike_overlap is None:
            errors.append(f"{path}.spike_overlap_rate: must not be null when spike_requests > 0")

    nonspike_overlap = value.get("nonspike_overlap_rate")
    _rate01(nonspike_overlap, f"{path}.nonspike_overlap_rate", errors, nullable=True)
    if _is_plain_int(nonspike_requests):
        if nonspike_requests == 0 and nonspike_overlap is not None:
            errors.append(
                f"{path}.nonspike_overlap_rate: must be null when nonspike_requests == 0"
            )
        elif nonspike_requests > 0 and nonspike_overlap is None:
            errors.append(
                f"{path}.nonspike_overlap_rate: must not be null when nonspike_requests > 0"
            )


def _validate_gc_ttft_correlation(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    for key in ("frontend", "scheduler"):
        if key not in value:
            errors.append(f"{path}.{key}: missing")
            continue
        sub = value[key]
        if sub is None:
            continue
        _validate_gc_ttft_x(sub, f"{path}.{key}", errors)


def _validate_memory_role(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    rss_bytes = value.get("rss_bytes")
    if not isinstance(rss_bytes, dict):
        errors.append(f"{path}.rss_bytes: must be a dict")
    else:
        _int(rss_bytes.get("start"), f"{path}.rss_bytes.start", errors, min_value=0)
        _int(rss_bytes.get("end"), f"{path}.rss_bytes.end", errors, min_value=0)
        _int(rss_bytes.get("max"), f"{path}.rss_bytes.max", errors, min_value=0, nullable=True)
        _int(rss_bytes.get("growth"), f"{path}.rss_bytes.growth", errors, nullable=True)

    _validate_pair_list(
        value.get("rss_curve"),
        f"{path}.rss_curve",
        errors,
        checkers=(
            lambda v, p, e: _num(v, p, e),
            lambda v, p, e: _int(v, p, e, min_value=0),
        ),
    )

    tracemalloc_v = value.get("tracemalloc")
    if not isinstance(tracemalloc_v, dict):
        errors.append(f"{path}.tracemalloc: must be a dict")
    else:
        _int(
            tracemalloc_v.get("current_start"),
            f"{path}.tracemalloc.current_start",
            errors,
            min_value=0,
        )
        _int(
            tracemalloc_v.get("current_end"),
            f"{path}.tracemalloc.current_end",
            errors,
            min_value=0,
        )
        _int(
            tracemalloc_v.get("peak"),
            f"{path}.tracemalloc.peak",
            errors,
            min_value=0,
            nullable=True,
        )

    _validate_pair_list(
        value.get("tracemalloc_curve"),
        f"{path}.tracemalloc_curve",
        errors,
        checkers=(
            lambda v, p, e: _num(v, p, e),
            lambda v, p, e: _int(v, p, e, min_value=0),
            lambda v, p, e: _int(v, p, e, min_value=0),
        ),
    )

    top_alloc_sites = value.get("top_alloc_sites")
    if top_alloc_sites is not None:
        if not isinstance(top_alloc_sites, list):
            errors.append(f"{path}.top_alloc_sites: must be a list")
        else:
            if len(top_alloc_sites) > TOP_ALLOC_SITES:
                errors.append(f"{path}.top_alloc_sites: at most {TOP_ALLOC_SITES} entries")
            for i, site in enumerate(top_alloc_sites):
                spath = f"{path}.top_alloc_sites[{i}]"
                if not isinstance(site, dict):
                    errors.append(f"{spath}: must be a dict")
                    continue
                if not isinstance(site.get("file"), str):
                    errors.append(f"{spath}.file: must be a str")
                _int(site.get("line"), f"{spath}.line", errors)
                _int(site.get("size_bytes"), f"{spath}.size_bytes", errors, min_value=0)
                _int(site.get("count"), f"{spath}.count", errors, min_value=0)


def _validate_tree_bytes(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    _int(value.get("ready"), f"{path}.ready", errors, min_value=0, nullable=True)
    _int(value.get("end"), f"{path}.end", errors, min_value=0, nullable=True)


def _validate_memory(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    per_role = value.get("per_role")
    if not isinstance(per_role, dict):
        errors.append(f"{path}.per_role: must be a dict")
    else:
        for role in ROLES:
            if role not in per_role:
                errors.append(f"{path}.per_role.{role}: missing")
                continue
            _validate_memory_role(per_role[role], f"{path}.per_role.{role}", errors)

    _validate_tree_bytes(value.get("rss_tree_bytes"), f"{path}.rss_tree_bytes", errors)
    _validate_tree_bytes(value.get("pss_tree_bytes"), f"{path}.pss_tree_bytes", errors)


def _validate_bucket(
    value: Any, path: str, errors: list[str], active_samples: Any
) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    samples = value.get("samples")
    _int(samples, f"{path}.samples", errors, min_value=0)
    if (
        _is_plain_int(samples)
        and _is_plain_int(active_samples)
        and samples > active_samples
    ):
        errors.append(f"{path}.samples: must be <= active_samples")
    _rate01(value.get("share_of_active"), f"{path}.share_of_active", errors, nullable=True)
    _num(value.get("per_request_ms"), f"{path}.per_request_ms", errors, min_value=0, nullable=True)


def _validate_cpu_role(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    active_samples = value.get("active_samples")
    _int(active_samples, f"{path}.active_samples", errors, min_value=0)
    _int(value.get("gil_samples"), f"{path}.gil_samples", errors, min_value=0)
    _num(value.get("window_s"), f"{path}.window_s", errors, min_value=0)
    _int(value.get("rate_hz"), f"{path}.rate_hz", errors, min_value=1)
    _num(value.get("cpu_active_pct"), f"{path}.cpu_active_pct", errors, min_value=0, nullable=True)
    _num(value.get("gil_held_pct"), f"{path}.gil_held_pct", errors, min_value=0, nullable=True)

    buckets = value.get("buckets")
    if not isinstance(buckets, dict):
        errors.append(f"{path}.buckets: must be a dict")
    else:
        for name in REQUIRED_BUCKETS:
            if name not in buckets:
                errors.append(f"{path}.buckets.{name}: missing")
                continue
            _validate_bucket(buckets[name], f"{path}.buckets.{name}", errors, active_samples)
        for name in buckets:
            if name not in REQUIRED_BUCKETS:
                errors.append(f"{path}.buckets.{name}: unknown bucket")


def _validate_radix(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    radix_samples = value.get("radix_samples")
    _int(radix_samples, f"{path}.radix_samples", errors, min_value=0)
    scheduler_samples = value.get("scheduler_samples")
    _int(scheduler_samples, f"{path}.scheduler_samples", errors, min_value=0)
    if (
        _is_plain_int(radix_samples)
        and _is_plain_int(scheduler_samples)
        and radix_samples > scheduler_samples
    ):
        errors.append(f"{path}: radix_samples must be <= scheduler_samples")

    share = value.get("share")
    _rate01(share, f"{path}.share", errors, nullable=True)
    if _is_plain_int(scheduler_samples):
        if scheduler_samples == 0 and share is not None:
            errors.append(f"{path}: share must be null when scheduler_samples == 0")
        elif scheduler_samples > 0 and share is None:
            errors.append(f"{path}: share must not be null when scheduler_samples > 0")


def _validate_artifacts(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return
    if not isinstance(value.get("work_dir"), str):
        errors.append(f"{path}.work_dir: must be a str")


def _validate_coldstart(value: Any, path: str, errors: list[str]) -> None:
    if not isinstance(value, dict):
        errors.append(f"{path}: must be a dict")
        return

    hyperfine = value.get("hyperfine")
    if not isinstance(hyperfine, dict):
        errors.append(f"{path}.hyperfine: must be a dict")
    else:
        for key in ("mean_s", "median_s", "min_s", "max_s"):
            _num(hyperfine.get(key), f"{path}.hyperfine.{key}", errors, min_value=0)
        _num(
            hyperfine.get("stddev_s"),
            f"{path}.hyperfine.stddev_s",
            errors,
            min_value=0,
            nullable=True,
        )
        times_s = hyperfine.get("times_s")
        if not isinstance(times_s, list):
            errors.append(f"{path}.hyperfine.times_s: must be a list")
            times_s = None
        else:
            _validate_scalar_list(
                times_s,
                f"{path}.hyperfine.times_s",
                errors,
                checker=lambda v, p, e: _num(v, p, e, min_value=0),
            )
        runs = hyperfine.get("runs")
        _int(runs, f"{path}.hyperfine.runs", errors, min_value=1)
        if _is_plain_int(runs) and times_s is not None and runs != len(times_s):
            errors.append(f"{path}.hyperfine: runs must equal len(times_s)")

    _validate_scalar_list(
        value.get("ready_s_self_timed"),
        f"{path}.ready_s_self_timed",
        errors,
        checker=lambda v, p, e: _num(v, p, e, min_value=0),
    )
    _validate_scalar_list(
        value.get("rss_tree_bytes_at_ready"),
        f"{path}.rss_tree_bytes_at_ready",
        errors,
        checker=lambda v, p, e: _int(v, p, e, min_value=0),
    )
    _validate_scalar_list(
        value.get("pss_tree_bytes_at_ready"),
        f"{path}.pss_tree_bytes_at_ready",
        errors,
        checker=lambda v, p, e: _int(v, p, e, min_value=0, nullable=True),
    )


def _validate_scenario_entry(key: str, entry: Any, errors: list[str]) -> None:
    path = f"scenarios.{key}"
    if not isinstance(entry, dict):
        errors.append(f"{path}: must be a dict")
        return

    if not isinstance(entry.get("params"), dict):
        errors.append(f"{path}.params: must be a dict")

    _validate_processes(entry.get("processes"), f"{path}.processes", errors)
    _num(entry.get("window_s"), f"{path}.window_s", errors, min_value=0)
    _validate_requests(entry.get("requests"), f"{path}.requests", errors)

    gc_value = entry.get("gc")
    if not isinstance(gc_value, dict):
        errors.append(f"{path}.gc: must be a dict")
    else:
        for role in ROLES:
            if role not in gc_value:
                errors.append(f"{path}.gc.{role}: missing")
                continue
            _validate_gc_role(gc_value[role], f"{path}.gc.{role}", errors)

    _validate_gc_ttft_correlation(
        entry.get("gc_ttft_correlation"), f"{path}.gc_ttft_correlation", errors
    )
    _validate_memory(entry.get("memory"), f"{path}.memory", errors)

    cpu_value = entry.get("cpu")
    if not isinstance(cpu_value, dict):
        errors.append(f"{path}.cpu: must be a dict")
    else:
        for role in ROLES:
            if role not in cpu_value:
                errors.append(f"{path}.cpu.{role}: missing")
                continue
            _validate_cpu_role(cpu_value[role], f"{path}.cpu.{role}", errors)

    _validate_radix(entry.get("radix"), f"{path}.radix", errors)
    _validate_artifacts(entry.get("artifacts"), f"{path}.artifacts", errors)

    if key == "s3_coldstart":
        _validate_coldstart(entry.get("coldstart"), f"{path}.coldstart", errors)
    elif entry.get("coldstart") is not None:
        errors.append(f"{path}.coldstart: only allowed for s3_coldstart")


def validate_sidecar(
    doc: Any, *, require_scenarios: Iterable[str] = (), require_gpu: bool = False
) -> list[str]:
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

    for key in SCENARIOS:
        if key in scenarios:
            _validate_scenario_entry(key, scenarios[key], errors)

    if require_gpu:
        platform_value = meta.get("platform") if isinstance(meta, dict) else None
        gpu_value = meta.get("gpu") if isinstance(meta, dict) else None
        platform_ok = isinstance(platform_value, str) and platform_value.startswith("linux")
        gpu_ok = isinstance(gpu_value, str) and gpu_value != ""
        mode_ok = mode == "run"
        if not (platform_ok and gpu_ok and mode_ok):
            errors.append(
                "meta: require_gpu needs mode='run', platform starting with 'linux', "
                "and a non-empty gpu name"
            )

    _find_nan_inf(doc, "$", errors)
    return errors


def write_sidecar(
    doc: Mapping[str, Any],
    path: Path,
    *,
    require_scenarios: Iterable[str] = (),
    require_gpu: bool = False,
) -> None:
    errors = validate_sidecar(doc, require_scenarios=require_scenarios, require_gpu=require_gpu)
    if errors:
        raise SidecarError(errors)

    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.parent / f".{path.name}.tmp-{os.getpid()}"
    text = json.dumps(doc, indent=2, allow_nan=False) + "\n"
    tmp_path.write_text(text, encoding="utf-8")
    os.replace(tmp_path, path)
