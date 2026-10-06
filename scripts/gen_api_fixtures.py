#!/usr/bin/env python3
"""Capture the API-01 golden fixture set from a live run of upstream's frozen Python frontend
against the mock-scheduler (D-02).

Every fixture is the status code, content-type and (for `compare: "bytes"` cases) the
normalized response body bytes of one of 18 fixed requests, issued one at a time, in a fixed
order, against a single fresh run of `rsglang.testing.python_frontend` (Plan 05-05 Task 1)
backed by `target/debug/mock-scheduler`. The only normalization applied is the `created`
timestamp field, replaced with a fixed `0` after checking it is within one day of capture time.

Usage:
  scripts/gen_api_fixtures.py            write fixtures/api/*.body and manifest.json
  scripts/gen_api_fixtures.py --out DIR  write them to DIR instead
  scripts/gen_api_fixtures.py --check    regenerate into a temp dir and byte-diff with fixtures/api

Exit codes: 0 ok, 1 fixtures differ (--check), 2 environment error.
The case table must stay in step with crates/rsg-server/tests/api_parity.rs (plan 05-09).
"""

from __future__ import annotations

import argparse
import dataclasses
import http.client
import json
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Dict, List, Optional, Tuple

REPO = Path(__file__).resolve().parent.parent
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"
FIXTURES_DIR = REPO / "fixtures" / "api"
SHA_FILE = REPO / "vendor" / "UPSTREAM_SHA"

MODEL = "Qwen/Qwen3-0.6B"
MOCK_ARGS = ["--prefill-delay-ms", "0", "--decode-delay-ms", "0", "--max-seq-len", "4096"]
CHECKED_MANIFEST_KEYS = ("model", "mock_args", "upstream_sha", "cases")
# Every normalized case, mapped to its normalized field names. Only "created" is ever
# normalized (ModelCard.created / the hardcoded non-streaming chat response's created).
NORMALIZED_FIELDS: Dict[str, Tuple[str, ...]] = {
    "models_list": ("created",),
    "chat_nonstream_multibyte": ("created",),
    "chat_nonstream_eos_final": ("created",),
    "chat_nonstream_prompt_text": ("created",),
}

_HANDSHAKE_DEADLINE_S = 20.0
_READY_DEADLINE_S = 120.0
_TEARDOWN_GRACE_S = 10.0
_CREATED_RE = re.compile(rb'"created":(\d+)')
_MAX_CREATED_SKEW_S = 86400


class EnvError(Exception):
    pass


class _EosMaxTokens:
    """Placeholder for chat_nonstream_eos_final's N (max_tokens), computed at capture time
    from the tokenizer's chat template and eos_token_id -- not a static value."""

    def __repr__(self) -> str:
        return "<computed at capture time>"


_EOS_MAX_TOKENS = _EosMaxTokens()

