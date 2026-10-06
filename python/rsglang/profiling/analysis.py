"""Pure analysis layer for BENCH-01 (py-spy speedscope + hook.py records ->
sidecar metric blocks). Standard library only.

This module turns py-spy speedscope profiles, rsglang.profiling.hook JSONL
records (gc/mem/alloc_top, see python/rsglang/profiling/hook.py) and request
timing records (plan 02-06's RequestRecord, duck-typed -- this module never
imports scenarios.py) into the exact per-scenario metric blocks of the
sidecar schema (python/rsglang/profiling/sidecar.py).

Every value is left unrounded (rounding belongs to the markdown report, not
here) and every zero-denominator / empty-input case returns None rather than
raising or silently reporting 0.
"""

from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


class SpeedscopeError(ValueError):
    """Raised by load_speedscope() when the document doesn't match the
    py-spy speedscope shape (RESEARCH Assumption A3): fail loudly on a
    missing key or a samples/weights length mismatch rather than silently
    producing a wrong bucket count.
    """


def load_speedscope(path_or_doc: "str | Path | Mapping[str, Any]") -> dict:
    """Load and validate a py-spy speedscope document.

    Accepts a path to a JSON file, or an already-parsed dict. Requires
    shared.frames (a list) and profiles (a list); each profile needs
    samples and weights lists of equal length. Raises SpeedscopeError
    naming the missing key or the offending profile's index otherwise.
    """
    if isinstance(path_or_doc, (str, Path)):
        import json

        with open(path_or_doc, "r", encoding="utf-8") as fh:
            doc = json.load(fh)
    else:
        doc = path_or_doc

    if not isinstance(doc, dict):
        raise SpeedscopeError("speedscope document must be a dict")

    shared = doc.get("shared")
    if not isinstance(shared, dict) or not isinstance(shared.get("frames"), list):
        raise SpeedscopeError("missing or invalid 'shared.frames'")

    profiles = doc.get("profiles")
    if not isinstance(profiles, list):
        raise SpeedscopeError("missing or invalid 'profiles'")

    for i, profile in enumerate(profiles):
        if not isinstance(profile, dict):
            raise SpeedscopeError(f"profile {i}: must be a dict")
        samples = profile.get("samples")
        weights = profile.get("weights")
        if not isinstance(samples, list):
            raise SpeedscopeError(f"profile {i}: missing or invalid 'samples'")
        if not isinstance(weights, list):
            raise SpeedscopeError(f"profile {i}: missing or invalid 'weights'")
        if len(samples) != len(weights):
            raise SpeedscopeError(
                f"profile {i}: samples ({len(samples)}) and weights ({len(weights)}) length mismatch"
            )

    return doc


# ---------------------------------------------------------------------------
# Frame rules and bucketing
# ---------------------------------------------------------------------------

# A rule is (path_fragment, names). names is a frozenset of function names,
# or None meaning "any function in this file".
FrameRule = tuple  # (str, "frozenset[str] | None")

RADIX_RULES: tuple = (
    ("minisgl/scheduler/cache.py", frozenset({"match_req", "cache_req"})),
    (
        "minisgl/kvcache/radix_cache.py",
        frozenset({"match_prefix", "insert_prefix", "evict", "_tree_walk"}),
    ),
)

# Insertion-ordered: every output of bucket_samples() carries all of these
# keys, even when a bucket's count is 0. Names match sidecar.REQUIRED_BUCKETS.
BUCKETS: dict = {
    "radix": RADIX_RULES,
    "ipc_zmq": (("minisgl/utils/mp.py", None),),
    "serde": (("minisgl/message/utils.py", None),),
    "tokenize": (("minisgl/tokenizer/tokenize.py", None),),
    "detokenize": (("minisgl/tokenizer/detokenize.py", None),),
    "http_stack": (("/uvicorn/", None), ("/starlette/", None), ("/fastapi/", None)),
    "api_handlers": (("minisgl/server/api_server.py", None),),
}


def frame_matches(frame: Mapping[str, Any], rule: tuple) -> bool:
    """True when frame's file contains rule's path fragment and, unless
    rule's names is None, frame's function name matches one of them either
    as given (co_name, e.g. "match_prefix") or as the last dotted component
    of a qualified name (e.g. "RadixPrefixCache.match_prefix").
    """
    fragment, names = rule
    file = str(frame.get("file", "")).replace("\\", "/")
    if fragment not in file:
        return False
    if names is None:
        return True
    name = str(frame.get("name", ""))
    if name in names:
        return True
    short = name.rsplit(".", 1)[-1]
    return short in names


def _frame_matches_any(frame: Mapping[str, Any], rules: Iterable[tuple]) -> bool:
    return any(frame_matches(frame, rule) for rule in rules)


