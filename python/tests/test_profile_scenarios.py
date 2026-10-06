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
