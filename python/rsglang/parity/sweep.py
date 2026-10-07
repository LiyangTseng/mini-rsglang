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

# Every frontend answers these (ROADMAP criterion 1 / Phase 5 criteria 1, 5).
# The Rust frontend additionally answers /health, /health/ready and /metrics
# -- the upstream Python frontend has no equivalent (06-RESEARCH.md).
PYTHON_ENDPOINTS = (
    "GET /v1/models",
    "GET /v1",
    "POST /v1/chat/completions",
    "POST /v1/chat/completions stream",
    "POST /generate",
)
RUST_ENDPOINTS = PYTHON_ENDPOINTS + ("GET /health", "GET /health/ready", "GET /metrics")


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


async def _read_lines(resp: aiohttp.ClientResponse) -> "list[str]":
    lines: "list[str]" = []
    async for raw_line in resp.content:
        lines.append(raw_line.decode("utf-8", errors="replace").rstrip("\n").rstrip("\r"))
    return lines


def _framing_ok(lines: "list[str]", *, data_prefix: str) -> "tuple[bool, str]":
    nonempty = [line for line in lines if line.strip()]
    has_data_line = any(line.startswith(data_prefix) for line in nonempty)
    last_is_done = bool(nonempty) and nonempty[-1] == "data: [DONE]"
    ok = has_data_line and last_is_done
    detail = "" if ok else f"framing check failed: has_data_line={has_data_line} last_is_done={last_is_done}"
    return ok, detail


async def endpoint_smoke(base_url: str, frontend: str, model: str) -> "list[dict]":
    """Checks every endpoint the given frontend is supposed to serve (D-07,
    ROADMAP criterion 1). Each entry is {name, ok, status, detail}. Never
    raises -- a connection failure becomes one entry with ok=False."""
    results: "list[dict]" = []
    timeout = aiohttp.ClientTimeout(total=120.0)

    async def _get(session: aiohttp.ClientSession, name: str, path: str, check) -> None:
        try:
            async with session.get(f"{base_url}{path}") as resp:
                ok, status, detail = await check(resp)
        except (asyncio.TimeoutError, aiohttp.ClientError) as exc:
            ok, status, detail = False, "error", str(exc)
        results.append({"name": name, "ok": ok, "status": status, "detail": detail})

    async def _post(session: aiohttp.ClientSession, name: str, path: str, body: dict, check) -> None:
        try:
            async with session.post(f"{base_url}{path}", json=body) as resp:
                ok, status, detail = await check(resp)
        except (asyncio.TimeoutError, aiohttp.ClientError) as exc:
            ok, status, detail = False, "error", str(exc)
        results.append({"name": name, "ok": ok, "status": status, "detail": detail})

    async def _check_models(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        try:
            data = json.loads(await resp.text())
            model_id = data["data"][0]["id"]
        except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
            return False, resp.status, f"malformed body: {exc}"
        if model_id != model:
            return False, resp.status, f"model id {model_id!r} != {model!r}"
        return True, resp.status, ""

    async def _check_v1_root(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        try:
            data = json.loads(await resp.text())
        except json.JSONDecodeError as exc:
            return False, resp.status, f"malformed body: {exc}"
        if data != {"status": "ok"}:
            return False, resp.status, f"body {data!r} != {{'status': 'ok'}}"
        return True, resp.status, ""

    async def _check_chat_nonstream(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        try:
            data = json.loads(await resp.text())
            content = data["choices"][0]["message"]["content"]
        except (json.JSONDecodeError, KeyError, IndexError, TypeError) as exc:
            return False, resp.status, f"malformed body: {exc}"
        if not (isinstance(content, str) and content):
            return False, resp.status, f"content {content!r} is not a non-empty str"
        return True, resp.status, ""

    async def _check_chat_stream(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        lines = await _read_lines(resp)
        ok, detail = _framing_ok(lines, data_prefix="data: {")
        return ok, resp.status, detail

    async def _check_generate(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        lines = await _read_lines(resp)
        ok, detail = _framing_ok(lines, data_prefix="data: ")
        return ok, resp.status, detail

    async def _check_simple_200(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        ok = resp.status == 200
        return ok, resp.status, ("" if ok else f"HTTP {resp.status}")

    async def _check_metrics(resp: aiohttp.ClientResponse) -> "tuple[bool, Any, str]":
        if resp.status != 200:
            return False, resp.status, f"HTTP {resp.status}"
        body = await resp.text()
        ok = "# TYPE" in body
        return ok, resp.status, ("" if ok else "missing '# TYPE' in body")

    chat_body = {
        "model": model,
        "messages": [{"role": "user", "content": "Say hello."}],
        "temperature": 0.0,
        "max_tokens": 8,
        "stream": False,
    }
    generate_body = {"prompt": "Hello", "max_tokens": 8}

    async with aiohttp.ClientSession(timeout=timeout) as session:
        await _get(session, "GET /v1/models", "/v1/models", _check_models)
        await _get(session, "GET /v1", "/v1", _check_v1_root)
        await _post(session, "POST /v1/chat/completions", "/v1/chat/completions", chat_body, _check_chat_nonstream)
        await _post(
            session,
            "POST /v1/chat/completions stream",
            "/v1/chat/completions",
            dict(chat_body, stream=True),
            _check_chat_stream,
        )
        await _post(session, "POST /generate", "/generate", generate_body, _check_generate)

        if frontend == "rust":
            await _get(session, "GET /health", "/health", _check_simple_200)
            await _get(session, "GET /health/ready", "/health/ready", _check_simple_200)
            await _get(session, "GET /metrics", "/metrics", _check_metrics)

    return results