# The 18-case table, in fixed capture order (D-02). uid-consuming cases get 0..9 with no
# gaps: the 3 status-only cases (validation/assertion errors) are proven to consume no uid
# by chat_stream_after_errors landing on uid 9 right after them.
CASES: List[Dict] = [
    {"name": "models_list", "method": "GET", "path": "/v1/models", "body": None,
     "compare": "bytes", "uid": None},
    {"name": "v1_get", "method": "GET", "path": "/v1", "body": None,
     "compare": "bytes", "uid": None},
    {"name": "v1_post", "method": "POST", "path": "/v1", "body": None,
     "compare": "bytes", "uid": None},
    {"name": "v1_head", "method": "HEAD", "path": "/v1", "body": None,
     "compare": "bytes", "uid": None},
    {"name": "v1_options", "method": "OPTIONS", "path": "/v1", "body": None,
     "compare": "bytes", "uid": None},
    {"name": "generate_ascii", "method": "POST", "path": "/generate",
     "body": {"prompt": "The quick brown fox jumps over the lazy dog.", "max_tokens": 12},
     "compare": "bytes", "uid": 0},
    {"name": "generate_multibyte", "method": "POST", "path": "/generate",
     "body": {"prompt": "你好，世界！😀👍🏽 café", "max_tokens": 16},
     "compare": "bytes", "uid": 1},
    {"name": "generate_one_token", "method": "POST", "path": "/generate",
     "body": {"prompt": "Hi", "max_tokens": 1, "ignore_eos": True},
     "compare": "bytes", "uid": 2},
    {"name": "chat_stream_system_user", "method": "POST", "path": "/v1/chat/completions",
     "body": {
         "model": "qwen3",
         "messages": [
             {"role": "system", "content": "You are terse."},
             {"role": "user", "content": "Say héllo 👋"},
         ],
         "max_tokens": 24,
         "stream": True,
     },
     "compare": "bytes", "uid": 3},
    {"name": "chat_stream_default_max_tokens", "method": "POST", "path": "/v1/chat/completions",
     "body": {
         "model": "qwen3",
         "messages": [{"role": "user", "content": "Count to three."}],
         "stream": True,
     },
     "compare": "bytes", "uid": 4},
    {"name": "chat_nonstream_multibyte", "method": "POST", "path": "/v1/chat/completions",
     "body": {
         "model": "qwen3",
         "messages": [{"role": "user", "content": "翻译：早上好 🌅"}],
         "max_tokens": 20,
     },
     "compare": "bytes", "uid": 5},
    {"name": "chat_nonstream_eos_final", "method": "POST", "path": "/v1/chat/completions",
     "body": {
         "model": "qwen3",
         "messages": [{"role": "user", "content": "hi"}],
         "max_tokens": _EOS_MAX_TOKENS,
     },
     "compare": "bytes", "uid": 6},
    {"name": "chat_nonstream_prompt_text", "method": "POST", "path": "/v1/chat/completions",
     "body": {"model": "qwen3", "prompt": "Plain text prompt, no template.", "max_tokens": 8},
     "compare": "bytes", "uid": 7},
    {"name": "chat_stream_empty_messages_prompt", "method": "POST", "path": "/v1/chat/completions",
     "body": {"model": "qwen3", "messages": [], "prompt": "fallback", "max_tokens": 4,
               "stream": True},
     "compare": "bytes", "uid": 8},
    {"name": "generate_missing_max_tokens", "method": "POST", "path": "/generate",
     "body": {"prompt": "x"},
     "compare": "status", "uid": None},
    {"name": "chat_bad_role", "method": "POST", "path": "/v1/chat/completions",
     "body": {"model": "qwen3", "messages": [{"role": "tool", "content": "x"}]},
     "compare": "status", "uid": None},
    {"name": "chat_missing_prompt", "method": "POST", "path": "/v1/chat/completions",
     "body": {"model": "qwen3"},
     "compare": "status", "uid": None},
    {"name": "chat_stream_after_errors", "method": "POST", "path": "/v1/chat/completions",
     "body": {
         "model": "qwen3",
         "messages": [{"role": "user", "content": "ok"}],
         "max_tokens": 2,
         "stream": True,
     },
     "compare": "bytes", "uid": 9},
]


def normalize_created(body: bytes) -> Tuple[bytes, int]:
    """Replace the single `"created":<digits>` occurrence with `"created":0`, returning the
    original int. Raises ValueError if there is zero or more than one occurrence."""
    matches = list(_CREATED_RE.finditer(body))
    if len(matches) != 1:
        raise ValueError(
            f'expected exactly one "created":<digits> occurrence, found {len(matches)}'
        )
    match = matches[0]
    original = int(match.group(1))
    normalized = body[: match.start()] + b'"created":0' + body[match.end():]
    return normalized, original


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _drain(stream, sink: List[str]) -> None:
    for raw in iter(stream.readline, b""):
        sink.append(raw.decode("utf-8", errors="replace").rstrip("\n"))
    stream.close()


