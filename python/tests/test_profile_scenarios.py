"""Tests for rsglang.profiling.scenarios (BENCH-01): the three benchmark
workload drivers -- scenario 1 (128 aiohttp agents with deterministic
cancellations), scenario 2 (bench_simple adaptation via lazy imports),
and scenario 3 (hyperfine cold-start helpers).

Scenario 1 and 3 are tested end to end against the 02-03 stand-in server
(rsglang.testing.fake_profile_env). Scenario 2's real import path needs
openai/transformers/minisgl.benchmark, which exist only on the GPU box;
its control flow is proven here with fakes injected into sys.modules.
"""

from __future__ import annotations

import asyncio
import json
import os
import random
import shlex
import socket
import sys
import time
import types
from pathlib import Path

import pytest

from rsglang.profiling import procs, scenarios

# --- shared helpers ------------------------------------------------------------


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _launch_stand_in(tmp_path: Path, *, extra_env: dict | None = None):
    port = _free_port()
    argv = procs.server_argv(
        "{python} -m rsglang.testing.fake_profile_env server --port {port}",
        python=sys.executable,
        model="fake/model",
        port=port,
    )
    env = dict(os.environ)
    if extra_env:
        env.update(extra_env)
    handle = procs.launch_server(argv, env=env, log_path=tmp_path / "server.log")
    procs.wait_ready(handle, port=port, timeout_s=30)
    return handle, f"http://127.0.0.1:{port}"


# --- Task 1: scenario 1 (128 aiohttp agents, deterministic cancellations) ------


def test_plan_agent_request_deterministic():
    rng1 = random.Random(42042)
    rng2 = random.Random(42042)
    seq1 = [
        scenarios.plan_agent_request(rng1, cancel_fraction=0.3, max_tokens=16, think_max_s=0.5)
        for _ in range(20)
    ]
    seq2 = [
        scenarios.plan_agent_request(rng2, cancel_fraction=0.3, max_tokens=16, think_max_s=0.5)
        for _ in range(20)
    ]
    assert seq1 == seq2

    rng_never = random.Random(1)
    for _ in range(200):
        think, cancel_after, prompt = scenarios.plan_agent_request(
            rng_never, cancel_fraction=0.0, max_tokens=16, think_max_s=0.5
        )
        assert cancel_after is None
        assert 0.0 <= think <= 0.5
        words = prompt.split()
        assert 16 <= len(words) <= 128
        assert prompt

    rng_always = random.Random(2)
    for _ in range(200):
        think, cancel_after, prompt = scenarios.plan_agent_request(
            rng_always, cancel_fraction=1.0, max_tokens=16, think_max_s=0.5
        )
        assert cancel_after is not None
        assert 0 <= cancel_after <= 16 // 2
        assert 0.0 <= think <= 0.5
        words = prompt.split()
        assert 16 <= len(words) <= 128


def test_make_session_unlimited():
    async def inner():
        session = scenarios.make_session()
        try:
            assert session.connector.limit == 0
            assert session.connector.limit_per_host == 0
        finally:
            await session.close()

    asyncio.run(inner())


@pytest.mark.slow
def test_s1_cancellation_semantics(tmp_path):
    handle, base_url = _launch_stand_in(tmp_path, extra_env={"RSGLANG_FAKE_TOKEN_DELAY_S": "0.01"})
    try:
        async def inner():
            session = scenarios.make_session()
            try:
                model = await scenarios.get_model_id(base_url)

                completed = await scenarios.stream_chat(
                    session, base_url, model=model, prompt="hello there", max_tokens=8, cancel_after=None
                )
                assert completed.outcome == "completed"
                assert completed.chunks >= 8
                assert completed.t_send < completed.t_first <= completed.t_end

                cancelled = await scenarios.stream_chat(
                    session, base_url, model=model, prompt="hello there", max_tokens=8, cancel_after=3
                )
                assert cancelled.outcome == "cancelled"
                assert cancelled.chunks == 3

                headers_only = await scenarios.stream_chat(
                    session, base_url, model=model, prompt="hello there", max_tokens=8, cancel_after=0
                )
                assert headers_only.outcome == "cancelled"
                assert headers_only.chunks == 0
                assert headers_only.t_first is None
            finally:
                await session.close()

        asyncio.run(inner())
    finally:
        procs.teardown(handle)


@pytest.mark.slow
def test_run_s1_concurrency(tmp_path):
    handle, base_url = _launch_stand_in(tmp_path)
    try:
        records = asyncio.run(
            scenarios.run_s1(
                base_url,
                agents=8,
                duration_s=2.0,
                cancel_fraction=0.5,
                max_tokens=16,
                think_max_s=0.05,
                seed=1,
            )
        )
        assert len(records) >= 8
        assert all(r.outcome in scenarios.OUTCOMES for r in records)
        failed = sum(1 for r in records if r.outcome == "failed")
        assert failed == 0
        completed = sum(1 for r in records if r.outcome == "completed")
        cancelled = sum(1 for r in records if r.outcome == "cancelled")
        assert completed + cancelled == len(records)
        for r in records:
            if r.t_first is not None:
                assert r.t_first >= r.t_send
    finally:
        procs.teardown(handle)


