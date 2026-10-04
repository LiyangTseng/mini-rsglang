"""The single launch command (D-05): python -m rsglang.launch --frontend python|rust <upstream args>.

--frontend python  execs upstream's own launcher (`python -m minisgl`) unchanged.
--frontend rust    starts the upstream scheduler ranks and rsg-server, then
                   sends rsg-server the readiness handshake on its stdin.

All side effects live under `if __name__ == "__main__":` because spawned
children re-import this module as __mp_main__.
"""

from __future__ import annotations

import argparse
import dataclasses
import os
import queue
import signal
import subprocess
import sys
import threading
import time
from collections import deque
from pathlib import Path
from typing import Callable, Deque, List, Optional, Sequence

from . import handshake, sockets

_PREFIX = "rsglang.launch:"
_SHUTDOWN_GRACE_S = 10.0
_SUPERVISE_POLL_S = 0.2
_READY_POLL_S = 0.5


def _write_stderr(text: str) -> None:
    # A closed or broken stderr (e.g. a pipeline reader that already exited) must
    # never abort supervision or shutdown halfway.
    try:
        sys.stderr.write(text)
        sys.stderr.flush()
    except (OSError, ValueError):
        pass


def _log(msg: str) -> None:
    _write_stderr(f"{_PREFIX} {msg}\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m rsglang.launch",
        allow_abbrev=False,
        description="Launch mini-sglang with the Python or the Rust frontend. "
        "Arguments not listed here are forwarded to upstream's server argument parser.",
    )
    parser.add_argument("--frontend", choices=["python", "rust"], required=True)
    parser.add_argument("--rust-bin", metavar="PATH", default=None,
                        help="rsg-server binary (default: $RSGLANG_RUST_BIN, then target/{release,debug})")
    parser.add_argument("--ready-timeout", metavar="SECONDS", type=float, default=900.0,
                        help="Seconds to wait for the backend to become ready (rust mode)")
    parser.add_argument("--rust-log", metavar="LEVEL", default="info",
                        help="RUST_LOG for rsg-server (rust mode)")
    return parser


def resolve_rust_bin(explicit: Optional[str]) -> Optional[Path]:
    """The rsg-server binary to run, or None (after printing why) if there is none."""
    root = handshake.repo_root()
    if explicit:
        candidates = [Path(explicit)]
    elif os.environ.get("RSGLANG_RUST_BIN"):
        candidates = [Path(os.environ["RSGLANG_RUST_BIN"])]
    else:
        candidates = [root / "target" / "release" / "rsg-server", root / "target" / "debug" / "rsg-server"]
    for path in candidates:
        if path.is_file() and os.access(path, os.X_OK):
            return path
    _log("rsg-server binary not found; run: cargo build -p rsg-server")
    return None


def exec_python_frontend(rest: Sequence[str], execv: Callable = os.execv) -> int:
    """Replace this process with upstream's launcher: the frozen baseline, zero overhead (D-05).

    Upstream's own launch_server parses the forwarded args; the launcher never does.
    """
    argv = [sys.executable, "-m", "minisgl", *rest]
    execv(sys.executable, argv)
    return 0  # only reached when execv is a test recorder


def _pump_rsg_stderr(stream, tail: Deque[str]) -> None:
    """Forward rsg-server's stderr live as '[rsg-server] <line>' and keep a tail for failures."""
    for raw in iter(stream.readline, b""):
        line = raw.decode("utf-8", errors="replace").rstrip("\n")
        tail.append(line)
        _write_stderr(f"[rsg-server] {line}\n")
    stream.close()


def run_rust_mode(ns: argparse.Namespace, rest: List[str]) -> int:
    if "--shell-mode" in rest:
        _log("--shell-mode is not supported with --frontend rust")
        return 2
    rust_bin = resolve_rust_bin(ns.rust_bin)
    if rust_bin is None:
        return 2
    suffix = f".rsg={os.getpid()}"
    try:
        return _run_rust_mode(ns, rest, rust_bin, suffix)
    finally:
        # Every exit path the launcher survives; a group SIGKILL cleans up before it fires.
        sockets.unlink_run_sockets(suffix)


