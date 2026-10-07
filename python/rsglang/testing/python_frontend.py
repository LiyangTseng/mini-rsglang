"""Runs upstream's frozen Python frontend half (api_server + tokenize_worker) against an
externally started scheduler stand-in (the mock-scheduler binary). Test/fixture tooling only,
never used for benchmarks or production. Nothing upstream is modified or patched; only the
scheduler-rank processes are left out.

Usage (the scheduler stand-in must already be bound on the same `--rsg-suffix` addresses):
    python -m rsglang.testing.python_frontend --rsg-suffix SUFFIX <upstream server args>

All side effects live under `if __name__ == "__main__":` because spawned tokenizer/detokenizer
children re-import this module as `__mp_main__` (same convention as rsglang.launch).
"""

from __future__ import annotations

import argparse
import dataclasses
import queue
import sys
from pathlib import Path
from typing import List, Tuple

REPO = Path(__file__).resolve().parents[3]
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"

_PREFIX = "rsglang.testing.python_frontend:"
# How long start_backend() waits for a single tokenizer/detokenizer ack before giving up.
_ACK_TIMEOUT_S = 120.0


def _log(msg: str) -> None:
    print(f"{_PREFIX} {msg}", file=sys.stderr, flush=True)


def _load_upstream():
    """Import upstream's frontend entry points from the vendored tree, and nowhere else.

    minisgl is a namespace package (no __init__.py), so there is no `minisgl.__file__` to
    check; the equivalent is `minisgl.__path__`, the same guard scripts/gen_wire_fixtures.py
    uses for the same reason.
    """
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import minisgl
        from minisgl.server.api_server import run_api_server
        from minisgl.server.args import parse_args
        from minisgl.tokenizer import tokenize_worker
    except ImportError as exc:
        raise EnvironmentError(f"cannot import upstream frontend modules: {exc}") from exc
    origin = Path(list(minisgl.__path__)[0]).resolve()
    if not origin.is_relative_to(VENDOR_PY.resolve()):
        raise EnvironmentError(
            f"minisgl resolved to {origin}, not the vendored tree under {VENDOR_PY}"
        )
    return run_api_server, parse_args, tokenize_worker


def _parse_rsg_args(argv) -> Tuple[str, List[str]]:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--rsg-suffix", dest="suffix", required=True)
    ns, rest = parser.parse_known_args(argv)
    return ns.suffix, rest


def _make_start_backend(server_args, tokenize_worker):
    """The upstream start_subprocess spawn, minus the scheduler-rank processes.

    Spawns exactly the detokenizer (and, if --num-tokenizer > 0, the separate tokenizer)
    processes `minisgl.server.launch.start_subprocess` would, with the same kwargs and
    process names, then waits for their acks. The scheduler-rank processes are left out:
    the mock-scheduler binary, started by the caller before this runs, stands in for them
    on the same ipc addresses.
    """

    def start_backend() -> None:
        import multiprocessing as mp

        mp.set_start_method("spawn", force=True)
        ack_queue: "mp.Queue[str]" = mp.Queue()
        num_tokenizers = server_args.num_tokenizer

        mp.Process(
            target=tokenize_worker,
            kwargs={
                "tokenizer_path": server_args.model_path,
                "addr": server_args.zmq_detokenizer_addr,
                "backend_addr": server_args.zmq_backend_addr,
                "frontend_addr": server_args.zmq_frontend_addr,
                "local_bs": 1,
                "create": server_args.tokenizer_create_addr,
                "tokenizer_id": num_tokenizers,
                "ack_queue": ack_queue,
            },
            daemon=False,
            name="minisgl-detokenizer-0",
        ).start()
        for i in range(num_tokenizers):
            mp.Process(
                target=tokenize_worker,
                kwargs={
                    "tokenizer_path": server_args.model_path,
                    "addr": server_args.zmq_tokenizer_addr,
                    "backend_addr": server_args.zmq_backend_addr,
                    "frontend_addr": server_args.zmq_frontend_addr,
                    "local_bs": 1,
                    "create": server_args.tokenizer_create_addr,
                    "tokenizer_id": i,
                    "ack_queue": ack_queue,
                },
                daemon=False,
                name=f"minisgl-tokenizer-{i}",
            ).start()

        # 1 detokenizer + num_tokenizers tokenizers; no scheduler ack (none spawned here).
        for _ in range(num_tokenizers + 1):
            try:
                msg = ack_queue.get(timeout=_ACK_TIMEOUT_S)
            except queue.Empty:
                _log(f"timed out after {_ACK_TIMEOUT_S:g}s waiting for a worker ack")
                sys.exit(2)
            _log(msg)

    return start_backend


def main(argv=None) -> int:
    suffix, rest = _parse_rsg_args(argv)
    try:
        run_api_server, parse_args, tokenize_worker = _load_upstream()
    except EnvironmentError as exc:
        _log(f"error: {exc}")
        return 2

    server_args, run_shell = parse_args(rest)
    if run_shell:
        _log("error: --shell-mode is not supported by the fixture runner")
        return 2
    server_args = dataclasses.replace(server_args, _unique_suffix=suffix)

    start_backend = _make_start_backend(server_args, tokenize_worker)
    run_api_server(server_args, start_backend, run_shell=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
