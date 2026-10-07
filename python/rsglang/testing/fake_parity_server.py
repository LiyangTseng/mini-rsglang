"""Mac-only fake parity server (PAR-01/PAR-02): a deterministic stand-in for
the real `--frontend {python,rust}` server, driven by scripts/parity_check.py.

Never used by production runs: the GPU box runs the real server behind
either frontend. This module exists purely so the Mac tracer tests can prove
the sweep/tap-join/compare/sidecar path -- including the abort-stress part's
watcher/tap-evidence path (06-05) -- end to end without GPU access.

Run as:
    python -m rsglang.testing.fake_parity_server server --port P --model M \
        [--diverge-when SUBSTR --diverge-output-at K] \
        [--emit-scheduler-child --abort-timing V --double-free-on-abort]

Or, as the fake 128-agent stress driver (06-05 Task 1):
    python -m rsglang.testing.fake_parity_server stress --base-url URL \
        --requests N --abort-fraction F --seed S
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
import random
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Sequence

from rsglang.parity import tap

_MAX_OUTPUT_TOKENS = 8


class _TapWriter:
    """One append-mode handle per process, guarded by a lock -- mirrors the
    real tap's writer discipline so load_tap_records sees identical framing.
    """

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._path: "Path | None" = None
        self._seq = 0
        self._pid = os.getpid()

    def _ensure_open(self) -> "Path | None":
        tap_dir = os.environ.get(tap.TAP_DIR_ENV)
        if not tap_dir:
            return None
        if self._path is None:
            Path(tap_dir).mkdir(parents=True, exist_ok=True)
            self._path = Path(tap_dir) / f"tap-{self._pid}-{time.perf_counter_ns()}.jsonl"
        return self._path

    def write(self, kind: str, **fields) -> None:
        with self._lock:
            path = self._ensure_open()
            if path is None:
                return
            record = {"kind": kind, "pid": self._pid, "seq": self._seq, "t": time.monotonic()}
            record.update(fields)
            self._seq += 1
            with open(path, "a", encoding="utf-8") as fh:
                fh.write(json.dumps(record) + "\n")


_tap_writer = _TapWriter()
_uid_lock = threading.Lock()
_uid_counter = 0

_in_flight_lock = threading.Lock()
_in_flight = 0


def _next_uid() -> int:
    global _uid_counter
    with _uid_lock:
        uid = _uid_counter
        _uid_counter += 1
        return uid


def _enter_in_flight() -> int:
    """Increments the in-flight counter and returns the count *before* this
    request was added, so --diverge-under-load can compare against the
    caller-specified threshold."""
    global _in_flight
    with _in_flight_lock:
        before = _in_flight
        _in_flight += 1
        return before


def _exit_in_flight() -> None:
    global _in_flight
    with _in_flight_lock:
        _in_flight -= 1


def _token_delay_s() -> float:
    try:
        return float(os.environ.get("RSGLANG_FAKE_PARITY_TOKEN_DELAY_S", "0.002"))
    except ValueError:
        return 0.002


def _render_prompt(payload: dict) -> str:
    messages = payload.get("messages")
    if messages:
        rendered = "".join(f"[{m.get('role')}] {m.get('content')}\n" for m in messages)
        return rendered + "[reply]"
    return payload.get("prompt") or ""


def _deterministic_output(
    rendered_prompt: str,
    max_tokens: int,
    *,
    diverge_when: "str | None",
    diverge_output_at: "int | None",
) -> "tuple[list[int], list[int], str]":
    input_ids = list(rendered_prompt.encode("utf-8"))
    total = sum(input_ids)
    n = min(max_tokens, _MAX_OUTPUT_TOKENS)
    output_ids = [((total + 7 * k) % 251) + 1 for k in range(n)]
    if diverge_when and diverge_when in rendered_prompt:
        if diverge_output_at is not None and 0 <= diverge_output_at < len(output_ids):
            output_ids[diverge_output_at] += 1
    text = "".join(chr(0x61 + (i % 26)) for i in output_ids)
    return input_ids, output_ids, text


def _make_handler(
    model_id: str,
    diverge_when: "str | None",
    diverge_output_at: "int | None",
    diverge_under_load: "int | None" = None,
    flavor: str = "python",
    double_free_on_abort: bool = False,
):
    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):  # noqa: A002 - matches base signature
            pass

        def _send_json(self, status: int, obj: dict) -> None:
            body = json.dumps(obj).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _send_text(self, status: int, body: bytes, content_type: str) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _send_404(self) -> None:
            self.send_response(404)
            self.end_headers()

        def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler naming
            if self.path == "/v1/models":
                self._send_json(
                    200, {"object": "list", "data": [{"id": model_id, "object": "model"}]}
                )
                return
            if self.path == "/v1":
                self._send_json(200, {"status": "ok"})
                return
            if flavor == "rust" and self.path in ("/health", "/health/ready"):
                self._send_json(200, {"status": "ok"})
                return
            if flavor == "rust" and self.path == "/metrics":
                body = b"# TYPE rsg_requests_total counter\nrsg_requests_total 0\n"
                self._send_text(200, body, "text/plain; version=0.0.4")
                return
            self._send_404()

        def _read_json_body(self) -> dict:
            length = int(self.headers.get("Content-Length", 0) or 0)
            raw = self.rfile.read(length) if length else b""
            try:
                return json.loads(raw) if raw else {}
            except json.JSONDecodeError:
                return {}

        def _compute_output(self, rendered: str, max_tokens: int, in_flight_before: int):
            input_ids, output_ids, text = _deterministic_output(
                rendered,
                max_tokens,
                diverge_when=diverge_when,
                diverge_output_at=diverge_output_at,
            )
            if diverge_under_load is not None and in_flight_before >= diverge_under_load:
                output_ids = list(output_ids)
                output_ids[0] += 1
                text = "".join(chr(0x61 + (i % 26)) for i in output_ids)
            return input_ids, output_ids, text

        def _write_free(self, uid: int, *, dup: bool) -> None:
            _tap_writer.write(tap.KIND_FREE, uid=uid, table_idx=uid % 16, dup_free_slots=dup)

        def _write_abort_and_free(self, uid: int) -> None:
            _tap_writer.write(tap.KIND_ABORT, uid=uid, in_pending=False, in_running=True, chunked=False)
            self._write_free(uid, dup=False)
            if double_free_on_abort:
                self._write_free(uid, dup=True)

        def _handle_chat(self) -> None:
            payload = self._read_json_body()
            stream = bool(payload.get("stream", False))
            max_tokens = int(payload.get("max_tokens") or 1)
            sampling = {
                "temperature": float(payload.get("temperature", 0.0)),
                "top_k": int(payload.get("top_k", -1)),
                "top_p": float(payload.get("top_p", 1.0)),
                "ignore_eos": bool(payload.get("ignore_eos", False)),
                "max_tokens": max_tokens,
            }
            rendered = _render_prompt(payload)

            in_flight_before = _enter_in_flight()
            try:
                input_ids, output_ids, text = self._compute_output(rendered, max_tokens, in_flight_before)

                uid = _next_uid()
                _tap_writer.write(tap.KIND_USER, uid=uid, input_ids=input_ids, sampling=sampling)

                delay = _token_delay_s()
                if stream:
                    try:
                        self.send_response(200)
                        self.send_header("Content-Type", "text/event-stream")
                        self.end_headers()
                        first = True
                        for k, token in enumerate(output_ids):
                            if delay:
                                time.sleep(delay)
                            finished = k == len(output_ids) - 1
                            _tap_writer.write(tap.KIND_DETOK, uid=uid, next_token=token, finished=finished)
                            delta = {}
                            if first:
                                delta["role"] = "assistant"
                                first = False
                            delta["content"] = chr(0x61 + (token % 26))
                            chunk = {
                                "id": f"chatcmpl-{uid}",
                                "object": "chat.completion.chunk",
                                "choices": [{"index": 0, "delta": delta, "finish_reason": None}],
                            }
                            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
                        end_chunk = {
                            "id": f"chatcmpl-{uid}",
                            "object": "chat.completion.chunk",
                            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                        }
                        self.wfile.write(f"data: {json.dumps(end_chunk)}\n\n".encode())
                        self.wfile.write(b"data: [DONE]\n\n")
                        self.close_connection = True
                    except (BrokenPipeError, ConnectionResetError):
                        self._write_abort_and_free(uid)
                        return
                    self._write_free(uid, dup=False)
                else:
                    try:
                        for k, token in enumerate(output_ids):
                            if delay:
                                time.sleep(delay)
                            finished = k == len(output_ids) - 1
                            _tap_writer.write(tap.KIND_DETOK, uid=uid, next_token=token, finished=finished)
                        self._send_json(
                            200,
                            {
                                "id": f"chatcmpl-{uid}",
                                "object": "chat.completion",
                                "created": int(time.time()),
                                "model": model_id,
                                "choices": [
                                    {
                                        "index": 0,
                                        "message": {"role": "assistant", "content": text},
                                        "finish_reason": "stop",
                                    }
                                ],
                                "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
                            },
                        )
                    except (BrokenPipeError, ConnectionResetError):
                        self._write_abort_and_free(uid)
                        return
                    self._write_free(uid, dup=False)
            finally:
                _exit_in_flight()

        def _handle_generate(self) -> None:
            payload = self._read_json_body()
            max_tokens = int(payload.get("max_tokens") or 1)
            sampling = {
                "temperature": 0.0,
                "top_k": -1,
                "top_p": 1.0,
                "ignore_eos": bool(payload.get("ignore_eos", False)),
                "max_tokens": max_tokens,
            }
            rendered = _render_prompt(payload)

            in_flight_before = _enter_in_flight()
            try:
                input_ids, output_ids, _text = self._compute_output(rendered, max_tokens, in_flight_before)

                uid = _next_uid()
                _tap_writer.write(tap.KIND_USER, uid=uid, input_ids=input_ids, sampling=sampling)

                delay = _token_delay_s()
                try:
                    self.send_response(200)
                    self.send_header("Content-Type", "text/event-stream")
                    self.end_headers()
                    for k, token in enumerate(output_ids):
                        if delay:
                            time.sleep(delay)
                        finished = k == len(output_ids) - 1
                        _tap_writer.write(tap.KIND_DETOK, uid=uid, next_token=token, finished=finished)
                        incremental = chr(0x61 + (token % 26))
                        self.wfile.write(f"data: {incremental}\n".encode())
                    self.wfile.write(b"data: [DONE]\n")
                    self.close_connection = True
                except (BrokenPipeError, ConnectionResetError):
                    self._write_abort_and_free(uid)
                    return
                self._write_free(uid, dup=False)
            finally:
                _exit_in_flight()

        def do_POST(self):  # noqa: N802
            if self.path == "/v1/chat/completions":
                self._handle_chat()
                return
            if self.path == "/generate":
                self._handle_generate()
                return
            self._send_404()

    return Handler


class _Server(http.server.ThreadingHTTPServer):
    """ThreadingHTTPServer's default request_queue_size (5, from
    socketserver.BaseServer) is smaller than PAR-02's 128-wide concurrent
    sweep; under real concurrent load the OS silently drops/refuses the
    overflow, so a handful of requests never reach do_POST and their tap
    user records never get written -- producing spurious "no tap user
    record" joins unrelated to the frontends being compared. A generous
    fixed backlog (well above any --concurrency this phase uses) avoids
    that entirely."""

    request_queue_size = 256
    daemon_threads = True


def _build_server_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="fake_parity_server server", add_help=False)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--diverge-when", default=None)
    parser.add_argument("--diverge-output-at", type=int, default=None)
    parser.add_argument("--diverge-under-load", type=int, default=None)
    parser.add_argument("--flavor", choices=("python", "rust"), default="python")
    parser.add_argument("--gated", action="store_true")
    parser.add_argument("--gated-models", default=None, metavar="LIST")
    parser.add_argument("--emit-scheduler-child", action="store_true")
    parser.add_argument("--abort-timing", default=None, metavar="TIMING")
    parser.add_argument("--double-free-on-abort", action="store_true")
    return parser


def _spawn_scheduler_child() -> "subprocess.Popen | None":
    """Spawns a long-sleeping child process and prints the stress part's
    scheduler-pid discovery line (stress._SCHEDULER_PID_RE) to this
    process's own stderr -- so the stress part's watcher has a real pid to
    watch without needing a real scheduler (06-05 Task 1)."""
    child = subprocess.Popen(
        [
            sys.executable,
            "-c",
            "import time,sys; sys.argv[0]='fake-parity-scheduler'; time.sleep(3600)",
            "fake-parity-scheduler",
        ],
    )
    print(f"rsglang.launch: spawned scheduler rank=0 pid={child.pid}", file=sys.stderr, flush=True)
    return child


def _cmd_server(argv: Sequence[str]) -> int:
    ns = _build_server_parser().parse_args(argv)

    gated_models = {m.strip() for m in (ns.gated_models or "").split(",") if m.strip()}
    if ns.gated or ns.model in gated_models:
        print(f"Cannot access gated repo for url https://huggingface.co/{ns.model}", file=sys.stderr)
        return 1

    if ns.abort_timing:
        print(f"abort_timing={ns.abort_timing}", file=sys.stderr, flush=True)

    scheduler_child = _spawn_scheduler_child() if ns.emit_scheduler_child else None

    _tap_writer.write(
        tap.KIND_PATCHED,
        targets=[
            "Scheduler._process_one_msg",
            "Scheduler._free_req_resources",
            "Scheduler._prepare_batch",
            "SchedulerIOMixin._reply_tokenizer_rank0",
        ],
    )

    handler_cls = _make_handler(
        ns.model,
        ns.diverge_when,
        ns.diverge_output_at,
        ns.diverge_under_load,
        ns.flavor,
        ns.double_free_on_abort,
    )
    httpd = _Server(("127.0.0.1", ns.port), handler_cls)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.shutdown()
        httpd.server_close()
        if scheduler_child is not None:
            try:
                scheduler_child.kill()
                scheduler_child.wait(timeout=5.0)
            except Exception:
                pass
    return 0


def _build_stress_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="fake_parity_server stress", add_help=False)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--requests", type=int, required=True)
    parser.add_argument("--abort-fraction", type=float, default=0.0)
    parser.add_argument("--seed", type=int, default=0)
    return parser


async def _run_stress_driver(ns: argparse.Namespace) -> int:
    import aiohttp

    rng = random.Random(ns.seed)
    abort_flags = [rng.random() < ns.abort_fraction for _ in range(ns.requests)]
    aborted_count = 0

    async def _one(session: "aiohttp.ClientSession", i: int) -> None:
        nonlocal aborted_count
        payload = {
            "model": "fake/model",
            "messages": [{"role": "user", "content": f"stress agent {i}"}],
            "temperature": 0.0,
            "max_tokens": 8,
            "stream": True,
        }
        should_abort = abort_flags[i]
        try:
            async with session.post(f"{ns.base_url}/v1/chat/completions", json=payload) as resp:
                if should_abort:
                    async for raw_line in resp.content:
                        if raw_line.strip():
                            aborted_count += 1
                            break
                    resp.close()
                else:
                    async for _raw_line in resp.content:
                        pass
        except Exception:
            pass

    timeout = aiohttp.ClientTimeout(total=120.0)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        import asyncio

        await asyncio.gather(*(_one(session, i) for i in range(ns.requests)))

    print(f"fake stress: sent {ns.requests}, aborted {aborted_count}")
    return 0


def _cmd_stress(argv: Sequence[str]) -> int:
    import asyncio

    ns = _build_stress_parser().parse_args(argv)
    return asyncio.run(_run_stress_driver(ns))


def main(argv: "Sequence[str] | None" = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        print("usage: fake_parity_server {server|stress} ...", file=sys.stderr)
        return 2
    command, rest = argv[0], argv[1:]
    if command == "server":
        return _cmd_server(rest)
    if command == "stress":
        return _cmd_stress(rest)
    print(f"fake_parity_server: unknown command {command!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
