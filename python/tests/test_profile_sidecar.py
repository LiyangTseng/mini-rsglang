"""Tests for the full BENCH-01 scenario-entry schema in rsglang.profiling.sidecar
(plan 02-04): validate_sidecar's per-scenario, per-role GC/memory/CPU/radix keys
and their cross-field invariants, plus the require_gpu gate on
validate_sidecar/write_sidecar.

make_valid_doc() builds a complete run-mode document field by field from the
plan's interfaces schema: all three scenarios, all three roles, all seven
bucket names, and an s3 coldstart block.
"""

from __future__ import annotations

import copy
import json

import pytest

from rsglang.profiling.sidecar import (
    GENERATED_BY,
    REQUIRED_BUCKETS,
    ROLES,
    SCENARIOS,
    SCHEMA_VERSION,
    SidecarError,
    validate_sidecar,
    write_sidecar,
)


def _meta(mode="run", platform="darwin", gpu=None):
    return {
        "created_utc": "2026-10-05T00:00:00Z",
        "platform": platform,
        "python": "3.12.0",
        "git_commit": "abc123",
        "git_dirty": False,
        "upstream_sha": "def456",
        "model": "some-model",
        "gpu": gpu,
        "py_spy": {"version": "0.4.2", "rate_hz": 100, "flags": []},
        "clock": "monotonic",
        "mode": mode,
    }


def _gc_role_entry(count: int):
    if count == 0:
        return {
            "count": 0,
            "by_generation": {"0": 0, "1": 0, "2": 0},
            "total_pause_ms": 0.0,
            "pause_ms": {"p50": None, "p99": None, "max": None},
            "collected": 0,
            "events": [],
        }
    return {
        "count": count,
        "by_generation": {"0": count - 1, "1": 1, "2": 0},
        "total_pause_ms": 5.0,
        "pause_ms": {"p50": 1.0, "p99": 2.0, "max": 2.5},
        "collected": 10,
        "events": [[0.1, 1.0, 0], [0.5, 1.5, 1]],
    }


def _gc_ttft_x():
    return {
        "p99_ttft_ms": 100.0,
        "spike_requests": 2,
        "spike_with_gc": 1,
        "nonspike_requests": 8,
        "nonspike_with_gc": 1,
        "spike_overlap_rate": 0.5,
        "nonspike_overlap_rate": 0.125,
    }


def _memory_role_entry():
    return {
        "rss_bytes": {"start": 1000, "end": 1200, "max": 1300, "growth": 200},
        "rss_curve": [[0.0, 1000], [1.0, 1100]],
        "tracemalloc": {"current_start": 500, "current_end": 600, "peak": 700},
        "tracemalloc_curve": [[0.0, 500, 500], [1.0, 600, 700]],
        "top_alloc_sites": [
            {"file": "a.py", "line": 10, "size_bytes": 100, "count": 1},
        ],
    }


def _cpu_role_entry(active_samples: int = 100):
    buckets = {
        name: {"samples": 5, "share_of_active": 0.05, "per_request_ms": 1.0}
        for name in REQUIRED_BUCKETS
    }
    return {
        "active_samples": active_samples,
        "gil_samples": 50,
        "window_s": 10.0,
        "rate_hz": 100,
        "cpu_active_pct": 50.0,
        "gil_held_pct": 25.0,
        "buckets": buckets,
    }


def _scenario_entry(key: str, *, nulls: bool = False):
    gc_count = 0 if nulls else 2
    entry = {
        "params": {"flag": "value"},
        "processes": {"api_server": 10, "scheduler": 11, "tokenizer": 12, "other": [13]},
        "window_s": 30.0,
        "requests": {
            "sent": 10,
            "completed": 8,
            "cancelled": 1,
            "failed": 1,
            "ttft_ms": (
                {"p50": None, "p90": None, "p99": None, "max": None}
                if nulls
                else {"p50": 10.0, "p90": 20.0, "p99": 30.0, "max": 40.0}
            ),
            "rps": None if nulls else 5.0,
        },
        "gc": {role: _gc_role_entry(gc_count) for role in ROLES},
        "gc_ttft_correlation": {
            "frontend": None if nulls else _gc_ttft_x(),
            "scheduler": None if nulls else _gc_ttft_x(),
        },
        "memory": {
            "per_role": {role: _memory_role_entry() for role in ROLES},
            "rss_tree_bytes": (
                {"ready": None, "end": None} if nulls else {"ready": 2000, "end": 2200}
            ),
            "pss_tree_bytes": (
                {"ready": None, "end": None} if nulls else {"ready": 1800, "end": 2000}
            ),
        },
        "cpu": {role: _cpu_role_entry() for role in ROLES},
        "radix": (
            {"radix_samples": 0, "scheduler_samples": 0, "share": None}
            if nulls
            else {"radix_samples": 3, "scheduler_samples": 10, "share": 0.3}
        ),
        "artifacts": {"work_dir": "/tmp/work"},
    }
    if key == "s3_coldstart":
        entry["coldstart"] = {
            "hyperfine": {
                "mean_s": 1.0,
                "median_s": 1.0,
                "min_s": 0.9,
                "max_s": 1.1,
                "stddev_s": None if nulls else 0.1,
                "times_s": [0.9, 1.0, 1.1],
                "runs": 3,
            },
            "ready_s_self_timed": [1.0, 1.0, 1.0],
            "rss_tree_bytes_at_ready": [1000, 1000, 1000],
            "pss_tree_bytes_at_ready": [None, None, None] if nulls else [900, 900, 900],
        }
    return entry