def _resolve_mock_bin() -> Path:
    try:
        subprocess.run(
            ["cargo", "build", "-p", "rsg-server", "--bin", "mock-scheduler"],
            cwd=REPO, check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise EnvError(f"cargo build -p rsg-server --bin mock-scheduler failed: {exc}") from exc
    path = REPO / "target" / "debug" / "mock-scheduler"
    if not path.is_file():
        raise EnvError(f"mock-scheduler binary not found at {path} after cargo build")
    return path


def _load_server_args(suffix: str):
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import minisgl
        from minisgl.server.args import parse_args
    except ImportError as exc:
        raise EnvError(f"cannot import upstream server args: {exc}") from exc
    origin = Path(list(minisgl.__path__)[0]).resolve()
    if not origin.is_relative_to(VENDOR_PY.resolve()):
        raise EnvError(f"minisgl resolved to {origin}, not the vendored tree under {VENDOR_PY}")
    server_args, run_shell = parse_args(["--model", MODEL, "--dtype", "bfloat16"])
    if run_shell:
        raise EnvError("parse_args unexpectedly set run_shell")
    return dataclasses.replace(server_args, _unique_suffix=suffix)


def _compute_eos_max_tokens() -> int:
    """N such that a greedy echo of `messages` ends on the model's eos_token_id: the index of
    the first eos id in the rendered+encoded prompt, plus one (the echoed token itself)."""
    from transformers import AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(MODEL)
    messages = [{"role": "user", "content": "hi"}]
    prompt = tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
    ids = tokenizer.encode(prompt)
    eos_id = tokenizer.eos_token_id
    try:
        index = ids.index(eos_id)
    except ValueError as exc:
        raise EnvError(
            f"eos_token_id {eos_id} does not occur in encoded ids for {messages!r}"
        ) from exc
    return index + 1


def _spawn_mock(mock_bin: Path, server_args) -> Tuple[subprocess.Popen, List[str]]:
    detok_role = "bind" if server_args.backend_create_detokenizer_link else "connect"
    mock = subprocess.Popen(
        [
            str(mock_bin),
            "--backend-addr", server_args.zmq_backend_addr,
            "--backend-role", "bind",
            "--detok-addr", server_args.zmq_detokenizer_addr,
            "--detok-role", detok_role,
            *MOCK_ARGS,
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    errors: List[str] = []
    threading.Thread(target=_drain, args=(mock.stderr, errors), daemon=True).start()
    deadline = time.monotonic() + _HANDSHAKE_DEADLINE_S
    line = b""
    while time.monotonic() < deadline and not line:
        line = mock.stdout.readline()
    if not line:
        mock.kill()
        raise EnvError(
            f"mock-scheduler printed no handshake line within {_HANDSHAKE_DEADLINE_S:g}s\n"
            + "\n".join(errors)
        )
    return mock, errors


def _spawn_frontend(suffix: str, port: int) -> Tuple[subprocess.Popen, List[str]]:
    frontend = subprocess.Popen(
        [
            sys.executable, "-m", "rsglang.testing.python_frontend",
            "--rsg-suffix", suffix,
            "--model", MODEL,
            "--dtype", "bfloat16",
            "--host", "127.0.0.1",
            "--port", str(port),
        ],
        cwd=REPO,
        start_new_session=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    out: List[str] = []
    threading.Thread(target=_drain, args=(frontend.stdout, out), daemon=True).start()
    return frontend, out


def _wait_ready(frontend: subprocess.Popen, port: int, log: List[str]) -> None:
    deadline = time.monotonic() + _READY_DEADLINE_S
    while time.monotonic() < deadline:
        if frontend.poll() is not None:
            raise EnvError(
                f"python_frontend exited early with code {frontend.poll()}\n" + "\n".join(log)
            )
        try:
            conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            try:
                conn.request("GET", "/v1/models")
                resp = conn.getresponse()
                resp.read()
                if resp.status == 200:
                    return
            finally:
                conn.close()
        except (ConnectionRefusedError, OSError, http.client.HTTPException):
            pass
        time.sleep(0.5)
    raise EnvError(
        f"frontend never answered /v1/models within {_READY_DEADLINE_S:g}s\n" + "\n".join(log)
    )


def _teardown(frontend: Optional[subprocess.Popen], mock: Optional[subprocess.Popen],
              suffix: str) -> None:
    from rsglang import sockets

    if frontend is not None:
        try:
            os.killpg(frontend.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            frontend.wait(timeout=_TEARDOWN_GRACE_S)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(frontend.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            frontend.wait(timeout=_TEARDOWN_GRACE_S)
    if mock is not None:
        try:
            mock.terminate()
            mock.wait(timeout=_TEARDOWN_GRACE_S)
        except subprocess.TimeoutExpired:
            mock.kill()
            mock.wait(timeout=_TEARDOWN_GRACE_S)
    sockets.unlink_run_sockets(suffix)


def _resolve_case_body(case: Dict, eos_max_tokens: int) -> Optional[dict]:
    body = case["body"]
    if body is None:
        return None
    if any(v is _EOS_MAX_TOKENS for v in body.values()):
        body = dict(body)
        body["max_tokens"] = eos_max_tokens
    return body


def _send_case(port: int, case: Dict, body_bytes: Optional[bytes]):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
    try:
        headers = {"Content-Type": "application/json"} if body_bytes is not None else {}
        conn.request(case["method"], case["path"], body=body_bytes, headers=headers)
        resp = conn.getresponse()
        content_type = resp.getheader("content-type")
        raw = resp.read()
        return resp.status, content_type, raw
    finally:
        conn.close()


def _normalize_body(name: str, body: bytes) -> bytes:
    normalized, original = normalize_created(body)
    skew = abs(original - time.time())
    if skew > _MAX_CREATED_SKEW_S:
        raise EnvError(
            f'{name}: "created":{original} is {skew:.0f}s from capture time; refusing to normalize'
        )
    return normalized


def _generator_versions() -> Dict[str, str]:
    import platform

    import fastapi
    import pydantic
    import starlette
    import tokenizers
    import transformers
    import uvicorn

    return {
        "python": platform.python_version(),
        "fastapi": fastapi.__version__,
        "starlette": starlette.__version__,
        "uvicorn": uvicorn.__version__,
        "pydantic": pydantic.__version__,
        "transformers": transformers.__version__,
        "tokenizers": tokenizers.__version__,
    }


def generate(out_dir: Path) -> int:
    """Capture every case in CASES against one fresh python_frontend + mock-scheduler run,
    writing out_dir/<name>.body for bytes-compared cases and out_dir/manifest.json. Returns the
    case count. Always tears down the spawned processes and sockets, even on error."""
    try:
        upstream_sha = SHA_FILE.read_text().strip()
    except OSError as exc:
        raise EnvError(f"cannot read {SHA_FILE}: {exc}") from exc

    mock_bin = _resolve_mock_bin()
    eos_max_tokens = _compute_eos_max_tokens()

    suffix = f".rsgapi={os.getpid()}"
    server_args = _load_server_args(suffix)
    from rsglang import sockets

    sockets.unlink_run_sockets(suffix)

    port = _free_port()
    mock = None
    frontend = None
    try:
        mock, _mock_log = _spawn_mock(mock_bin, server_args)
        frontend, frontend_log = _spawn_frontend(suffix, port)
        _wait_ready(frontend, port, frontend_log)

        out_dir.mkdir(parents=True, exist_ok=True)
        entries = []
        for case in CASES:
            resolved_body = _resolve_case_body(case, eos_max_tokens)
            body_bytes = None if resolved_body is None else json.dumps(
                resolved_body, ensure_ascii=False
            ).encode("utf-8")
            status, content_type, raw = _send_case(port, case, body_bytes)

            normalize_list: List[str] = []
            if case["compare"] == "bytes":
                if case["name"] in NORMALIZED_FIELDS:
                    raw = _normalize_body(case["name"], raw)
                    normalize_list = list(NORMALIZED_FIELDS[case["name"]])
                (out_dir / f"{case['name']}.body").write_bytes(raw)

            entries.append({
                "name": case["name"],
                "method": case["method"],
                "path": case["path"],
                "request_body": None if resolved_body is None else json.dumps(
                    resolved_body, ensure_ascii=False
                ),
                "status": status,
                "content_type": content_type,
                "compare": case["compare"],
                "normalize": normalize_list,
                "uid": case["uid"],
            })

        manifest = {
            "model": MODEL,
            "mock_args": MOCK_ARGS,
            "upstream_sha": upstream_sha,
            "cases": entries,
            "generator": _generator_versions(),
        }
        (out_dir / "manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        return len(entries)
    finally:
        _teardown(frontend, mock, suffix)


def check(committed: Path) -> int:
    """Regenerate into a temp dir and diff against the committed fixtures; return the exit code."""
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp)
        count = generate(fresh)
        diffs = []
        fresh_names = {p.name for p in fresh.glob("*.body")}
        committed_names = {p.name for p in committed.glob("*.body")}
        for name in sorted(committed_names - fresh_names):
            diffs.append((name, "committed fixture has no generated case"))
        for name in sorted(fresh_names - committed_names):
            diffs.append((name, "generated case is missing from the committed fixtures"))
        for name in sorted(fresh_names & committed_names):
            if (fresh / name).read_bytes() != (committed / name).read_bytes():
                diffs.append((name, "bytes differ"))
        try:
            committed_manifest = json.loads((committed / "manifest.json").read_text())
        except (OSError, ValueError) as exc:
            diffs.append(("manifest.json", f"unreadable: {exc}"))
        else:
            fresh_manifest = json.loads((fresh / "manifest.json").read_text())
            for key in CHECKED_MANIFEST_KEYS:
                if committed_manifest.get(key) != fresh_manifest.get(key):
                    diffs.append(("manifest.json", f"key {key!r} differs"))
    for name, reason in diffs:
        print(f"DIFF {name}: {reason}")
    if diffs:
        return 1
    print(f"gen_api_fixtures: fixtures match ({count} cases)")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=FIXTURES_DIR, help="output directory")
    parser.add_argument(
        "--check", action="store_true", help="regenerate into a temp dir and byte-diff"
    )
    args = parser.parse_args(argv)
    try:
        if args.check:
            return check(args.out)
        count = generate(args.out)
    except EnvError as exc:
        print(f"gen_api_fixtures: error: {exc}", file=sys.stderr)
        return 2
    print(f"gen_api_fixtures: wrote {count} cases to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
