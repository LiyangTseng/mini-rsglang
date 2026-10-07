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
import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Awaitable, Callable, Mapping, Sequence

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


def _build_payload(model: str, item: Any) -> "dict[str, Any]":
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
    return payload


def port_free(port: int) -> bool:
    """True iff 127.0.0.1:port can be bound right now (T-06-09)."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            sock.bind(("127.0.0.1", port))
        except OSError:
            return False
    return True


async def send_sequential(
    base_url: str, model: str, items: "Sequence[Any]", timeout_s: float
) -> "list[HttpResult]":
    """POST each item to /v1/chat/completions in order, awaiting the full
    response body before sending the next -- never two requests in flight."""
    results: "list[HttpResult]" = []
    timeout = aiohttp.ClientTimeout(total=timeout_s)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        for item in items:
            payload = _build_payload(model, item)

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


async def send_concurrent(
    base_url: str, model: str, items: "Sequence[Any]", concurrency: int, timeout_s: float
) -> "list[HttpResult]":
    """POST every item at once, bounded to `concurrency` requests in flight by
    both the connector's limit and an explicit semaphore. Results come back
    in item order -- asyncio.gather preserves the order of its awaitables
    regardless of completion order (PAR-02, D-10)."""
    timeout = aiohttp.ClientTimeout(total=timeout_s)
    connector = aiohttp.TCPConnector(limit=concurrency)
    semaphore = asyncio.Semaphore(concurrency)

    async def _one(session: aiohttp.ClientSession, item: Any) -> HttpResult:
        payload = _build_payload(model, item)
        async with semaphore:
            try:
                async with session.post(f"{base_url}/v1/chat/completions", json=payload) as resp:
                    body_text = await resp.text()
                    if resp.status != 200:
                        return HttpResult(
                            item.id, "error", f"HTTP {resp.status}: {body_text[:200]}", None
                        )
                    try:
                        body = json.loads(body_text)
                        text = body["choices"][0]["message"]["content"]
                    except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
                        return HttpResult(item.id, "error", f"malformed body: {exc}", None)
                    return HttpResult(item.id, resp.status, None, text)
            except asyncio.TimeoutError:
                return HttpResult(item.id, "error", "timeout", None)
            except aiohttp.ClientError as exc:
                return HttpResult(item.id, "error", str(exc), None)

    async with aiohttp.ClientSession(timeout=timeout, connector=connector) as session:
        results = await asyncio.gather(*(_one(session, item) for item in items))
    return list(results)


def join_by_input_ids(
    items: "Sequence[Any]",
    http_results: "Sequence[HttpResult]",
    tap_records: tap.TapRecords,
    expected_input_ids: "Mapping[str, list]",
) -> "tuple[dict[str, dict], int]":
    """Join user tap records to corpus prompts by matching input_ids against
    expected_input_ids -- built from the SAME frontend's own sequential sides
    for the gate model, never cross-frontend. Unlike join_sequential, this
    makes no ordering assumption between the tap and the HTTP results: under
    concurrent load requests can be serviced in any order.

    Returns (sides, unmatched_tap). A user record whose input_ids match no
    expected prompt increments unmatched_tap instead of being silently
    dropped. A prompt with no matching user record gets status error
    "no tap user record".
    """
    ids_to_prompt: "dict[tuple, str]" = {}
    for prompt_id, ids in expected_input_ids.items():
        key = tuple(ids)
        if key in ids_to_prompt:
            raise ValueError(f"ambiguous input_ids for prompts {ids_to_prompt[key]}, {prompt_id}")
        ids_to_prompt[key] = prompt_id

    user_records = [r for r in tap_records.records if r["kind"] == tap.KIND_USER]
    detok_records = [r for r in tap_records.records if r["kind"] == tap.KIND_DETOK]

    detok_by_uid: "dict[int, list[dict]]" = {}
    for r in detok_records:
        detok_by_uid.setdefault(r["uid"], []).append(r)
    for recs in detok_by_uid.values():
        recs.sort(key=lambda r: r["seq"])

    http_by_id = {r.prompt_id: r for r in http_results}

    sides_by_prompt: "dict[str, dict]" = {}
    unmatched_tap = 0
    for user_record in user_records:
        key = tuple(user_record["input_ids"])
        prompt_id = ids_to_prompt.get(key)
        if prompt_id is None:
            unmatched_tap += 1
            continue

        uid = user_record["uid"]
        detoks = detok_by_uid.get(uid, [])
        output_ids = [d["next_token"] for d in detoks]
        finished = any(d.get("finished") for d in detoks)

        http_result = http_by_id.get(prompt_id)
        if http_result is None or http_result.status == "error":
            sides_by_prompt[prompt_id] = {
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
            sides_by_prompt[prompt_id] = {
                "status": "ok",
                "error": None,
                "uid": uid,
                "input_ids": user_record["input_ids"],
                "sampling": user_record["sampling"],
                "output_ids": output_ids,
                "finished": finished,
                "text": http_result.text,
            }

    result: "dict[str, dict]" = {}
    for item in items:
        result[item.id] = sides_by_prompt.get(item.id) or _error_side("no tap user record")

    return result, unmatched_tap
