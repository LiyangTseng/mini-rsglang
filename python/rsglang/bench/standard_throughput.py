"""BENCH-06 "standard inference" workload driver (D-12).

Per D-12, this module reuses vendor/mini-sglang/benchmark/online/bench_simple.py's
client-helper shape -- the same shape Phase 2 adapted for its Scenario 2 driver
(rsglang.profiling.scenarios.run_s2) -- instead of defining a new workload.
bench_simple.py hard-codes PORT 1919 and only logs its stats; this module takes
--port, computes the same statistics with upstream's own formulas
(minisgl.benchmark.client.process_benchmark_results) and writes them as JSON
for the Rust orchestrator to read as one A/B trial.

bench_simple interleaves its prompt-generation RNG draws with an asyncio task,
so their order depends on scheduling. This module draws in a fixed order: the
warm-up prompt, then the batch_size prompt lengths and prompts, then the
batch_size output lengths. That keeps the workload identical for both
frontends at a given seed, which is what the A/B comparison needs.

T-02-11: the base URL is always http://127.0.0.1:<port>/v1 -- this module has
no host-selecting flag, so load can never be pointed at a non-local host.

This module is imported on the Mac, where openai, transformers and the
minisgl.benchmark client dependencies are absent: those imports happen lazily
inside run(), following the rsglang.profiling.scenarios.run_s2 pattern.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import sys
import time
from pathlib import Path
from typing import Any, Awaitable, Callable

SCHEMA = "rsglang.bench.standard_throughput/1"

BATCH_SIZE = 64
MAX_INPUT = 8192
OUTPUT_MIN = 16
OUTPUT_MAX = 1024
SEED = 42
WARMUP_INPUT = 100
WARMUP_OUTPUT = 2

Runner = Callable[..., Awaitable[dict[str, Any]]]


def summarize(tics_lists: list[list[float]]) -> dict[str, Any]:
    """Reproduce minisgl.benchmark.client.process_benchmark_results's formulas.

    num_tokens = sum(len(tics)); duration = max(all tics) - min(all tics);
    throughput_tok_s = num_tokens / duration; throughput_req_s =
    num_requests / duration. Percentile indexing is upstream's own
    sorted[int(len * q)] (not nearest-rank interpolation). An empty list or a
    non-positive duration raises ValueError.
    """
    if not tics_lists:
        raise ValueError("tics_lists must not be empty")

    num_requests = len(tics_lists)
    num_tokens = sum(len(tics) for tics in tics_lists)
    all_tics = [t for tics in tics_lists for t in tics]
    duration = max(all_tics) - min(all_tics)
    if duration <= 0:
        raise ValueError("duration must be positive")

    first_times: list[float] = []
    accum_times: list[float] = []
    e2e_times: list[float] = []
    for tics in tics_lists:
        deltas = [tics[i + 1] - tics[i] for i in range(len(tics) - 1)]
        first_times.append(deltas[0])
        accum_times.extend(deltas[1:])
        e2e_times.append(tics[-1] - tics[0])
    first_times.sort()
    accum_times.sort()
    e2e_times.sort()

    def _stats(times: list[float], scale: float = 1.0) -> dict[str, float]:
        n = len(times)
        return {
            "avg": scale * sum(times) / n,
            "p50": scale * times[int(n * 0.5)],
            "p90": scale * times[int(n * 0.9)],
            "p99": scale * times[int(n * 0.99)],
            "max": scale * max(times),
        }

    return {
        "num_requests": num_requests,
        "num_tokens": num_tokens,
        "duration_s": duration,
        "throughput_tok_s": num_tokens / duration,
        "throughput_req_s": num_requests / duration,
        "ttft_ms": _stats(first_times, 1000.0),
        "tpot_ms": _stats(accum_times, 1000.0),
        "e2e_s": _stats(e2e_times, 1.0),
    }


async def run(
    port: int,
    *,
    seed: int = SEED,
    batch_size: int = BATCH_SIZE,
    max_input: int = MAX_INPUT,
) -> dict[str, Any]:
    """Run bench_simple.py's workload through upstream's own client helpers.

    Draw order is fixed (not interleaved with an asyncio task, unlike
    bench_simple.py itself): warm-up prompt, then batch_size prompt lengths
    and prompts, then batch_size output lengths.
    """
    # Lazy imports: openai/transformers/minisgl.benchmark exist on the GPU box
    # (upstream deps) but not in the Mac lock -- module import must not need
    # them (rsglang.profiling.scenarios.run_s2 pattern).
    from minisgl.benchmark.client import (
        benchmark_one,
        benchmark_one_batch,
        generate_prompt,
        get_model_name,
    )
    from openai import AsyncOpenAI

    from transformers import AutoTokenizer

    random.seed(seed)
    async with AsyncOpenAI(base_url=f"http://127.0.0.1:{port}/v1", api_key="dummy") as client:
        model = await get_model_name(client)
        tokenizer = AutoTokenizer.from_pretrained(model)

        warmup_prompt = generate_prompt(tokenizer, WARMUP_INPUT)
        warmup_result = await benchmark_one(client, warmup_prompt, WARMUP_OUTPUT, model, pbar=False)
        if len(warmup_result.tics) <= 2:
            raise RuntimeError(f"Server connection test failed on http://127.0.0.1:{port}")

        prompts = [generate_prompt(tokenizer, random.randint(1, max_input)) for _ in range(batch_size)]
        output_lengths = [random.randint(OUTPUT_MIN, OUTPUT_MAX) for _ in range(batch_size)]

        t_start_unix = time.time()
        results = await benchmark_one_batch(client, prompts, output_lengths, model, pbar=False)
        t_end_unix = time.time()

    return {
        "model": model,
        "t_start_unix": t_start_unix,
        "t_end_unix": t_end_unix,
        "tics": [r.tics for r in results],
    }


def main(argv: list[str] | None = None, *, runner: Runner | None = None) -> int:
    import asyncio

    parser = argparse.ArgumentParser(prog="python -m rsglang.bench.standard_throughput")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--seed", type=int, default=SEED)
    parser.add_argument("--batch-size", type=int, default=BATCH_SIZE)
    parser.add_argument("--max-input", type=int, default=MAX_INPUT)
    args = parser.parse_args(argv)

    out_path = Path(args.out)
    if out_path.is_symlink():
        print(f"{out_path}: refusing to write through a symlink", file=sys.stderr)
        return 1

    try:
        result = asyncio.run(
            (runner or run)(
                args.port,
                seed=args.seed,
                batch_size=args.batch_size,
                max_input=args.max_input,
            )
        )
        summary = summarize(result["tics"])
    except Exception as exc:  # noqa: BLE001 - any run/summarize failure is reported, not raised
        print(str(exc), file=sys.stderr)
        return 1

    doc = {
        "schema": SCHEMA,
        "params": {
            "seed": args.seed,
            "batch_size": args.batch_size,
            "max_input": args.max_input,
            "output_min": OUTPUT_MIN,
            "output_max": OUTPUT_MAX,
            "warmup_input": WARMUP_INPUT,
            "warmup_output": WARMUP_OUTPUT,
        },
        "model": result["model"],
        "t_start_unix": result["t_start_unix"],
        "t_end_unix": result["t_end_unix"],
        "summary": summary,
    }

    tmp_path = out_path.with_name(out_path.name + f".tmp-{os.getpid()}")
    try:
        with open(tmp_path, "x", encoding="utf-8") as f:
            json.dump(doc, f)
        os.replace(tmp_path, out_path)
    except OSError as exc:
        print(str(exc), file=sys.stderr)
        try:
            tmp_path.unlink()
        except OSError:
            pass
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