def _run_rust_mode(ns: argparse.Namespace, rest: List[str], rust_bin: Path, suffix: str) -> int:
    import multiprocessing as mp

    from minisgl.distributed import DistributedInfo
    from minisgl.server.args import parse_args

    from . import backend

    server_args, _ = parse_args(rest)
    server_args = dataclasses.replace(server_args, _unique_suffix=suffix)  # D-06
    sockets.unlink_run_sockets(suffix)

    # Every child (scheduler ranks, rsg-server) joins the launcher's process group (D-12).
    if os.getpgrp() != os.getpid():
        os.setpgid(0, 0)

    upstream_sha = handshake.read_upstream_sha()

    stop_requested = False

    def _request_stop(signum, frame):
        nonlocal stop_requested
        stop_requested = True

    # Python-level handlers are reset to default in exec'd and spawned children.
    signal.signal(signal.SIGINT, _request_stop)
    signal.signal(signal.SIGTERM, _request_stop)

    # rsg-server first, so its startup overlaps backend startup (D-10).
    rust = subprocess.Popen(
        [str(rust_bin), *sockets.rust_cli_args(server_args)],
        stdin=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, "RUST_LOG": ns.rust_log},
    )
    _log(f"spawned rsg-server pid={rust.pid}")
    rust_tail: Deque[str] = deque(maxlen=200)
    pump = threading.Thread(target=_pump_rsg_stderr, args=(rust.stderr, rust_tail),
                            name="rsg-server-stderr", daemon=True)
    pump.start()

    mp.set_start_method("spawn", force=True)
    ready_queue = mp.Queue()
    world = server_args.tp_info.size
    ranks = []
    for i in range(world):
        rank_args = dataclasses.replace(server_args, tp_info=DistributedInfo(i, world))
        p = mp.Process(
            target=backend.run_scheduler,
            args=(rank_args, ready_queue, upstream_sha),
            daemon=False,
            name=f"rsglang-TP{i}-scheduler",
        )
        p.start()
        ranks.append(p)
        _log(f"spawned scheduler rank={i} pid={p.pid}")

    def children():
        yield "rsg-server", rust.poll()
        for p in ranks:
            yield p.name, p.exitcode

    def report_errors(timeout: float) -> None:
        """Print every scheduler error envelope that arrives within `timeout` seconds."""
        deadline = time.monotonic() + timeout
        while True:
            try:
                msg = ready_queue.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                return
            if msg.get("kind") == "error":
                _log(f"scheduler rank {msg.get('rank')} failed:\n{msg.get('traceback', '')}")

    def shutdown(code: int) -> int:
        # Only now: ignored dispositions are inherited across exec.
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        try:
            os.killpg(os.getpgrp(), signal.SIGINT)
        except ProcessLookupError:
            pass
        deadline = time.monotonic() + _SHUTDOWN_GRACE_S
        try:
            rust.wait(timeout=max(0.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            pass
        for p in ranks:
            p.join(max(0.0, deadline - time.monotonic()))
        if rust.poll() is not None:
            pump.join(timeout=2.0)  # collect rsg-server's last lines for the tail
        if code != 0:
            lines = list(rust_tail)
            _log(f"--- last {len(lines)} lines of rsg-server stderr ---")
            for line in lines:
                _write_stderr(f"{line}\n")
            _log("--- end ---")
        sockets.unlink_run_sockets(suffix)
        if rust.poll() is None or any(p.is_alive() for p in ranks):
            _log("escalating to SIGKILL for the process group")
            for stream in (sys.stdout, sys.stderr):
                try:
                    stream.flush()
                except (OSError, ValueError):
                    pass
            # Ends the launcher too: its exit status is the SIGKILL status, non-zero.
            os.killpg(os.getpgrp(), signal.SIGKILL)
        if rust.stdin is not None:
            try:
                rust.stdin.close()
            except BrokenPipeError:
                pass
        _log(f"exit code {code}")
        return code

    # Wait for TP rank 0 to report ready.
    deadline = time.monotonic() + ns.ready_timeout
    payload = None
    while payload is None:
        if stop_requested:
            return shutdown(0)
        try:
            msg = ready_queue.get(timeout=_READY_POLL_S)
        except queue.Empty:
            for name, code in children():
                if code is not None:
                    report_errors(1.0)
                    _log(f"{name} exited with code {code} before ready")
                    return shutdown(1)
            if time.monotonic() >= deadline:
                _log(f"backend not ready after {ns.ready_timeout:g} s")
                return shutdown(1)
            continue
        if msg.get("kind") == "ready":
            payload = msg["handshake"]
        elif msg.get("kind") == "error":
            _log(f"scheduler rank {msg.get('rank')} failed:\n{msg.get('traceback', '')}")
            return shutdown(1)

    try:
        rust.stdin.write(handshake.encode_handshake_line(payload))
        rust.stdin.flush()  # stdin stays open: closing it means shutdown
    except BrokenPipeError:
        _log(f"rsg-server exited with code {rust.poll()} before the handshake was sent")
        return shutdown(1)
    _log("backend ready; handshake sent to rsg-server")

    while True:
        if stop_requested:
            return shutdown(0)
        try:
            msg = ready_queue.get(timeout=_SUPERVISE_POLL_S)
        except queue.Empty:
            msg = None
        if msg is not None and msg.get("kind") == "error":
            _log(f"scheduler rank {msg.get('rank')} failed:\n{msg.get('traceback', '')}")
            return shutdown(1)
        for name, code in children():
            if code is not None:
                report_errors(1.0)
                _log(f"{name} exited with code {code}")
                return shutdown(1)


def main(argv: Optional[Sequence[str]] = None, *, execv: Callable = os.execv) -> int:
    ns, rest = build_parser().parse_known_args(argv)
    if ns.frontend == "python":
        return exec_python_frontend(rest, execv=execv)
    return run_rust_mode(ns, rest)


if __name__ == "__main__":
    sys.exit(main())
