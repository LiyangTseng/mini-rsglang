"""Session orchestration, the one-at-a-time HTTP sweep, and the tap-to-request
join (PAR-01/PAR-02, D-04/Pattern 4): a fresh session per frontend, never two
requests in flight, joined against the backend tap's ground-truth ids.

aiohttp (already approved, Phase 2's scenario drivers use it) rather than the
openai SDK: the sweep sends non-streaming JSON requests, so no SSE parser is
needed, and aiohttp is already in the hashed Mac lock.
"""

from __future__ import annotations

import asyncio
import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Sequence

import aiohttp

from ..profiling import procs
from . import tap


@dataclass
class HttpResult:
    prompt_id: str
    status: "int | str"
    error: "str | None"
    text: "str | None"


@dataclass
class SessionResult:
    label: str
    value: Any
    tap: tap.TapRecords
    log_path: Path
    launcher_pid: int
    alive_after_teardown: "list[int]"


def run_session(
    label: str,
    *,
    argv: Sequence[str],
    port: int,
    timeout_s: float,
    work_dir: Path,
    workload: "Callable[[str], Awaitable[Any]]",
) -> SessionResult:
    """Launch argv, wait for /v1/models, run workload(base_url), then always
    tear down -- even if wait_ready or workload raises. Readiness failures
    propagate as procs.ServerExited or TimeoutError."""
    work_dir = Path(work_dir)
    tap_dir = work_dir / f"tap-{label}"
    env = tap.tap_env(os.environ, work_dir=work_dir, tap_dir=tap_dir)
    log_path = work_dir / f"{label}.log"

    handle = procs.launch_server(list(argv), env=env, log_path=log_path)
    value: Any = None
    alive_after_teardown: "list[int]" = []
    try:
        procs.wait_ready(handle, port=port, timeout_s=timeout_s)
        base_url = f"http://127.0.0.1:{port}"
        value = asyncio.run(workload(base_url))
    finally:
        alive_after_teardown = procs.teardown(handle, grace_s=60.0)

    records = tap.load_tap_records(tap_dir)
    return SessionResult(
        label=label,
        value=value,
        tap=records,
        log_path=log_path,
        launcher_pid=handle.proc.pid,
        alive_after_teardown=alive_after_teardown,
    )


async def send_sequential(
    base_url: str, model: str, items: "Sequence[Any]", timeout_s: float
) -> "list[HttpResult]":
    """POST each item to /v1/chat/completions in order, awaiting the full
    response body before sending the next -- never two requests in flight."""
    results: "list[HttpResult]" = []
    timeout = aiohttp.ClientTimeout(total=timeout_s)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        for item in items:
            payload: "dict[str, Any]" = {
                "model": model,
                "temperature": 0.0,
                "top_k": -1,
                "top_p": 1.0,
                "max_tokens": item.max_tokens,
                "stream": False,
            }
            if item.kind == "chat":
                payload["messages"] = item.messages
            else:
                payload["prompt"] = item.prompt

            try:
                async with session.post(f"{base_url}/v1/chat/completions", json=payload) as resp:
                    body_text = await resp.text()
                    if resp.status != 200:
                        results.append(
                            HttpResult(item.id, "error", f"HTTP {resp.status}: {body_text[:200]}", None)
                        )
                        continue
                    try:
                        body = json.loads(body_text)
                        text = body["choices"][0]["message"]["content"]
                    except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
                        results.append(HttpResult(item.id, "error", f"malformed body: {exc}", None))
                        continue
                    results.append(HttpResult(item.id, resp.status, None, text))
            except asyncio.TimeoutError:
                results.append(HttpResult(item.id, "error", "timeout", None))
            except aiohttp.ClientError as exc:
                results.append(HttpResult(item.id, "error", str(exc), None))
    return results


def _error_side(msg: str) -> dict:
    return {
        "status": "error",
        "error": msg,
        "uid": None,
        "input_ids": None,
        "sampling": None,
        "output_ids": None,
        "finished": None,
        "text": None,
    }


def join_sequential(
    items: "Sequence[Any]", http_results: "Sequence[HttpResult]", tap_records: tap.TapRecords
) -> "dict[str, dict]":
    """Join the kth user tap record to the kth corpus item, and that uid's
    detok records to its output_ids/finished. A global count/pid mismatch
    between the tap and the HTTP results gives every item an error side."""
    user_records = [r for r in tap_records.records if r["kind"] == tap.KIND_USER]
    detok_records = [r for r in tap_records.records if r["kind"] == tap.KIND_DETOK]

    pids = {r["pid"] for r in user_records}
    ok_count = sum(1 for r in http_results if r.status == 200)

    if len(pids) > 1:
        msg = f"user records come from multiple pids: {sorted(pids)}"
        return {item.id: _error_side(msg) for item in items}

    if len(user_records) != ok_count:
        msg = f"tap/request count mismatch: {len(user_records)} user records for {ok_count} requests"
        return {item.id: _error_side(msg) for item in items}

    detok_by_uid: "dict[int, list[dict]]" = {}
    for r in detok_records:
        detok_by_uid.setdefault(r["uid"], []).append(r)
    for recs in detok_by_uid.values():
        recs.sort(key=lambda r: r["seq"])

    http_by_id = {r.prompt_id: r for r in http_results}

    result: "dict[str, dict]" = {}
    for k, item in enumerate(items):
        user_record = user_records[k]
        uid = user_record["uid"]
        detoks = detok_by_uid.get(uid, [])
        output_ids = [d["next_token"] for d in detoks]
        finished = any(d.get("finished") for d in detoks)

        http_result = http_by_id.get(item.id)
        if http_result is None or http_result.status == "error":
            result[item.id] = {
                "status": "error",
                "error": http_result.error if http_result is not None else "no http result",
                "uid": uid,
                "input_ids": user_record["input_ids"],
                "sampling": user_record["sampling"],
                "output_ids": output_ids,
                "finished": finished,
                "text": None,
            }
        else:
            result[item.id] = {
                "status": "ok",
                "error": None,
                "uid": uid,
                "input_ids": user_record["input_ids"],
                "sampling": user_record["sampling"],
                "output_ids": output_ids,
                "finished": finished,
                "text": http_result.text,
            }
    return result