# --- Task 2: scenario 2 (bench_simple adaptation) and first-request workload --


def _install_s2_fakes(monkeypatch, *, benchmark_one, recorded_lengths=None):
    def fake_generate_prompt(tokenizer, n):
        if recorded_lengths is not None:
            recorded_lengths.append(n)
        return "p" * n

    async def fake_get_model_name(client):
        return "m"

    fake_client_module = types.ModuleType("minisgl.benchmark.client")
    fake_client_module.generate_prompt = fake_generate_prompt
    fake_client_module.benchmark_one = benchmark_one
    fake_client_module.get_model_name = fake_get_model_name

    class _FakeAsyncOpenAI:
        def __init__(self, *, base_url, api_key):
            self.base_url = base_url
            self.api_key = api_key

        async def __aenter__(self):
            return self

        async def __aexit__(self, *exc_info):
            return False

    fake_openai_module = types.ModuleType("openai")
    fake_openai_module.AsyncOpenAI = _FakeAsyncOpenAI

    class _FakeAutoTokenizer:
        @staticmethod
        def from_pretrained(model):
            return object()

    fake_transformers_module = types.ModuleType("transformers")
    fake_transformers_module.AutoTokenizer = _FakeAutoTokenizer

    monkeypatch.setitem(sys.modules, "minisgl.benchmark.client", fake_client_module)
    monkeypatch.setitem(sys.modules, "openai", fake_openai_module)
    monkeypatch.setitem(sys.modules, "transformers", fake_transformers_module)


def test_run_s2_with_injected_helpers(monkeypatch):
    class _RawResult:
        def __init__(self, tics):
            self.tics = tics

    async def fake_benchmark_one(client, prompt, output_length, model, *, pbar=False):
        t0 = time.perf_counter()
        await asyncio.sleep(0.001)
        return _RawResult([t0, t0 + 0.01, t0 + 0.02])

    recorded_lengths: list[int] = []
    _install_s2_fakes(monkeypatch, benchmark_one=fake_benchmark_one, recorded_lengths=recorded_lengths)

    records = asyncio.run(
        scenarios.run_s2("http://127.0.0.1:1", requests=40, max_input=32, output_tokens=32, seed=7)
    )
    assert len(records) == 40
    for r in records:
        assert r.outcome == "completed"
        assert r.t_send <= r.t_first

    first_lengths = list(recorded_lengths)
    assert len(first_lengths) == 40
    assert all(1 <= n <= 32 for n in first_lengths)

    recorded_lengths.clear()
    asyncio.run(
        scenarios.run_s2("http://127.0.0.1:1", requests=40, max_input=32, output_tokens=32, seed=7)
    )
    assert recorded_lengths == first_lengths


def test_run_s2_failure_recorded(monkeypatch):
    call_index = {"i": -1}

    async def fake_benchmark_one_raises_first(client, prompt, output_length, model, *, pbar=False):
        call_index["i"] += 1
        idx = call_index["i"]
        if idx == 0:
            raise RuntimeError("x")
        await asyncio.sleep(0.001)
        t0 = time.perf_counter()
        return type("RawResult", (), {"tics": [t0, t0 + 0.01, t0 + 0.02]})()

    _install_s2_fakes(monkeypatch, benchmark_one=fake_benchmark_one_raises_first)
    records = asyncio.run(
        scenarios.run_s2("http://127.0.0.1:1", requests=5, max_input=32, output_tokens=32, seed=11)
    )
    assert len(records) == 5
    failed = [r for r in records if r.outcome == "failed"]
    completed = [r for r in records if r.outcome == "completed"]
    assert len(failed) == 1
    assert failed[0].error == "x"
    assert len(completed) == 4

    async def fake_benchmark_one_single_tic(client, prompt, output_length, model, *, pbar=False):
        t0 = time.perf_counter()
        return type("RawResult", (), {"tics": [t0]})()

    monkeypatch.undo()
    _install_s2_fakes(monkeypatch, benchmark_one=fake_benchmark_one_single_tic)
    records2 = asyncio.run(
        scenarios.run_s2("http://127.0.0.1:1", requests=3, max_input=32, output_tokens=32, seed=12)
    )
    assert len(records2) == 3
    for r in records2:
        assert r.outcome == "failed"
        assert r.t_first is None


@pytest.mark.slow
def test_run_first_request(tmp_path):
    handle, base_url = _launch_stand_in(tmp_path)
    try:
        records = asyncio.run(scenarios.run_first_request(base_url, max_tokens=4))
        assert len(records) == 1
        assert records[0].outcome == "completed"
        assert records[0].t_first is not None
    finally:
        procs.teardown(handle)


# --- Task 3: scenario 3 helpers (hyperfine argv/JSON, coldstart_once/stop) ----


