"""Workload drivers for BENCH-01's three benchmark scenarios (D-05):

- Scenario 1 (D-06): 128 concurrent asyncio/aiohttp agents against
  /v1/chat/completions with stream=True, and seeded per-agent cancellation
  decisions, including headers-only (prefill) aborts.
- Scenario 2 (D-07): a 32-token short-prompt saturation adaptation of
  vendor/mini-sglang/benchmark/online/bench_simple.py's own client helpers.
- Scenario 3 (D-08): hyperfine cold-start timing to readiness (not teardown),
  with whole-tree RSS/PSS sampled outside the timed region.

All three emit RequestRecord instances on the shared time.perf_counter() clock.
This module is imported on the Mac, where openai/transformers/minisgl.benchmark
are not installed -- those are imported lazily, inside run_s2 only.

T-02-11: every function here sends load only to a caller-supplied base_url.
Callers must always build it as http://127.0.0.1:<port> -- this module has no
host-selecting flag, so load can never be pointed at a non-local host.
"""

from __future__ import annotations

import asyncio
import random
import time
from dataclasses import dataclass

import aiohttp

OUTCOMES = ("completed", "cancelled", "failed")

WORDS = (
    "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
    "a", "an", "and", "or", "but", "if", "then", "else",
    "cat", "sparrow", "whale", "fish", "run", "jump", "walk", "talk",
    "red", "blue", "green", "yellow", "big", "small", "fast", "slow",
    "happy", "sad", "angry", "calm", "bright", "dark", "light", "heavy",
    "water", "fire", "earth", "air", "tree", "flower", "river", "mountain",
    "book", "pen", "paper", "table", "chair", "door", "window", "wall",
    "time", "space", "mind", "body", "soul", "heart", "hand", "eye",
)


@dataclass(frozen=True)
class RequestRecord:
    t_send: float
    t_first: float | None
    t_end: float
    chunks: int
    outcome: str
    error: str | None = None


# --- Scenario 1: 128 aiohttp agents with deterministic cancellations (D-06) ---


def make_prompt(rng: random.Random) -> str:
    n = rng.randint(16, 128)
    return " ".join(rng.choice(WORDS) for _ in range(n))


def plan_agent_request(
    rng: random.Random, *, cancel_fraction: float, max_tokens: int, think_max_s: float
) -> tuple[float, int | None, str]:
    think = rng.uniform(0, think_max_s)
    cancel = rng.random() < cancel_fraction
    cancel_after = rng.randint(0, max_tokens // 2) if cancel else None
    prompt = make_prompt(rng)
    return think, cancel_after, prompt


def make_session() -> aiohttp.ClientSession:
    # The default connector limit of 100 would make agents 101-128 queue
    # inside the client itself and inflate their TTFT -- disable both caps.
    connector = aiohttp.TCPConnector(limit=0, limit_per_host=0)
    timeout = aiohttp.ClientTimeout(total=None, sock_read=300)
    return aiohttp.ClientSession(connector=connector, timeout=timeout)


async def get_model_id(base_url: str) -> str:
    async with aiohttp.ClientSession() as session:
        async with session.get(f"{base_url}/v1/models") as resp:
            data = await resp.json()
    return data["data"][0]["id"]


async def stream_chat(
    session: aiohttp.ClientSession,
    base_url: str,
    *,
    model: str,
    prompt: str,
    max_tokens: int,
    cancel_after: int | None,
) -> RequestRecord:
    body = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "stream": True,
        "ignore_eos": True,
    }
    t_send = time.perf_counter()
    try:
        resp = await session.post(f"{base_url}/v1/chat/completions", json=body)
    except (aiohttp.ClientError, asyncio.TimeoutError) as exc:
        return RequestRecord(
            t_send=t_send, t_first=None, t_end=time.perf_counter(), chunks=0, outcome="failed", error=str(exc)
        )

    t_first: float | None = None
    chunks = 0
    outcome = "failed"
    error: str | None = None
    try:
        if resp.status != 200:
            error = f"status {resp.status}"
        elif cancel_after == 0:
            # Close right after the headers, before reading any chunk --
            # models a queued/prefill abort.
            outcome = "cancelled"
        else:
            outcome = "completed"
            async for line in resp.content:
                text = line.decode("utf-8", errors="replace").strip()
                if not text.startswith("data: "):
                    continue
                payload = text[len("data: "):]
                if payload == "[DONE]":
                    outcome = "completed"
                    break
                if t_first is None:
                    t_first = time.perf_counter()
                chunks += 1
                if cancel_after is not None and chunks >= cancel_after:
                    outcome = "cancelled"
                    break
    except (aiohttp.ClientError, asyncio.TimeoutError) as exc:
        outcome = "failed"
        error = str(exc)
    finally:
        resp.close()
    t_end = time.perf_counter()
    return RequestRecord(t_send=t_send, t_first=t_first, t_end=t_end, chunks=chunks, outcome=outcome, error=error)


