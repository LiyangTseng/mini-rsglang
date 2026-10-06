"""Unit tests for rsglang.profiling.analysis (BENCH-01): speedscope loading,
frame-matching rules, bucketing, radix share, per-process CPU/GIL metrics,
GC pause statistics, GC-to-P99-TTFT correlation, and request/memory
summaries. Pure functions, synthetic fixtures built inline -- no real py-spy
or hook.py output is read here.
"""

from __future__ import annotations

import pytest

from rsglang.profiling import analysis


# ---------------------------------------------------------------------------
# Speedscope fixture helpers
# ---------------------------------------------------------------------------


def _frame(name: str, file: str, line: int = 1, col: int = 1) -> dict:
    return {"name": name, "file": file, "line": line, "col": col}


def _profile(samples: list, name: str = "thread") -> dict:
    return {
        "type": "sampled",
        "name": name,
        "unit": "seconds",
        "startValue": 0.0,
        "endValue": float(len(samples)),
        "samples": samples,
        "weights": [1.0] * len(samples),
    }


def _doc(frames: list, profiles: list) -> dict:
    return {
        "$schema": "https://www.speedscope.app/file-format-schema.json",
        "shared": {"frames": frames},
        "profiles": profiles,
        "activeProfileIndex": 0,
        "exporter": "test",
        "name": "test",
    }


# ---------------------------------------------------------------------------
# Task 1: speedscope loading
# ---------------------------------------------------------------------------


def test_load_speedscope_valid_and_invalid():
    # ---- valid document loads ----
    frames = [_frame("foo", "/a/foo.py")]
    doc = _doc(frames, [_profile([[0]])])
    loaded = analysis.load_speedscope(doc)
    assert loaded == doc

    # ---- missing "shared" raises SpeedscopeError ----
    bad_no_shared = {"profiles": []}
    with pytest.raises(analysis.SpeedscopeError, match="shared"):
        analysis.load_speedscope(bad_no_shared)

    # ---- a profile missing "samples" raises SpeedscopeError naming the profile ----
    bad_no_samples = _doc(frames, [{"weights": []}])
    with pytest.raises(analysis.SpeedscopeError, match="profile 0"):
        analysis.load_speedscope(bad_no_samples)

    # ---- samples/weights length mismatch raises SpeedscopeError naming the profile ----
    bad_profile = _profile([[0], [0]])
    bad_profile["weights"] = [1.0]  # length 1, samples length 2
    bad_mismatch = _doc(frames, [bad_profile])
    with pytest.raises(analysis.SpeedscopeError, match="profile 0"):
        analysis.load_speedscope(bad_mismatch)


# ---------------------------------------------------------------------------
# Task 1: radix frame bucketing
# ---------------------------------------------------------------------------


def test_radix_frame_bucketing():
    f_match_prefix = _frame("match_prefix", "/opt/venv/lib/minisgl/kvcache/radix_cache.py")
    f_tree_walk = _frame("_tree_walk", "/opt/venv/lib/minisgl/kvcache/radix_cache.py")
    f_cache_req = _frame("cache_req", "/opt/venv/lib/minisgl/scheduler/cache.py")
    f_evict_radix = _frame("RadixPrefixCache.evict", "/opt/venv/lib/minisgl/kvcache/radix_cache.py")
    f_evict_other = _frame("evict", "/opt/venv/lib/other/lru.py")
    f_filler = _frame("something_else", "/opt/venv/lib/unrelated/module.py")

    frames = [f_match_prefix, f_tree_walk, f_cache_req, f_evict_radix, f_evict_other, f_filler]
    # indices:        0              1             2             3              4            5

    profile_a_samples = [
        [0],  # match_prefix
        [0],  # match_prefix
        [0, 1],  # match_prefix with nested _tree_walk -- counts once
        [2],  # cache_req
        [5], [5], [5], [5], [5], [5],  # 6 filler samples
    ]
    assert len(profile_a_samples) == 10

    profile_b_samples = [
        [3],  # RadixPrefixCache.evict -- matches "evict" via qualified-name suffix
        [4],  # evict in other/lru.py -- must NOT match
        [5], [5], [5],
    ]
    assert len(profile_b_samples) == 5

    doc = _doc(frames, [_profile(profile_a_samples, "A"), _profile(profile_b_samples, "B")])

    result = analysis.radix_share(doc)
    assert result == {"radix_samples": 5, "scheduler_samples": 15, "share": 5 / 15}


def test_radix_share_zero_samples():
    frames = [_frame("foo", "/a/foo.py")]
    doc = _doc(frames, [_profile([])])
    result = analysis.radix_share(doc)
    assert result["scheduler_samples"] == 0
    assert result["radix_samples"] == 0
    assert result["share"] is None


