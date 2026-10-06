"""Unit tests for rsglang.profiling.analysis (BENCH-01): speedscope loading,
frame-matching rules, bucketing, radix share, per-process CPU/GIL metrics,
GC pause statistics, GC-to-P99-TTFT correlation, and request/memory
summaries. Pure functions, synthetic fixtures built inline -- no real py-spy
or hook.py output is read here.
"""

from __future__ import annotations

from dataclasses import dataclass

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


# ---------------------------------------------------------------------------
# Task 2: slice_window
# ---------------------------------------------------------------------------


def test_slice_window():
    records = [{"t": 1.0}, {"t": 2.0}, {"t": 3.0}]
    result = analysis.slice_window(records, 2.0, 3.0)
    assert result == [{"t": 2.0}, {"t": 3.0}]


# ---------------------------------------------------------------------------
# Task 2: gc_stats
# ---------------------------------------------------------------------------


def test_gc_stats():
    t0_base = 100.0
    events = [
        {"kind": "gc", "pid": 1, "t": t0_base + 1.0, "duration_s": 0.001, "generation": 0, "collected": 5, "uncollectable": 0},
        {"kind": "gc", "pid": 1, "t": t0_base + 2.0, "duration_s": 0.003, "generation": 0, "collected": 0, "uncollectable": 0},
        {"kind": "gc", "pid": 1, "t": t0_base + 3.0, "duration_s": 0.010, "generation": 2, "collected": 7, "uncollectable": 0},
    ]
    t0 = events[0]["t"] - 1.0

    result = analysis.gc_stats(events, t0=t0)
    assert result["count"] == 3
    assert result["by_generation"] == {"0": 2, "1": 0, "2": 1}
    assert result["total_pause_ms"] == pytest.approx(14.0, abs=1e-9)
    assert result["pause_ms"]["max"] == 10.0
    assert result["pause_ms"]["p50"] == 3.0
    assert result["collected"] == 12
    assert result["events"][0] == [1.0, 1.0, 0]

    empty_result = analysis.gc_stats([], t0=0.0)
    assert empty_result["count"] == 0
    assert empty_result["total_pause_ms"] == 0.0
    assert empty_result["pause_ms"]["p50"] is None
    assert empty_result["pause_ms"]["p99"] is None
    assert empty_result["pause_ms"]["max"] is None


# ---------------------------------------------------------------------------
# Task 2: gc_ttft_correlation
# ---------------------------------------------------------------------------


@dataclass
class _FakeReq:
    t_send: float
    t_first: "float | None"
    t_end: float
    outcome: str


def _build_correlation_fixture():
    # 60 qualifying requests (t_first set): 1 outlier (500ms TTFT) + 59 normal
    # (10ms TTFT). With n=60 and p=99, nearest-rank selects the single max
    # (ceil(0.99*60)=60 -> last index), so the outlier is the sole spike.
    requests = []

    # the spike request: t_send=0.0, t_first=0.5 (500ms)
    spike_req = _FakeReq(t_send=0.0, t_first=0.5, t_end=0.6, outcome="completed")
    requests.append(spike_req)

    touch_req = None
    for i in range(1, 60):
        t_send = float(i)
        req = _FakeReq(t_send=t_send, t_first=t_send + 0.01, t_end=t_send + 0.02, outcome="completed")
        requests.append(req)
        if i == 10:
            touch_req = req

    # one extra request without t_first -- must be excluded entirely
    requests.append(_FakeReq(t_send=100.0, t_first=None, t_end=100.1, outcome="cancelled"))

    # GC pause 1: strictly inside the spike request's (t_send, t_first) window
    pause_inside = {"kind": "gc", "pid": 1, "t": 0.3, "duration_s": 0.05, "generation": 0, "collected": 0, "uncollectable": 0}
    # GC pause 2: ends exactly at touch_req's t_send -- touching, not overlapping
    pause_touching = {
        "kind": "gc",
        "pid": 1,
        "t": touch_req.t_send,
        "duration_s": 0.05,
        "generation": 0,
        "collected": 0,
        "uncollectable": 0,
    }
    gc_events = [pause_inside, pause_touching]
    return requests, gc_events


