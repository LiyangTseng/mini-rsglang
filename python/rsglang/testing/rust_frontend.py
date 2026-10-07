"""Mac-only Rust-frontend test server (Phase 6 Task 2, precondition item (a)):
wires the real `rsg-server` binary to a real `mock-scheduler` subprocess over
real `ipc://` sockets -- two separate OS processes, connected exactly the way
the real launcher connects `rsg-server` to the real GPU scheduler, except the
backend is Phase 3's GPU-free `mock-scheduler` stand-in (D-08) instead of the
real one.

This is the Rust-frontend counterpart of `python_frontend.py`: that module
runs upstream's frozen Python frontend against an externally-started
`mock-scheduler`; this module runs the real `rsg-server` binary the same way.
`rsglang.launch --frontend rust` cannot be reused directly for this: it always
spawns the real upstream scheduler via multiprocessing (GPU-only), and the
Mac-only `RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler`
substitution used by `test_launch_rust_e2e.py` never answers a `UserMsg` with
tokens (see its `run_forever`), so it cannot drive a real `/generate` or
`/v1/chat/completions` response -- only `mock-scheduler`'s real echo/delay
decode loop can. Hence this standalone wiring.

`mock-scheduler` prints its readiness handshake (the same JSON line format
`rsg-server` reads on stdin) as its first stdout line; this module reads that
line and forwards it verbatim to `rsg-server`'s stdin, mirroring what
`rsglang.launch`'s real launcher does with the real scheduler's handshake.

Usage:
    python -m rsglang.testing.rust_frontend --port PORT --model MODEL \
        [--host HOST] [--abort-timing immediate|deferred] \
        [--backend-timeout-ms N] [--rust-bin PATH] [--mock-bin PATH] \
        [--ready-timeout SECONDS]

Blocks until SIGINT/SIGTERM (or until `rsg-server` exits on its own), then
tears down `rsg-server` and `mock-scheduler` and unlinks the ipc socket
files. Exit code mirrors `rsg-server`'s own exit code; 2 on setup failure
before either child is reachable.

All side effects live under `if __name__ == "__main__":` for the same reason
as `rsglang.launch`/`python_frontend.py`: nothing here is multiprocessing
re-imported, but the convention is kept for consistency.
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Optional, Sequence

from .. import handshake, sockets
from ..launch import resolve_rust_bin

_PREFIX = "rsglang.testing.rust_frontend:"


def _log(msg: str) -> None:
    try:
        sys.stderr.write(f"{_PREFIX} {msg}\n")
        sys.stderr.flush()
    except (OSError, ValueError):
        pass


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m rsglang.testing.rust_frontend", allow_abbrev=False)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--abort-timing", choices=("immediate", "deferred"), default=None)
    parser.add_argument("--backend-timeout-ms", type=int, default=None)
    parser.add_argument("--max-seq-len", type=int, default=4096)
    parser.add_argument("--rust-bin", default=None, metavar="PATH")
    parser.add_argument("--mock-bin", default=None, metavar="PATH")
    parser.add_argument("--ready-timeout", type=float, default=60.0, metavar="SECONDS")
    parser.add_argument("--rust-log", default="info", metavar="LEVEL")
    return parser


def resolve_mock_bin(explicit: Optional[str]) -> Optional[Path]:
    """The mock-scheduler binary, same resolution convention as
    rsglang.launch.resolve_rust_bin (RSGLANG_MOCK_BIN env var, then
    target/{release,debug})."""
    root = handshake.repo_root()
    if explicit:
        candidates = [Path(explicit)]
    elif os.environ.get("RSGLANG_MOCK_BIN"):
        candidates = [Path(os.environ["RSGLANG_MOCK_BIN"])]
    else:
        candidates = [
            root / "target" / "release" / "mock-scheduler",
            root / "target" / "debug" / "mock-scheduler",
        ]
    for path in candidates:
        if path.is_file() and os.access(path, os.X_OK):
            return path
    _log("mock-scheduler binary not found; run: cargo build -p rsg-server --bin mock-scheduler")
    return None


def _drain(stream, prefix: str) -> None:
    for raw in iter(stream.readline, b""):
        line = raw.decode("utf-8", errors="replace").rstrip("\n")
        _log(f"[{prefix}] {line}")
    try:
        stream.close()
    except (OSError, ValueError):
        pass


def _read_first_line(stream, timeout_s: float) -> "bytes | None":
    """Blocks on the first stdout line of `stream` in a daemon thread, so a
    caller can enforce a deadline even though readline() itself has none."""
    box: "dict[str, bytes]" = {}

    def _reader() -> None:
        line = stream.readline()
        box["line"] = line

    t = threading.Thread(target=_reader, daemon=True)
    t.start()
    t.join(timeout=timeout_s)
    return box.get("line")


def _teardown(mock: "subprocess.Popen | None", rust: "subprocess.Popen | None", suffix: str) -> None:
    for proc in (rust, mock):
        if proc is None:
            continue
        try:
            proc.terminate()
        except ProcessLookupError:
            pass
    for proc, grace in ((rust, 10.0), (mock, 5.0)):
        if proc is None:
            continue
        try:
            proc.wait(timeout=grace)
        except subprocess.TimeoutExpired:
            try:
                proc.kill()
            except ProcessLookupError:
                pass
            try:
                proc.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                pass
    sockets.unlink_run_sockets(suffix)


def main(argv: "Sequence[str] | None" = None) -> int:
    ns = build_parser().parse_args(argv)

    mock_bin = resolve_mock_bin(ns.mock_bin)
    if mock_bin is None:
        return 2
    rust_bin = resolve_rust_bin(ns.rust_bin)
    if rust_bin is None:
        return 2

    suffix = f".rsg=rustfe-{os.getpid()}"
    sockets.unlink_run_sockets(suffix)
    paths = sockets.run_socket_paths(suffix)
    backend_addr = f"ipc://{paths[0]}"
    detok_addr = f"ipc://{paths[1]}"

    mock: "subprocess.Popen | None" = None
    rust: "subprocess.Popen | None" = None
    try:
        mock = subprocess.Popen(
            [
                str(mock_bin),
                "--backend-addr", backend_addr,
                "--backend-role", "bind",
                "--detok-addr", detok_addr,
                "--detok-role", "connect",
                "--max-seq-len", str(ns.max_seq_len),
            ],
            # mock-scheduler watches its own stdin for EOF as a parent-liveness
            # signal (EXIT_STDIN_EOF) -- DEVNULL is an immediate EOF, so this
            # must stay an open, unclosed pipe for the mock's whole lifetime
            # (mirrors tests/common/mod.rs's MockScheduler::spawn).
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        threading.Thread(target=_drain, args=(mock.stderr, "mock-scheduler"), daemon=True).start()

        handshake_line = _read_first_line(mock.stdout, ns.ready_timeout)
        if not handshake_line:
            _log(f"mock-scheduler printed no handshake line within {ns.ready_timeout:g}s")
            return 2
        threading.Thread(target=_drain, args=(mock.stdout, "mock-scheduler"), daemon=True).start()

        rust_argv = [
            str(rust_bin),
            "--backend-addr", backend_addr,
            "--backend-role", "connect",
            "--detok-addr", detok_addr,
            "--detok-role", "bind",
            "--model", ns.model,
            "--run-id", suffix,
            "--host", ns.host,
            "--port", str(ns.port),
        ]
        if ns.abort_timing:
            rust_argv += ["--abort-timing", ns.abort_timing]
        if ns.backend_timeout_ms is not None:
            rust_argv += ["--backend-timeout-ms", str(ns.backend_timeout_ms)]

        rust = subprocess.Popen(
            rust_argv,
            stdin=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={**os.environ, "RUST_LOG": ns.rust_log},
        )
        threading.Thread(target=_drain, args=(rust.stderr, "rsg-server"), daemon=True).start()

        try:
            rust.stdin.write(handshake_line)
            rust.stdin.flush()
        except BrokenPipeError:
            _log(f"rsg-server exited with code {rust.poll()} before the handshake was sent")
            return rust.poll() or 2

        _log(f"ready: mock-scheduler pid={mock.pid} rsg-server pid={rust.pid} port={ns.port}")

        try:
            return rust.wait()
        except (KeyboardInterrupt, SystemExit):
            return 0
    finally:
        _teardown(mock, rust, suffix)


if __name__ == "__main__":
    sys.exit(main())
