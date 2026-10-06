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

    Real py-spy --nonblocking output can contain a frame "name"/"file"
    string with invalid UTF-8 bytes: py-spy reads process memory without
    pausing it, and a symbol lookup into a native (non-Python) frame can
    misresolve into garbage bytes, observed on a real GPU run against
    api_server's asyncio/uvloop event loop. json.load()'s strict decoder
    raises UnicodeDecodeError on that, which would abort the whole
    measurement run over one unresolvable frame name. Decode with
    errors="replace" instead: the garbage bytes sit inside a JSON string
    value, never across a structural boundary, so replacing them with
    U+FFFD keeps the document valid and that frame simply fails every
    bucket's path-fragment match (falls into "other"), which is the
    correct outcome for a frame py-spy could not symbolize anyway.
    """
    if isinstance(path_or_doc, (str, Path)):
        import json

        with open(path_or_doc, "rb") as fh:
            text = fh.read().decode("utf-8", errors="replace")
        doc = json.loads(text)
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


# ---------------------------------------------------------------------------
# GC pauses, GC-to-TTFT correlation, request and memory summaries
# ---------------------------------------------------------------------------


def slice_window(records: Iterable[Mapping[str, Any]], t0: float, t1: float) -> list:
    """Records whose "t" falls in the closed interval [t0, t1]."""
    return [r for r in records if t0 <= r["t"] <= t1]


def gc_stats(events: Iterable[Mapping[str, Any]], *, t0: float) -> dict:
    """GC-pause-per-role block from already-sliced gc hook records.

    Durations go to ms (duration_s * 1000), unrounded. by_generation keys
    are the strings "0", "1", "2". Percentiles use percentile() over pause
    durations in ms. events become [t_rel_s, duration_ms, generation], with
    t_rel_s = t - t0. A role with 0 collections reports count 0,
    total_pause_ms 0.0, by_generation all 0 and every pause_ms percentile
    None.
    """
    events = list(events)
    by_generation = {"0": 0, "1": 0, "2": 0}
    collected_total = 0
    durations_ms: list[float] = []
    out_events: list[list] = []

    for e in events:
        duration_ms = e["duration_s"] * 1000
        durations_ms.append(duration_ms)
        generation = e.get("generation")
        key = str(generation)
        if key in by_generation:
            by_generation[key] += 1
        collected_total += e.get("collected", 0)
        out_events.append([e["t"] - t0, duration_ms, generation])

    return {
        "count": len(events),
        "by_generation": by_generation,
        "total_pause_ms": sum(durations_ms),
        "pause_ms": {
            "p50": percentile(durations_ms, 50),
            "p99": percentile(durations_ms, 99),
            "max": percentile(durations_ms, 100),
        },
        "collected": collected_total,
        "events": out_events,
    }


def gc_ttft_correlation(
    requests: Iterable[Any], gc_events: Iterable[Mapping[str, Any]]
) -> "dict | None":
    """Correlate GC pauses with P99 TTFT spikes (D-03).

    TTFT in ms is (t_first - t_send) * 1000 for every request whose t_first
    is not None (requests without t_first are excluded). The spike
    threshold is percentile(ttfts, 99); a request is a spike when its TTFT
    is at or above that threshold. A GC pause interval is
    (t - duration_s, t); it overlaps a request when start < t_first and
    end > t_send, both strict (a pause that only touches a window endpoint
    does not count). Overlap rates are with_gc / requests, None for a zero
    denominator. Returns None when no request has t_first.
    """
    pairs = [(r, (r.t_first - r.t_send) * 1000) for r in requests if r.t_first is not None]
    if not pairs:
        return None

    ttfts = [ttft for _, ttft in pairs]
    threshold = percentile(ttfts, 99)

    pauses = [(e["t"] - e["duration_s"], e["t"]) for e in gc_events]

    spike_requests = 0
    spike_with_gc = 0
    nonspike_requests = 0
    nonspike_with_gc = 0

    for r, ttft in pairs:
        overlaps = any(start < r.t_first and end > r.t_send for start, end in pauses)
        if ttft >= threshold:
            spike_requests += 1
            if overlaps:
                spike_with_gc += 1
        else:
            nonspike_requests += 1
            if overlaps:
                nonspike_with_gc += 1

    return {
        "p99_ttft_ms": threshold,
        "spike_requests": spike_requests,
        "spike_with_gc": spike_with_gc,
        "nonspike_requests": nonspike_requests,
        "nonspike_with_gc": nonspike_with_gc,
        "spike_overlap_rate": spike_with_gc / spike_requests if spike_requests else None,
        "nonspike_overlap_rate": (
            nonspike_with_gc / nonspike_requests if nonspike_requests else None
        ),
    }


def summarize_requests(records: Iterable[Any], window_s: float) -> dict:
    """The requests-per-scenario block: counts by outcome, TTFT percentiles,
    and rps. TTFT percentiles (p50, p90, p99, max) are computed, in ms,
    over completed or cancelled records that have t_first. rps is
    completed / window_s, or None when window_s <= 0.
    """
    records = list(records)
    sent = len(records)
    completed = sum(1 for r in records if r.outcome == "completed")
    cancelled = sum(1 for r in records if r.outcome == "cancelled")
    failed = sum(1 for r in records if r.outcome == "failed")

    ttfts = [
        (r.t_first - r.t_send) * 1000
        for r in records
        if r.outcome in ("completed", "cancelled") and r.t_first is not None
    ]

    return {
        "sent": sent,
        "completed": completed,
        "cancelled": cancelled,
        "failed": failed,
        "ttft_ms": {
            "p50": percentile(ttfts, 50),
            "p90": percentile(ttfts, 90),
            "p99": percentile(ttfts, 99),
            "max": percentile(ttfts, 100),
        },
        "rps": completed / window_s if window_s > 0 else None,
    }


def memory_role_summary(
    rss_samples: Iterable[tuple],
    mem_records: Iterable[Mapping[str, Any]],
    top_alloc_sites: Any,
    *,
    t0: float,
) -> dict:
    """The memory-per-role block: RSS curve/summary, tracemalloc
    curve/summary, and the passed-through top allocation sites.

    rss_samples is an iterable of (t, rss_bytes) pairs. rss_curve is
    [[t - t0, rss_bytes], ...]; growth = end - start. mem_records are
    hook.py "mem" records ({"t", "traced_current", "traced_peak"}, see
    rsglang.profiling.hook); tracemalloc_curve is
    [[t - t0, traced_current, traced_peak], ...] and peak is the max
    traced_peak across all records. With no samples, every summary value
    is None and the curves are empty lists.
    """
    rss_samples = list(rss_samples)
    rss_curve = [[t - t0, rss] for t, rss in rss_samples]
    if rss_samples:
        rss_values = [rss for _, rss in rss_samples]
        rss_bytes = {
            "start": rss_samples[0][1],
            "end": rss_samples[-1][1],
            "max": max(rss_values),
            "growth": rss_samples[-1][1] - rss_samples[0][1],
        }
    else:
        rss_bytes = {"start": None, "end": None, "max": None, "growth": None}

    mem_records = list(mem_records)
    tracemalloc_curve = [
        [r["t"] - t0, r["traced_current"], r["traced_peak"]] for r in mem_records
    ]
    if mem_records:
        tracemalloc = {
            "current_start": mem_records[0]["traced_current"],
            "current_end": mem_records[-1]["traced_current"],
            "peak": max(r["traced_peak"] for r in mem_records),
        }
    else:
        tracemalloc = {"current_start": None, "current_end": None, "peak": None}

    return {
        "rss_bytes": rss_bytes,
        "rss_curve": rss_curve,
        "tracemalloc": tracemalloc,
        "tracemalloc_curve": tracemalloc_curve,
        "top_alloc_sites": top_alloc_sites,
    }