def make_valid_doc(*, nulls: bool = False):
    """A complete run-mode document: all three scenarios, all three roles, all
    seven bucket names, an s3 coldstart block. With nulls=True, every "?"
    field is None while keeping the null-iff invariants consistent (gc
    count 0, radix scheduler_samples 0, no gc_ttft_correlation data)."""
    return {
        "schema_version": SCHEMA_VERSION,
        "generated_by": GENERATED_BY,
        "meta": _meta(),
        "scenarios": {key: _scenario_entry(key, nulls=nulls) for key in SCENARIOS},
        "warnings": [],
    }


# --- json_sidecar_schema -------------------------------------------------------------


def test_json_sidecar_schema():
    doc = make_valid_doc()
    assert validate_sidecar(doc, require_scenarios=SCENARIOS) == []


def test_null_metrics_allowed():
    doc = make_valid_doc(nulls=True)
    assert validate_sidecar(doc, require_scenarios=SCENARIOS) == []


# --- schema_rejects_each_violation ----------------------------------------------------


def _mutate_delete_scenario(doc):
    del doc["scenarios"]["s2_saturation"]


def _mutate_add_unknown_scenario(doc):
    doc["scenarios"]["s4"] = copy.deepcopy(doc["scenarios"]["s1_cancel"])


def _mutate_delete_gc_role(doc):
    del doc["scenarios"]["s1_cancel"]["gc"]["tokenizer"]


def _mutate_gc_count_bool(doc):
    doc["scenarios"]["s1_cancel"]["gc"]["scheduler"]["count"] = True


def _mutate_radix_share_out_of_range(doc):
    doc["scenarios"]["s1_cancel"]["radix"]["share"] = 1.5


def _mutate_radix_share_nonnull_when_scheduler_zero(doc):
    doc["scenarios"]["s1_cancel"]["radix"] = {
        "radix_samples": 0,
        "scheduler_samples": 0,
        "share": 0.2,
    }


def _mutate_requests_sum_mismatch(doc):
    doc["scenarios"]["s1_cancel"]["requests"]["completed"] = 100


def _mutate_by_generation_sum_mismatch(doc):
    doc["scenarios"]["s1_cancel"]["gc"]["api_server"]["by_generation"]["0"] = 999


def _mutate_pause_ms_null_when_count_positive(doc):
    doc["scenarios"]["s1_cancel"]["gc"]["api_server"]["pause_ms"]["p99"] = None


def _mutate_gil_held_pct_nan(doc):
    doc["scenarios"]["s1_cancel"]["cpu"]["api_server"]["gil_held_pct"] = float("nan")


def _mutate_delete_cpu_bucket(doc):
    del doc["scenarios"]["s1_cancel"]["cpu"]["scheduler"]["buckets"]["ipc_zmq"]


def _mutate_processes_duplicate_pid(doc):
    processes = doc["scenarios"]["s1_cancel"]["processes"]
    processes["tokenizer"] = processes["scheduler"]


def _mutate_top_alloc_sites_too_many(doc):
    site = {"file": "a.py", "line": 1, "size_bytes": 1, "count": 1}
    doc["scenarios"]["s1_cancel"]["memory"]["per_role"]["api_server"]["top_alloc_sites"] = (
        [site] * 11
    )


def _mutate_coldstart_runs_mismatch(doc):
    doc["scenarios"]["s3_coldstart"]["coldstart"]["hyperfine"]["runs"] += 1


