#!/usr/bin/env python3
"""Fixture runner for `throughput_runner.rs`'s tracer test (Task 1, D-12).

Plugs into `rsglang.bench.standard_throughput.main`'s injected-runner seam
(the same seam 07-03's own test suite uses) with a runner that needs none of
the GPU-box-only dependencies (`openai`/`transformers`/`minisgl.benchmark`):
it only reads the model id from `GET /v1/models` (a `bench-stub` arm) via
`urllib` (stdlib), then fabricates deterministic per-request tics scaled by
that model id so the tracer can assert the Rust arm (model id `slow`) always
reports a lower `throughput_tok_s` than the Python arm (model id `fast`).
The JSON this writes still goes through `st.main`'s own real writer, so its
schema is exactly `rsglang.bench.standard_throughput/1`.
"""

from __future__ import annotations

import json
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

# Defensive fallback: the Rust runner always sets PYTHONPATH to include
# `<repo>/python` before spawning this script, but inserting it here too
# keeps the fixture independently runnable (e.g. by hand, for debugging).
_REPO_PYTHON_DIR = Path(__file__).resolve().parents[4] / "python"
if str(_REPO_PYTHON_DIR) not in sys.path:
    sys.path.insert(0, str(_REPO_PYTHON_DIR))

import rsglang.bench.standard_throughput as st  # noqa: E402

NUM_REQUESTS = 8
TICS_PER_REQUEST = 10
GAP_S = 0.01


def _fetch_model_id(port: int) -> str:
    url = f"http://127.0.0.1:{port}/v1/models"
    with urllib.request.urlopen(url, timeout=10) as resp:  # noqa: S310 - loopback only
        body = json.loads(resp.read().decode("utf-8"))
    return body["data"][0]["id"]


async def fake_runner(port: int, *, seed: int, batch_size: int, max_input: int) -> dict[str, Any]:
    del seed, batch_size, max_input  # unused: this fixture's workload is fixed

    model = _fetch_model_id(port)
    scale = 1.0 if model == "fast" else 1.25

    t_start_unix = time.time()
    t = t_start_unix
    tics: list[list[float]] = []
    for _ in range(NUM_REQUESTS):
        request_tics: list[float] = []
        for _ in range(TICS_PER_REQUEST):
            t += GAP_S * scale
            request_tics.append(t)
        tics.append(request_tics)
    t_end_unix = t

    return {
        "model": model,
        "t_start_unix": t_start_unix,
        "t_end_unix": t_end_unix,
        "tics": tics,
    }


if __name__ == "__main__":
    sys.exit(st.main(sys.argv[1:], runner=fake_runner))
