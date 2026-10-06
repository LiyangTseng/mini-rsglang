"""Mac-only stand-ins for the real server process tree and the real py-spy CLI.

Never used by production runs: the GPU box runs the real
`python -m rsglang.launch --frontend python` server and the real py-spy binary.
`server` mimics rsglang.launch's exec hop, the default topology's spawn of one
scheduler child and one combined tokenize+detokenize child, and upstream's
start_backend-before-uvicorn.run ordering, plus a minimal /v1/models and
/v1/chat/completions HTTP surface. `py-spy` mimics the real py-spy CLI's
--version, dump and record closely enough for rsglang.profiling.procs's role
identification and (in later plans) radix-share bucketing.

Run as:
    python -m rsglang.testing.fake_profile_env server --port P [--no-exec-hop]
    python -m rsglang.testing.fake_profile_env py-spy <py-spy args>

All side effects live under `if __name__ == "__main__":` because spawned
children re-import this module as __mp_main__ (same convention as
rsglang.launch).
"""

from __future__ import annotations

import argparse
import gc
import http.server
import json
import multiprocessing as mp
import os
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Sequence

ROLE_MAP_ENV = "RSGLANG_FAKE_PROFILE_ROLE_MAP"
PYSPY_MODE_ENV = "RSGLANG_FAKE_PYSPY_MODE"
FAKE_MODEL_ID = "fake/model"
FAKE_RADIX_SAMPLES = 3
FAKE_ACTIVE_SAMPLES = 10
FAKE_GIL_SAMPLES = 4

_PREFILL_DELAY_ENV = "RSGLANG_FAKE_PREFILL_DELAY_S"
_TOKEN_DELAY_ENV = "RSGLANG_FAKE_TOKEN_DELAY_S"
_DEFAULT_PREFILL_DELAY_S = 0.02
_DEFAULT_TOKEN_DELAY_S = 0.002

_ACK_TIMEOUT_S = 60.0
_JOIN_TIMEOUT_S = 5.0

_PERMISSION_DENIED_MSG = "Permission Denied: Try running again with elevated permissions"


# --- spawn targets (module-level, per the plan's exact names) ------------------


def _busywork() -> None:
    """About 1000 small cyclic list objects, built and dropped, then collected."""
    garbage = []
    for _ in range(1000):
        node = []
        node.append(node)
        garbage.append(node)
    del garbage
    gc.collect()


def _run_scheduler(ack: "mp.Queue", parent_pid: int) -> None:
    ack.put(("scheduler", os.getpid()))
    try:
        while os.getppid() == parent_pid:
            _busywork()
            time.sleep(0.2)
    except KeyboardInterrupt:
        return


def tokenize_worker(ack: "mp.Queue", parent_pid: int) -> None:
    ack.put(("tokenizer", os.getpid()))
    try:
        while os.getppid() == parent_pid:
            _busywork()
            time.sleep(0.2)
    except KeyboardInterrupt:
        return


# --- server subcommand ----------------------------------------------------------


def _make_handler(model_id: str):
    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.0"

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
                    200,
                    {
                        "object": "list",
                        "data": [{"id": model_id, "object": "model", "root": model_id}],
                    },
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
            stream = bool(payload.get("stream"))
            max_tokens = int(payload.get("max_tokens") or 1)
            uid = "fake"
            if stream:
                self._stream_chat(uid, max_tokens)
            else:
                full_content = "tok " * max_tokens
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
                                "message": {"role": "assistant", "content": full_content},
                                "finish_reason": "stop",
                            }
                        ],
                        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
                    },
                )

        def _stream_chat(self, uid: str, max_tokens: int) -> None:
            prefill_delay = float(os.environ.get(_PREFILL_DELAY_ENV, _DEFAULT_PREFILL_DELAY_S))
            token_delay = float(os.environ.get(_TOKEN_DELAY_ENV, _DEFAULT_TOKEN_DELAY_S))
            try:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                time.sleep(prefill_delay)
                first = True
                for _ in range(max_tokens):
                    delta = {"role": "assistant", "content": "tok "} if first else {"content": "tok "}
                    first = False
                    chunk = {
                        "id": f"cmpl-{uid}",
                        "object": "text_completion.chunk",
                        "choices": [{"delta": delta, "index": 0, "finish_reason": None}],
                    }
                    self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode("utf-8"))
                    self.wfile.flush()
                    time.sleep(token_delay)
                end_chunk = {
                    "id": f"cmpl-{uid}",
                    "object": "text_completion.chunk",
                    "choices": [{"delta": {}, "index": 0, "finish_reason": "stop"}],
                }
                self.wfile.write(f"data: {json.dumps(end_chunk)}\n\n".encode("utf-8"))
                self.wfile.flush()
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError):
                return

    return Handler


def _build_server_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="fake_profile_env server", add_help=False)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--no-exec-hop", action="store_true")
    return parser