def bucket_samples(doc: Mapping[str, Any], buckets: Mapping[str, tuple] = BUCKETS) -> dict:
    """Bucket every sample across every profile in doc by frame rules.

    A sample counts at most once per bucket even when several of its frames
    match that bucket's rules (nested calls don't double count). Iterates
    over every profile (py-spy writes one profile per thread) and every
    sample, with a per-frame-index match table precomputed once so cost
    stays linear in the number of samples.
    """
    frames = doc["shared"]["frames"]
    bucket_names = list(buckets.keys())

    # Precompute, for each frame index, which buckets it matches.
    frame_bucket_mask: list[frozenset] = []
    for frame in frames:
        matched = frozenset(
            name for name, rules in buckets.items() if _frame_matches_any(frame, rules)
        )
        frame_bucket_mask.append(matched)

    bucket_counts = {name: 0 for name in bucket_names}
    total_samples = 0
    for profile in doc["profiles"]:
        for stack in profile["samples"]:
            total_samples += 1
            hit: set = set()
            for idx in stack:
                hit |= frame_bucket_mask[idx]
            for name in hit:
                bucket_counts[name] += 1

    return {"total_samples": total_samples, "buckets": bucket_counts}


def radix_share(doc: Mapping[str, Any]) -> dict:
    """Radix-cache share of scheduler-process sampled time (D-09/D-10).

    scheduler_samples is the total sample count across every profile in doc
    (D-10's denominator: all sampled time, not end-to-end request latency).
    share is radix_samples / scheduler_samples, unrounded, or None when
    scheduler_samples is 0.

    Known blind spot (CONTEXT D-09/D-11, kernel/radix.py): without py-spy
    --native, time spent inside the native radix kernel's C++
    fast_compare_key is attributed to its calling Python frame (match_prefix
    and friends), so it is still counted here as radix time but cannot be
    split out from the surrounding Python frame.
    """
    result = bucket_samples(doc, buckets={"radix": RADIX_RULES})
    total = result["total_samples"]
    radix = result["buckets"]["radix"]
    share = radix / total if total else None
    return {"radix_samples": radix, "scheduler_samples": total, "share": share}


def cpu_metrics(
    active_doc: Mapping[str, Any],
    gil_doc: "Mapping[str, Any] | None",
    *,
    rate_hz: int,
    window_s: float,
    requests_completed: int,
) -> dict:
    """Per-process CPU-active and GIL-held fractions, plus IPC/serde/
    tokenize/detokenize/HTTP bucket shares and per-request costs (D-01/D-04).

    ticks = rate_hz * window_s. cpu_active_pct/gil_held_pct are
    100 * samples / ticks, None when ticks <= 0. Each bucket's
    share_of_active is samples / active_samples (None if active_samples is
    0); per_request_ms is 1000 * samples / rate_hz / requests_completed
    (None if requests_completed is 0). gil_doc may be None, giving
    gil_samples 0 and gil_held_pct None.
    """
    active_result = bucket_samples(active_doc)
    active_samples = active_result["total_samples"]
    bucket_counts = active_result["buckets"]

    gil_samples = 0
    if gil_doc is not None:
        gil_samples = bucket_samples(gil_doc)["total_samples"]

    ticks = rate_hz * window_s
    cpu_active_pct = 100 * active_samples / ticks if ticks > 0 else None
    gil_held_pct = 100 * gil_samples / ticks if (ticks > 0 and gil_doc is not None) else None

    buckets_out = {}
    for name, count in bucket_counts.items():
        share_of_active = count / active_samples if active_samples > 0 else None
        per_request_ms = (
            1000 * count / rate_hz / requests_completed if requests_completed > 0 else None
        )
        buckets_out[name] = {
            "samples": count,
            "share_of_active": share_of_active,
            "per_request_ms": per_request_ms,
        }

    return {
        "active_samples": active_samples,
        "gil_samples": gil_samples,
        "window_s": window_s,
        "rate_hz": rate_hz,
        "cpu_active_pct": cpu_active_pct,
        "gil_held_pct": gil_held_pct,
        "buckets": buckets_out,
    }


def percentile(values: Sequence[float], p: float) -> "float | None":
    """Nearest-rank percentile: sort, take index ceil(p/100 * n) - 1.

    An empty input gives None. p must be in (0, 100], otherwise ValueError.
    """
    if not (0 < p <= 100):
        raise ValueError(f"p must be in (0, 100], got {p}")
    if not values:
        return None
    sorted_values = sorted(values)
    n = len(sorted_values)
    idx = math.ceil(p / 100 * n) - 1
    idx = max(0, min(idx, n - 1))
    return sorted_values[idx]