VIOLATION_CASES = [
    (_mutate_delete_scenario, "scenarios.s2_saturation"),
    (_mutate_add_unknown_scenario, "scenarios.s4"),
    (_mutate_delete_gc_role, "scenarios.s1_cancel.gc.tokenizer"),
    (_mutate_gc_count_bool, "scenarios.s1_cancel.gc.scheduler.count"),
    (_mutate_radix_share_out_of_range, "scenarios.s1_cancel.radix.share"),
    (_mutate_radix_share_nonnull_when_scheduler_zero, "scenarios.s1_cancel.radix"),
    (_mutate_requests_sum_mismatch, "scenarios.s1_cancel.requests"),
    (_mutate_by_generation_sum_mismatch, "scenarios.s1_cancel.gc.api_server.by_generation"),
    (_mutate_pause_ms_null_when_count_positive, "scenarios.s1_cancel.gc.api_server.pause_ms"),
    (_mutate_gil_held_pct_nan, "scenarios.s1_cancel.cpu.api_server.gil_held_pct"),
    (_mutate_delete_cpu_bucket, "scenarios.s1_cancel.cpu.scheduler.buckets.ipc_zmq"),
    (_mutate_processes_duplicate_pid, "scenarios.s1_cancel.processes"),
    (
        _mutate_top_alloc_sites_too_many,
        "scenarios.s1_cancel.memory.per_role.api_server.top_alloc_sites",
    ),
    (_mutate_coldstart_runs_mismatch, "scenarios.s3_coldstart.coldstart.hyperfine"),
]


@pytest.mark.parametrize(
    "mutate,expected_prefix", VIOLATION_CASES, ids=[c[1] for c in VIOLATION_CASES]
)
def test_schema_rejects_each_violation(mutate, expected_prefix):
    doc = make_valid_doc()
    mutate(doc)
    errors = validate_sidecar(doc, require_scenarios=SCENARIOS)
    assert any(e.startswith(expected_prefix) for e in errors), errors


# --- require_gpu ----------------------------------------------------------------------


def test_require_gpu():
    doc = make_valid_doc()
    doc["meta"]["platform"] = "linux"
    doc["meta"]["gpu"] = "NVIDIA H100 80GB HBM3"
    assert validate_sidecar(doc, require_scenarios=SCENARIOS, require_gpu=True) == []

    bad_platform = copy.deepcopy(doc)
    bad_platform["meta"]["platform"] = "darwin"
    errors = validate_sidecar(bad_platform, require_scenarios=SCENARIOS, require_gpu=True)
    assert any(e.startswith("meta") for e in errors), errors

    bad_gpu_none = copy.deepcopy(doc)
    bad_gpu_none["meta"]["gpu"] = None
    errors = validate_sidecar(bad_gpu_none, require_scenarios=SCENARIOS, require_gpu=True)
    assert any(e.startswith("meta") for e in errors), errors

    bad_gpu_empty = copy.deepcopy(doc)
    bad_gpu_empty["meta"]["gpu"] = ""
    errors = validate_sidecar(bad_gpu_empty, require_scenarios=SCENARIOS, require_gpu=True)
    assert any(e.startswith("meta") for e in errors), errors

    bad_mode = copy.deepcopy(doc)
    bad_mode["meta"]["mode"] = "discover"
    errors = validate_sidecar(bad_mode, require_scenarios=(), require_gpu=True)
    assert any(e.startswith("meta") for e in errors), errors


def test_write_sidecar_refuses_invalid(tmp_path):
    path = tmp_path / "out.json"

    doc_with_nan = make_valid_doc()
    doc_with_nan["scenarios"]["s1_cancel"]["cpu"]["api_server"]["gil_held_pct"] = float("nan")
    with pytest.raises(SidecarError):
        write_sidecar(doc_with_nan, path, require_scenarios=SCENARIOS)
    assert not path.exists()

    valid_doc = make_valid_doc()
    write_sidecar(valid_doc, path, require_scenarios=SCENARIOS)
    assert path.exists()
    assert json.loads(path.read_text()) == valid_doc


def test_write_sidecar_require_gpu(tmp_path):
    path = tmp_path / "out.json"
    darwin_doc = make_valid_doc()
    darwin_doc["meta"]["platform"] = "darwin"
    with pytest.raises(SidecarError):
        write_sidecar(darwin_doc, path, require_scenarios=SCENARIOS, require_gpu=True)
    assert not path.exists()


def test_build_meta_helpers_tolerate_subprocess_timeout(monkeypatch):
    # Code review CR-04: _git_commit/_git_dirty/_gpu_name caught only OSError
    # around subprocess.run(..., timeout=N); subprocess.TimeoutExpired is not
    # an OSError subclass, so a hung git/nvidia-smi call raised straight
    # through build_meta() -- which cmd_run calls only AFTER every requested
    # scenario has already run -- discarding a fully-measured session before
    # write_sidecar() was ever reached. Each helper must degrade to None
    # (or False for the boolean _git_dirty) instead of propagating.
    import subprocess

    from rsglang.profiling import sidecar

    def fake_run(*args, **kwargs):
        raise subprocess.TimeoutExpired(cmd=args[0] if args else "cmd", timeout=10)

    monkeypatch.setattr(subprocess, "run", fake_run)

    assert sidecar._git_commit(None) is None
    assert sidecar._git_dirty(None) is None
    assert sidecar._gpu_name() is None