def _wait_for_acks(ack_queue: "mp.Queue", timeout_s: float) -> dict:
    roles: dict = {}
    deadline = time.monotonic() + timeout_s
    while len(roles) < 2:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("fake_profile_env: timed out waiting for child acks")
        role, pid = ack_queue.get(timeout=remaining)
        roles[role] = pid
    return roles


def _cmd_server(argv: Sequence[str]) -> int:
    ns = _build_server_parser().parse_args(argv)

    if not ns.no_exec_hop:
        os.execv(
            sys.executable,
            [
                sys.executable,
                "-m",
                "rsglang.testing.fake_profile_env",
                "server",
                "--port",
                str(ns.port),
                "--no-exec-hop",
            ],
        )

    top_pid = os.getpid()
    mp.set_start_method("spawn", force=True)
    ack_queue: "mp.Queue" = mp.Queue()
    sched_proc = mp.Process(target=_run_scheduler, args=(ack_queue, top_pid), daemon=False, name="fake-scheduler")
    tok_proc = mp.Process(target=tokenize_worker, args=(ack_queue, top_pid), daemon=False, name="fake-tokenizer")
    sched_proc.start()
    tok_proc.start()

    try:
        roles = _wait_for_acks(ack_queue, _ACK_TIMEOUT_S)
    except TimeoutError:
        for proc in (sched_proc, tok_proc):
            proc.terminate()
        raise

    role_map_path = os.environ.get(ROLE_MAP_ENV)
    if role_map_path:
        doc = {
            str(top_pid): "api_server",
            str(roles["scheduler"]): "scheduler",
            str(roles["tokenizer"]): "tokenizer",
        }
        Path(role_map_path).write_text(json.dumps(doc))

    handler_cls = _make_handler(FAKE_MODEL_ID)
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", ns.port), handler_cls)

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.shutdown()
        httpd.server_close()
        for proc in (sched_proc, tok_proc):
            proc.join(timeout=_JOIN_TIMEOUT_S)
            if proc.is_alive():
                proc.terminate()

    return 0


# --- py-spy subcommand -----------------------------------------------------------

_ROLE_BASE_FRAME = {
    "scheduler": {"name": "_run_scheduler", "file": "rsglang/testing/fake_profile_env.py", "line": 1, "col": 1},
    "tokenizer": {"name": "tokenize_worker", "file": "rsglang/testing/fake_profile_env.py", "line": 1, "col": 1},
    "api_server": {"name": "serve_forever", "file": "socketserver.py", "line": 1, "col": 1},
    "other": {"name": "main", "file": "multiprocessing/resource_tracker.py", "line": 1, "col": 1},
}
_RADIX_FRAME = {"name": "match_prefix", "file": "/x/minisgl/kvcache/radix_cache.py", "line": 1, "col": 1}
_PUT_FRAME = {"name": "put", "file": "/x/minisgl/utils/mp.py", "line": 1, "col": 1}
_SERIALIZE_FRAME = {"name": "serialize_type", "file": "/x/minisgl/message/utils.py", "line": 1, "col": 1}
_TOKENIZE_FRAME = {"name": "tokenize", "file": "/x/minisgl/tokenizer/tokenize.py", "line": 1, "col": 1}


def _role_for_pid(pid: int) -> str:
    path = os.environ.get(ROLE_MAP_ENV)
    if not path or not os.path.exists(path):
        return "other"
    try:
        doc = json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError):
        return "other"
    return doc.get(str(pid), "other")


def _dump_text(pid: int) -> str:
    role = _role_for_pid(pid)
    frame = _ROLE_BASE_FRAME.get(role, _ROLE_BASE_FRAME["other"])
    return (
        f"Process {pid}: fake\n"
        'Thread 0x1 (active): "MainThread"\n'
        f'    {frame["name"]} ({frame["file"]}:{frame["line"]})\n'
    )


def _frames_for_role(role: str, gil: bool) -> list:
    base = _ROLE_BASE_FRAME.get(role, _ROLE_BASE_FRAME["other"])
    if gil:
        return [[base] for _ in range(FAKE_GIL_SAMPLES)]

    specials: list = []
    if role == "scheduler":
        specials = [[_RADIX_FRAME]] * FAKE_RADIX_SAMPLES
    elif role in ("api_server", "tokenizer"):
        specials = [[_PUT_FRAME]] * 2 + [[_SERIALIZE_FRAME]] * 1
        if role == "tokenizer":
            specials += [[_TOKENIZE_FRAME]] * 2

    n_plain = max(0, FAKE_ACTIVE_SAMPLES - len(specials))
    stacks = specials + [[] for _ in range(n_plain)]
    return [[base, *extra] for extra in stacks]


def _stacks_to_speedscope(stacks: list) -> tuple:
    frames: list = []
    frame_index: dict = {}
    samples: list = []
    for stack in stacks:
        idxs = []
        for frame in stack:
            key = (frame["name"], frame["file"], frame["line"], frame.get("col", 1))
            if key not in frame_index:
                frame_index[key] = len(frames)
                frames.append(
                    {"name": frame["name"], "file": frame["file"], "line": frame["line"], "col": frame.get("col", 1)}
                )
            idxs.append(frame_index[key])
        samples.append(idxs)
    return frames, samples