async def run_s1(
    base_url: str,
    *,
    agents: int = 128,
    duration_s: float = 120.0,
    cancel_fraction: float = 0.25,
    max_tokens: int = 256,
    think_max_s: float = 0.5,
    seed: int = 42,
) -> list[RequestRecord]:
    model = await get_model_id(base_url)
    session = make_session()
    try:
        deadline = time.perf_counter() + duration_s

        async def agent_loop(i: int) -> list[RequestRecord]:
            rng = random.Random(seed * 1000 + i)
            records: list[RequestRecord] = []
            while time.perf_counter() < deadline:
                think, cancel_after, prompt = plan_agent_request(
                    rng, cancel_fraction=cancel_fraction, max_tokens=max_tokens, think_max_s=think_max_s
                )
                await asyncio.sleep(think)
                record = await stream_chat(
                    session, base_url, model=model, prompt=prompt, max_tokens=max_tokens, cancel_after=cancel_after
                )
                records.append(record)
            return records

        results = await asyncio.gather(*(agent_loop(i) for i in range(agents)))
        return [record for agent_records in results for record in agent_records]
    finally:
        await session.close()


# --- Scenario 2: bench_simple adaptation (D-07) and first-request workload ---


async def run_s2(
    base_url: str,
    *,
    requests: int = 512,
    max_input: int = 32,
    output_tokens: int = 32,
    seed: int = 42,
) -> list[RequestRecord]:
    # Lazy imports: openai/transformers/minisgl.benchmark exist on the GPU box
    # (upstream deps) but not in the Mac lock -- module import must not need them.
    from minisgl.benchmark.client import benchmark_one, generate_prompt, get_model_name
    from openai import AsyncOpenAI
    from transformers import AutoTokenizer

    random.seed(seed)
    async with AsyncOpenAI(base_url=f"{base_url}/v1", api_key="dummy") as client:
        model = await get_model_name(client)
        tokenizer = AutoTokenizer.from_pretrained(model)

        lengths = [random.randint(1, max_input) for _ in range(requests)]
        prompts = [generate_prompt(tokenizer, n) for n in lengths]

        async def _wrap(prompt: str) -> RequestRecord:
            # t_send, not tics[0]: tics[0] is taken after the response headers,
            # so TTFT measured from it would hide the frontend's request-accept
            # latency, which is exactly what BENCH-01 profiles.
            t_send = time.perf_counter()
            try:
                result = await benchmark_one(client, prompt, output_tokens, model, pbar=False)
                tics = result.tics
                t_first = tics[1] if len(tics) > 1 else None
                t_end = tics[-1]
                chunks = len(tics) - 1
                outcome = "completed" if t_first is not None else "failed"
                error = None
            except Exception as exc:  # noqa: BLE001 - any benchmark_one failure is a recorded outcome
                t_first = None
                t_end = time.perf_counter()
                chunks = 0
                outcome = "failed"
                error = str(exc)
            return RequestRecord(
                t_send=t_send, t_first=t_first, t_end=t_end, chunks=chunks, outcome=outcome, error=error
            )

        return list(await asyncio.gather(*(_wrap(p) for p in prompts)))


async def run_first_request(base_url: str, *, max_tokens: int = 16) -> list[RequestRecord]:
    model = await get_model_id(base_url)
    session = make_session()
    try:
        record = await stream_chat(
            session, base_url, model=model, prompt="Say hello.", max_tokens=max_tokens, cancel_after=None
        )
        return [record]
    finally:
        await session.close()
