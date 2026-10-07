"""Phase 6's own external-target cancellation stress driver.

Checkpoint-resolved scope change (06-06-SUMMARY.md): Phase 5's
`crates/rsg-server/tests/stress_128.rs` is a `#[tokio::test]` cargo
integration test with no CLI, no binary, and no way to point it at an
already-running server by base URL -- its own correctness assertions
(registry snapshot, mock-observation ordering) only exist because it starts
everything itself, in-process. 06-CONTEXT.md's D-11 says to reuse that tool
"as-is" against the real backend; since it has no external-target mode at
all, and D-11 forbids changing it, that is impossible without either
rewriting it (no longer "as-is") or giving the test binary a new CLI surface
it doesn't have today. The user resolved this (06-06-SUMMARY.md's
checkpoint) by choosing to build a new, purpose-built tool instead: this
module.

This is NOT stress_128.rs and makes no attempt to reproduce its in-process
assertions -- those remain Phase 5's own correctness proof, unaffected by
this file. This module's only job is the external-targeting capability
Phase 6 Task 2 needs: drive real concurrent HTTP traffic, with real TCP
cancellation, against an already-running frontend (`--base-url`), and report
a simple pass/fail plus counts, mirroring stress_128.rs's cancellation-stress
*intent* (D-04's "no leaked requests, no stuck connections, exactly one
terminal state") from outside the server rather than from in-process state
an external client cannot reach.

Usage:
    python -m rsglang.parity.stress_client --base-url URL --requests N \
        [--abort-fraction F] [--seed S] [--model M] [--max-tokens N] \
        [--request-timeout-s S] [--canary-timeout-s S]

Exit codes: 0 pass (no request timed out or errored, and a final canary
request after the storm still succeeds), 1 fail, 2 usage error.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import random
import sys
from typing import Any, Sequence


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m rsglang.parity.stress_client", allow_abbrev=False)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--requests", type=int, required=True)
    parser.add_argument("--abort-fraction", type=float, default=0.3)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--max-tokens", type=int, default=8)
    parser.add_argument("--model", default="fake/model")
    parser.add_argument("--request-timeout-s", type=float, default=60.0)
    parser.add_argument("--canary-timeout-s", type=float, default=30.0)
    return parser


async def _one(
    session: Any,
    *,
    base_url: str,
    i: int,
    should_abort: bool,
    max_tokens: int,
    model: str,
    timeout_s: float,
    counters: "dict[str, int]",
) -> None:
    import aiohttp

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": f"stress agent {i}"}],
        "temperature": 0.0,
        "max_tokens": max_tokens,
        "stream": True,
    }
    try:
        async with session.post(
            f"{base_url}/v1/chat/completions",
            json=payload,
            timeout=aiohttp.ClientTimeout(total=timeout_s),
        ) as resp:
            counters["sent"] += 1
            if resp.status != 200:
                counters["bad_status"] += 1
                return
            if should_abort:
                async for raw_line in resp.content:
                    if raw_line.strip():
                        counters["aborted"] += 1
                        break
                resp.close()
            else:
                async for _raw_line in resp.content:
                    pass
                counters["completed"] += 1
    except asyncio.TimeoutError:
        counters["timed_out"] += 1
    except Exception:  # noqa: BLE001 - any client/connection error counts as a failure
        counters["errored"] += 1


async def _canary(base_url: str, model: str, timeout_s: float) -> bool:
    import aiohttp

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": "canary"}],
        "temperature": 0.0,
        "max_tokens": 4,
        "stream": False,
    }
    try:
        timeout = aiohttp.ClientTimeout(total=timeout_s)
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(f"{base_url}/v1/chat/completions", json=payload) as resp:
                if resp.status != 200:
                    return False
                body = json.loads(await resp.text())
                return bool(body["choices"][0]["message"]["content"])
    except Exception:  # noqa: BLE001 - any failure means "not alive"
        return False


async def _run(ns: argparse.Namespace) -> int:
    import aiohttp

    rng = random.Random(ns.seed)
    abort_flags = [rng.random() < ns.abort_fraction for _ in range(ns.requests)]
    counters = {
        "sent": 0,
        "completed": 0,
        "aborted": 0,
        "timed_out": 0,
        "errored": 0,
        "bad_status": 0,
    }

    timeout = aiohttp.ClientTimeout(total=ns.request_timeout_s + 10.0)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        await asyncio.gather(
            *(
                _one(
                    session,
                    base_url=ns.base_url,
                    i=i,
                    should_abort=abort_flags[i],
                    max_tokens=ns.max_tokens,
                    model=ns.model,
                    timeout_s=ns.request_timeout_s,
                    counters=counters,
                )
                for i in range(ns.requests)
            )
        )

    canary_ok = await _canary(ns.base_url, ns.model, ns.canary_timeout_s)

    print(
        "stress_client: "
        f"requests={ns.requests} sent={counters['sent']} completed={counters['completed']} "
        f"aborted={counters['aborted']} timed_out={counters['timed_out']} "
        f"errored={counters['errored']} bad_status={counters['bad_status']} "
        f"canary_ok={canary_ok}"
    )

    ok = counters["timed_out"] == 0 and counters["errored"] == 0 and canary_ok
    return 0 if ok else 1


def main(argv: "Sequence[str] | None" = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.requests < 1:
        print("--requests must be >= 1", file=sys.stderr)
        return 2
    if not (0.0 <= ns.abort_fraction <= 1.0):
        print("--abort-fraction must be in [0, 1]", file=sys.stderr)
        return 2
    return asyncio.run(_run(ns))


if __name__ == "__main__":
    sys.exit(main())