def test_gc_ttft_correlation():
    requests, gc_events = _build_correlation_fixture()

    result = analysis.gc_ttft_correlation(requests, gc_events)
    assert result["p99_ttft_ms"] == 500.0
    assert result["spike_requests"] == 1
    assert result["spike_with_gc"] == 1
    assert result["nonspike_requests"] == 59
    assert result["nonspike_with_gc"] == 0
    assert result["spike_overlap_rate"] == 1.0
    assert result["nonspike_overlap_rate"] == 0.0

    # no request has t_first -> None
    no_ttft_requests = [_FakeReq(t_send=0.0, t_first=None, t_end=1.0, outcome="completed")]
    assert analysis.gc_ttft_correlation(no_ttft_requests, []) is None


# ---------------------------------------------------------------------------
# Task 2: summarize_requests
# ---------------------------------------------------------------------------


def test_summarize_requests():
    records = [
        _FakeReq(t_send=0.0, t_first=0.01, t_end=0.02, outcome="completed"),
        _FakeReq(t_send=0.0, t_first=0.02, t_end=0.03, outcome="completed"),
        _FakeReq(t_send=0.0, t_first=None, t_end=0.01, outcome="cancelled"),
        _FakeReq(t_send=0.0, t_first=None, t_end=0.01, outcome="failed"),
    ]

    result = analysis.summarize_requests(records, 2.0)
    assert result["sent"] == 4
    assert result["completed"] == 2
    assert result["cancelled"] == 1
    assert result["failed"] == 1
    assert result["ttft_ms"]["p50"] == 10.0
    assert result["ttft_ms"]["max"] == 20.0
    assert result["rps"] == 1.0

    result_zero_window = analysis.summarize_requests(records, 0)
    assert result_zero_window["rps"] is None

    empty_result = analysis.summarize_requests([], 2.0)
    assert empty_result["sent"] == 0
    assert empty_result["ttft_ms"]["p50"] is None
    assert empty_result["ttft_ms"]["p90"] is None
    assert empty_result["ttft_ms"]["p99"] is None
    assert empty_result["ttft_ms"]["max"] is None


# ---------------------------------------------------------------------------
# Task 2: memory_role_summary
# ---------------------------------------------------------------------------


def test_memory_role_summary():
    t0 = 50.0
    rss_samples = [(t0 + 0, 100), (t0 + 1, 180), (t0 + 2, 150)]
    mem_records = [
        {"kind": "mem", "pid": 1, "t": t0 + 0, "traced_current": 1000, "traced_peak": 1200},
        {"kind": "mem", "pid": 1, "t": t0 + 1, "traced_current": 1100, "traced_peak": 1500},
    ]

    result = analysis.memory_role_summary(rss_samples, mem_records, None, t0=t0)
    assert result["rss_bytes"] == {"start": 100, "end": 150, "max": 180, "growth": 50}
    assert result["rss_curve"] == [[0.0, 100], [1.0, 180], [2.0, 150]]
    assert result["tracemalloc"]["current_start"] == 1000
    assert result["tracemalloc"]["current_end"] == 1100
    assert result["tracemalloc"]["peak"] == 1500
    assert result["top_alloc_sites"] is None

    empty_result = analysis.memory_role_summary([], [], None, t0=t0)
    assert empty_result["rss_bytes"] == {"start": None, "end": None, "max": None, "growth": None}
    assert empty_result["rss_curve"] == []
    assert empty_result["tracemalloc"] == {"current_start": None, "current_end": None, "peak": None}
    assert empty_result["tracemalloc_curve"] == []

    sites = [{"file": "a.py", "line": 1, "size_bytes": 10, "count": 1}]
    with_sites = analysis.memory_role_summary([], [], sites, t0=t0)
    assert with_sites["top_alloc_sites"] == sites