def test_hyperfine_argv():
    once_cmd = ["python", "driver.py", "once"]
    stop_cmd = ["python", "driver.py", "stop"]
    argv = scenarios.hyperfine_argv(
        hyperfine="hyperfine", runs=3, warmup=1, export_json="/x/h.json", once_cmd=once_cmd, stop_cmd=stop_cmd
    )
    assert argv == [
        "hyperfine",
        "--runs", "3",
        "--warmup", "1",
        "--export-json", "/x/h.json",
        "--conclude", shlex.join(stop_cmd),
        shlex.join(once_cmd),
    ]


def test_parse_hyperfine_json(tmp_path):
    path = tmp_path / "h.json"
    path.write_text(
        json.dumps(
            {
                "results": [
                    {
                        "command": "x",
                        "mean": 11.0,
                        "stddev": 1.0,
                        "median": 11.0,
                        "min": 10.0,
                        "max": 12.0,
                        "times": [10.0, 12.0, 11.0],
                    }
                ]
            }
        )
    )
    result = scenarios.parse_hyperfine_json(path)
    assert result == {
        "mean_s": 11.0,
        "stddev_s": 1.0,
        "median_s": 11.0,
        "min_s": 10.0,
        "max_s": 12.0,
        "times_s": [10.0, 12.0, 11.0],
        "runs": 3,
    }

    path2 = tmp_path / "h2.json"
    path2.write_text(
        json.dumps(
            {"results": [{"mean": 1.0, "stddev": None, "median": 1.0, "min": 1.0, "max": 1.0, "times": [1.0]}]}
        )
    )
    result2 = scenarios.parse_hyperfine_json(path2)
    assert result2["stddev_s"] is None

    path3 = tmp_path / "h3.json"
    path3.write_text(json.dumps({"no_results": []}))
    with pytest.raises(ValueError):
        scenarios.parse_hyperfine_json(path3)


def test_read_coldstart_records(tmp_path):
    path = tmp_path / "coldstart.jsonl"
    lines = [
        {"kind": "ready", "ready_s": 5.0},
        {"kind": "mem", "rss_tree_bytes": 100, "pss_tree_bytes": 10},
        {"kind": "ready", "ready_s": 4.0},
        {"kind": "mem", "rss_tree_bytes": 200, "pss_tree_bytes": 20},
        {"kind": "ready", "ready_s": 4.2},
        {"kind": "mem", "rss_tree_bytes": 300, "pss_tree_bytes": 30},
    ]
    path.write_text("\n".join(json.dumps(l) for l in lines) + "\n")
    result = scenarios.read_coldstart_records(path, warmup=1, runs=2)
    assert result == {
        "ready_s_self_timed": [4.0, 4.2],
        "rss_tree_bytes_at_ready": [200, 300],
        "pss_tree_bytes_at_ready": [20, 30],
    }


@pytest.mark.slow
def test_coldstart_once_then_stop(tmp_path):
    port = _free_port()
    pgid_file = tmp_path / "pgid"
    record_file = tmp_path / "record.jsonl"
    argv = procs.server_argv(
        "{python} -m rsglang.testing.fake_profile_env server --port {port}",
        python=sys.executable,
        model="fake/model",
        port=port,
    )

    try:
        rc = scenarios.coldstart_once(
            argv=argv,
            port=port,
            timeout_s=30,
            pgid_file=pgid_file,
            record_file=record_file,
            log_path=tmp_path / "s1.log",
        )
        assert rc == 0
        assert pgid_file.exists()
        pgid1 = int(pgid_file.read_text().strip())

        records = [json.loads(l) for l in record_file.read_text().splitlines() if l.strip()]
        assert len(records) == 1
        assert records[0]["kind"] == "ready"
        assert records[0]["ready_s"] > 0

        import urllib.request

        with urllib.request.urlopen(f"http://127.0.0.1:{port}/v1/models", timeout=5) as resp:
            assert resp.status == 200

        # Calling coldstart_once again without an intervening stop self-heals:
        # it stops pgid1 before launching a fresh group on the same port.
        rc2 = scenarios.coldstart_once(
            argv=argv,
            port=port,
            timeout_s=30,
            pgid_file=pgid_file,
            record_file=record_file,
            log_path=tmp_path / "s2.log",
        )
        assert rc2 == 0
        pgid2 = int(pgid_file.read_text().strip())
        assert pgid2 != pgid1
        assert _wait_until_dead({pgid1}) == set()

        rc3 = scenarios.coldstart_stop(pgid_file=pgid_file, record_file=record_file)
        assert rc3 == 0
        assert not pgid_file.exists()

        records2 = [json.loads(l) for l in record_file.read_text().splitlines() if l.strip()]
        mem_records = [r for r in records2 if r["kind"] == "mem"]
        assert len(mem_records) == 1
        mem = mem_records[0]
        assert isinstance(mem["rss_tree_bytes"], int) and mem["rss_tree_bytes"] > 0
        if sys.platform.startswith("linux"):
            assert isinstance(mem["pss_tree_bytes"], int)
        else:
            assert mem["pss_tree_bytes"] is None

        assert _wait_until_dead({pgid2}) == set()
    finally:
        if pgid_file.exists():
            scenarios.coldstart_stop(pgid_file=pgid_file, record_file=record_file)