def _build_pyspy_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="py-spy", add_help=False)
    parser.add_argument("--version", action="store_true")
    sub = parser.add_subparsers(dest="pyspy_command")

    dump_p = sub.add_parser("dump", add_help=False)
    dump_p.add_argument("--pid", type=int, required=True)
    dump_p.add_argument("--nonblocking", action="store_true")

    record_p = sub.add_parser("record", add_help=False)
    record_p.add_argument("--pid", type=int, required=True)
    record_p.add_argument("--rate", type=int, default=100)
    record_p.add_argument("--format", default="raw")
    record_p.add_argument("--output", required=True)
    record_p.add_argument("--nonblocking", action="store_true")
    record_p.add_argument("--gil", action="store_true")

    return parser


def _cmd_record(ns: argparse.Namespace) -> int:
    stacks = _frames_for_role(_role_for_pid(ns.pid), ns.gil)
    frames, samples = _stacks_to_speedscope(stacks)
    rate = ns.rate if ns.rate > 0 else 100
    weight = 1.0 / rate
    doc = {
        "$schema": "https://www.speedscope.app/file-format-schema.json",
        "shared": {"frames": frames},
        "profiles": [
            {
                "type": "sampled",
                "unit": "seconds",
                "name": f"pid {ns.pid}",
                "startValue": 0,
                "endValue": len(samples) * weight,
                "samples": samples,
                "weights": [weight] * len(samples),
            }
        ],
        "exporter": "py-spy@fake",
    }

    stop = threading.Event()

    def _stop(signum, frame):  # noqa: ANN001
        stop.set()

    import signal

    signal.signal(signal.SIGINT, _stop)
    signal.signal(signal.SIGTERM, _stop)
    stop.wait()

    out_path = Path(ns.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(doc))
    return 0


def _cmd_pyspy(argv: Sequence[str]) -> int:
    mode = os.environ.get(PYSPY_MODE_ENV)
    if mode == "denied":
        print(_PERMISSION_DENIED_MSG, file=sys.stderr)
        return 1

    ns = _build_pyspy_parser().parse_args(argv)
    if ns.version:
        print("py-spy 0.4.2-fake")
        return 0
    if ns.pyspy_command == "dump":
        print(_dump_text(ns.pid))
        return 0
    if ns.pyspy_command == "record":
        return _cmd_record(ns)
    print("py-spy: expected --version, dump or record", file=sys.stderr)
    return 1


# --- hyperfine subcommand ---------------------------------------------------------


def _build_hyperfine_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="hyperfine", add_help=False)
    parser.add_argument("--version", action="store_true")
    parser.add_argument("--runs", type=int, default=10)
    parser.add_argument("--warmup", type=int, default=0)
    parser.add_argument("--export-json", default=None)
    parser.add_argument("--conclude", default=None)
    parser.add_argument("cmd", nargs="?", default=None)
    return parser


def _cmd_hyperfine(argv: Sequence[str]) -> int:
    ns = _build_hyperfine_parser().parse_args(argv)
    if ns.version:
        print("hyperfine 1.20.0")
        return 0

    if ns.cmd is None:
        print("hyperfine: missing <command>", file=sys.stderr)
        return 1

    total = ns.warmup + ns.runs
    times: list = []
    exit_codes: list = []
    for i in range(total):
        t0 = time.perf_counter()
        result = subprocess.run(ns.cmd, shell=True)
        duration = time.perf_counter() - t0
        rc = result.returncode

        if ns.conclude:
            subprocess.run(ns.conclude, shell=True)

        if rc != 0:
            return 1

        if i >= ns.warmup:
            times.append(duration)
            exit_codes.append(rc)

    if ns.export_json:
        n = len(times)
        doc = {
            "results": [
                {
                    "command": ns.cmd,
                    "mean": sum(times) / n if n else 0.0,
                    "stddev": statistics.stdev(times) if n > 1 else None,
                    "median": statistics.median(times) if n else 0.0,
                    "min": min(times) if n else 0.0,
                    "max": max(times) if n else 0.0,
                    "times": times,
                    "exit_codes": exit_codes,
                }
            ]
        }
        export_path = Path(ns.export_json)
        export_path.parent.mkdir(parents=True, exist_ok=True)
        export_path.write_text(json.dumps(doc))

    return 0


# --- entry point -------------------------------------------------------------------


def main(argv: Sequence[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv:
        print("usage: fake_profile_env {server|py-spy|hyperfine} ...", file=sys.stderr)
        return 2
    command, rest = argv[0], argv[1:]
    if command == "server":
        return _cmd_server(rest)
    if command == "py-spy":
        return _cmd_pyspy(rest)
    if command == "hyperfine":
        return _cmd_hyperfine(rest)
    print(f"fake_profile_env: unknown command {command!r}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
