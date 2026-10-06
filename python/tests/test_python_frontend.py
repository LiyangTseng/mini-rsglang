"""Tracer: upstream's unmodified Python frontend (api_server + tokenize_worker) served against
the mock-scheduler on the Mac (05-05 Task 1).
"""

from __future__ import annotations

import dataclasses
import http.client
import json
import os
import signal
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import List

import psutil
import pytest

from rsglang import sockets

pytestmark = pytest.mark.slow

REPO = Path(__file__).resolve().parents[2]
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"
MODEL = "Qwen/Qwen3-0.6B"

_HANDSHAKE_DEADLINE_S = 20.0
_READY_DEADLINE_S = 120.0
_TEARDOWN_GRACE_S = 10.0


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def _drain(stream, sink: List[str]) -> None:
    for raw in iter(stream.readline, b""):
        sink.append(raw.decode("utf-8", errors="replace").rstrip("\n"))
    stream.close()


@pytest.fixture(scope="module")
def mock_bin() -> Path:
    subprocess.run(
        ["cargo", "build", "-p", "rsg-server", "--bin", "mock-scheduler"], cwd=REPO, check=True
    )
    return REPO / "target" / "debug" / "mock-scheduler"


def test_tracer_python_frontend_serves_generate_against_mock(mock_bin, tmp_path):
    sys.path.insert(0, str(VENDOR_PY))
    from minisgl.server.args import parse_args

    suffix = f".rsgpf={os.getpid()}"
    port = _free_port()

    server_args, run_shell = parse_args(["--model", MODEL, "--dtype", "bfloat16"])
    assert not run_shell
    server_args = dataclasses.replace(server_args, _unique_suffix=suffix)
    detok_role = "bind" if server_args.backend_create_detokenizer_link else "connect"

    sockets.unlink_run_sockets(suffix)

    mock = subprocess.Popen(
        [
            str(mock_bin),
            "--backend-addr", server_args.zmq_backend_addr,
            "--backend-role", "bind",
            "--detok-addr", server_args.zmq_detokenizer_addr,
            "--detok-role", detok_role,
            "--prefill-delay-ms", "0",
            "--decode-delay-ms", "0",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    mock_err: List[str] = []
    mock_err_drain = threading.Thread(target=_drain, args=(mock.stderr, mock_err), daemon=True)
    mock_err_drain.start()

    frontend = None
    frontend_out: List[str] = []
    frontend_drain = None
    try:
        deadline = time.monotonic() + _HANDSHAKE_DEADLINE_S
        handshake_line = b""
        while time.monotonic() < deadline and not handshake_line:
            handshake_line = mock.stdout.readline()
        assert handshake_line, (
            f"mock-scheduler printed no handshake line within {_HANDSHAKE_DEADLINE_S:g}s\n"
            + "\n".join(mock_err)
        )

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
        frontend_drain = threading.Thread(
            target=_drain, args=(frontend.stdout, frontend_out), daemon=True
        )
        frontend_drain.start()

        models_body = None
        deadline = time.monotonic() + _READY_DEADLINE_S
        while time.monotonic() < deadline:
            if frontend.poll() is not None:
                pytest.fail(
                    f"python_frontend exited early with code {frontend.poll()}\n"
                    + "\n".join(frontend_out)
                )
            try:
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
                try:
                    conn.request("GET", "/v1/models")
                    resp = conn.getresponse()
                    body = resp.read()
                    if resp.status == 200:
                        models_body = body
                        break
                finally:
                    conn.close()
            except (ConnectionRefusedError, OSError, http.client.HTTPException):
                pass
            time.sleep(0.5)
        assert models_body is not None, (
            f"frontend never answered /v1/models within {_READY_DEADLINE_S:g}s\n"
            + "\n".join(frontend_out)
        )
        models = json.loads(models_body)
        assert models["data"][0]["id"] == MODEL, models

        request_body = json.dumps({"prompt": "Hello world", "max_tokens": 4}).encode()
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
        try:
            conn.request(
                "POST", "/generate", body=request_body,
                headers={"Content-Type": "application/json"},
            )
            resp = conn.getresponse()
            assert resp.status == 200, resp.status
            content_type = resp.getheader("content-type") or ""
            assert content_type.startswith("text/event-stream"), content_type
            raw = resp.read()
        finally:
            conn.close()

        assert raw.endswith(b"data: [DONE]\n"), raw
        assert not raw.endswith(b"data: [DONE]\n\n"), raw
        body_without_done = raw[: -len(b"data: [DONE]\n")]
        token_lines = [line for line in body_without_done.split(b"\n") if line]
        assert len(token_lines) == 4, token_lines
        for line in token_lines:
            assert line.startswith(b"data: "), line
    finally:
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
            if frontend_drain is not None:
                frontend_drain.join(timeout=5)

        try:
            mock.terminate()
            mock.wait(timeout=_TEARDOWN_GRACE_S)
        except subprocess.TimeoutExpired:
            mock.kill()
            mock.wait(timeout=_TEARDOWN_GRACE_S)
        mock_err_drain.join(timeout=5)

        sockets.unlink_run_sockets(suffix)

        def _survivors() -> List[str]:
            found = []
            needle_suffix = suffix
            needle_bin = str(mock_bin)
            for proc in psutil.process_iter(["pid", "cmdline"]):
                try:
                    cmdline = proc.info["cmdline"] or []
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    continue
                joined = " ".join(cmdline)
                if needle_suffix in joined or needle_bin in joined:
                    found.append(f"pid={proc.info['pid']} cmdline={joined!r}")
            return found

        deadline = time.monotonic() + _TEARDOWN_GRACE_S
        survivors = _survivors()
        while survivors and time.monotonic() < deadline:
            time.sleep(0.2)
            survivors = _survivors()
        assert not survivors, survivors
        assert not any(path.exists() for path in sockets.run_socket_paths(suffix))