def test_bucket_samples_all_buckets():
    f_put = _frame("put", "/opt/venv/lib/minisgl/utils/mp.py")
    f_serialize = _frame("serialize_type", "/opt/venv/lib/minisgl/message/utils.py")
    f_http = _frame("handle", "/opt/venv/lib/site-packages/uvicorn/protocols/http/h11_impl.py")

    frames = [f_put, f_serialize, f_http]
    samples = [
        [0, 1],  # one sample hits both mp.py (ipc_zmq) and message/utils.py (serde)
        [2],  # http_stack
    ]
    doc = _doc(frames, [_profile(samples)])

    result = analysis.bucket_samples(doc)
    assert result["total_samples"] == 2
    assert set(result["buckets"].keys()) == set(analysis.BUCKETS.keys())
    assert result["buckets"]["ipc_zmq"] == 1
    assert result["buckets"]["serde"] == 1
    assert result["buckets"]["http_stack"] == 1
    assert result["buckets"]["radix"] == 0
    assert result["buckets"]["tokenize"] == 0
    assert result["buckets"]["detokenize"] == 0
    assert result["buckets"]["api_handlers"] == 0


# ---------------------------------------------------------------------------
# Task 1: CPU / GIL metrics
# ---------------------------------------------------------------------------


def _cpu_fixture(ipc_count: int, filler_count: int) -> dict:
    f_put = _frame("put", "/opt/venv/lib/minisgl/utils/mp.py")
    f_filler = _frame("x", "/opt/venv/lib/unrelated/module.py")
    frames = [f_put, f_filler]
    samples = [[0]] * ipc_count + [[1]] * filler_count
    return _doc(frames, [_profile(samples)])


def _gil_fixture(count: int) -> dict:
    f_filler = _frame("x", "/opt/venv/lib/unrelated/module.py")
    frames = [f_filler]
    samples = [[0]] * count
    return _doc(frames, [_profile(samples)])


def test_cpu_metrics():
    active_doc = _cpu_fixture(ipc_count=20, filler_count=380)
    gil_doc = _gil_fixture(250)

    result = analysis.cpu_metrics(active_doc, gil_doc, rate_hz=100, window_s=10.0, requests_completed=50)
    assert result["active_samples"] == 400
    assert result["gil_samples"] == 250
    assert result["cpu_active_pct"] == 40.0
    assert result["gil_held_pct"] == 25.0
    assert result["buckets"]["ipc_zmq"]["samples"] == 20
    assert result["buckets"]["ipc_zmq"]["share_of_active"] == 0.05
    assert result["buckets"]["ipc_zmq"]["per_request_ms"] == 4.0

    # window_s 0 -> both pcts None
    result_zero_window = analysis.cpu_metrics(
        active_doc, gil_doc, rate_hz=100, window_s=0, requests_completed=50
    )
    assert result_zero_window["cpu_active_pct"] is None
    assert result_zero_window["gil_held_pct"] is None

    # 0 completed requests -> every per_request_ms is None
    result_zero_requests = analysis.cpu_metrics(
        active_doc, gil_doc, rate_hz=100, window_s=10.0, requests_completed=0
    )
    for bucket in result_zero_requests["buckets"].values():
        assert bucket["per_request_ms"] is None

    # 0 active samples -> every share_of_active is None
    empty_active_doc = _doc([_frame("x", "/a.py")], [_profile([])])
    result_zero_active = analysis.cpu_metrics(
        empty_active_doc, gil_doc, rate_hz=100, window_s=10.0, requests_completed=50
    )
    assert result_zero_active["active_samples"] == 0
    for bucket in result_zero_active["buckets"].values():
        assert bucket["share_of_active"] is None

    # gil_doc may be None
    result_no_gil = analysis.cpu_metrics(
        active_doc, None, rate_hz=100, window_s=10.0, requests_completed=50
    )
    assert result_no_gil["gil_samples"] == 0
    assert result_no_gil["gil_held_pct"] is None


# ---------------------------------------------------------------------------
# Task 1: percentile
# ---------------------------------------------------------------------------


def test_percentile():
    assert analysis.percentile([], 99) is None
    assert analysis.percentile([7.0], 99) == 7.0
    assert analysis.percentile(list(range(1, 101)), 99) == 99
    assert analysis.percentile(list(range(1, 101)), 100) == 100
    assert analysis.percentile([1, 2, 3, 4], 50) == 2
    with pytest.raises(ValueError):
        analysis.percentile([1], 0)
    with pytest.raises(ValueError):
        analysis.percentile([1], 101)
