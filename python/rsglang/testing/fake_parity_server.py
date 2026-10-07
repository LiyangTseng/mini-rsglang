"""Mac-only fake parity server (PAR-01/PAR-02): a deterministic stand-in for
the real `--frontend {python,rust}` server, driven by scripts/parity_check.py.

Never used by production runs: the GPU box runs the real server behind
either frontend. This module exists purely so the Mac tracer tests can prove
the sweep/tap-join/compare/sidecar path end to end without GPU access.

Run as:
    python -m rsglang.testing.fake_parity_server server --port P --model M \
        [--diverge-when SUBSTR --diverge-output-at K]
"""

from __future__ import annotations

import argparse
import http.server
import json
import os
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


def _next_uid() -> int:
    global _uid_counter
    with _uid_lock:
        uid = _uid_counter
        _uid_counter += 1
        return uid


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


def _make_handler(model_id: str, diverge_when: "str | None", diverge_output_at: "int | None"):
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

        def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler naming
            if self.path == "/v1/models":
                self._send_json(
                    200, {"object": "list", "data": [{"id": model_id, "object": "model"}]}
                )
                return
            self.send_response(404)
            self.end_headers()

        def do_POST(self):  # noqa: N802
            if self.path != "/v1/chat/completions":
                self.send_response(404)
                self.end_headers()
                return
            length = int(self.headers.get("Content-Length", 0) or 0)
            raw = self.rfile.read(length) if length else b""
            try:
                payload = json.loads(raw) if raw else {}
            except json.JSONDecodeError:
                payload = {}

            max_tokens = int(payload.get("max_tokens") or 1)
            sampling = {
                "temperature": float(payload.get("temperature", 0.0)),
                "top_k": int(payload.get("top_k", -1)),
                "top_p": float(payload.get("top_p", 1.0)),
                "ignore_eos": bool(payload.get("ignore_eos", False)),
                "max_tokens": max_tokens,
            }

            rendered = _render_prompt(payload)
            input_ids, output_ids, text = _deterministic_output(
                rendered,
                max_tokens,
                diverge_when=diverge_when,
                diverge_output_at=diverge_output_at,
            )

            uid = _next_uid()
            _tap_writer.write(tap.KIND_USER, uid=uid, input_ids=input_ids, sampling=sampling)
            for k, token in enumerate(output_ids):
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

    return Handler


def _build_server_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="fake_parity_server server", add_help=False)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--diverge-when", default=None)
    parser.add_argument("--diverge-output-at", type=int, default=None)
    return parser


def _cmd_server(argv: Sequence[str]) -> int:
    ns = _build_server_parser().parse_args(argv)

    _tap_writer.write(
        tap.KIND_PATCHED,
        targets=[
            "Scheduler._process_one_msg",
            "Scheduler._free_req_resources",
            "Scheduler._prepare_batch",
            "SchedulerIOMixin._reply_tokenizer_rank0",
        ],
    )

    handler_cls = _make_handler(ns.model, ns.diverge_when, ns.diverge_output_at)
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", ns.port), handler_cls)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.shutdown()
        httpd.server_close()
    return 0


def main(argv: "Sequence[str] | None" = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        print("usage: fake_parity_server {server} ...", file=sys.stderr)
        return 2
    command, rest = argv[0], argv[1:]
    if command == "server":
        return _cmd_server(rest)
    print(f"fake_parity_server: unknown command {command!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
