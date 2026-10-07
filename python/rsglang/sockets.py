"""Per-run socket paths and the Rust endpoint roles, derived from upstream ServerArgs.

Addresses are never re-derived here: they come from ServerArgs properties
(minisgl/scheduler/config.py, minisgl/server/args.py). Roles mirror what
upstream's Python mode produces for the same ServerArgs (D-07).
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import TYPE_CHECKING, Dict, List

if TYPE_CHECKING:
    from minisgl.server.args import ServerArgs

_SUFFIX_RE = re.compile(r"^[A-Za-z0-9._=-]+$")


def run_socket_paths(suffix: str) -> List[Path]:
    """The five ipc socket files of one run: /tmp/minisgl_{0..4}<suffix>."""
    if not _SUFFIX_RE.match(suffix):
        raise ValueError(f"invalid socket suffix {suffix!r}: must match {_SUFFIX_RE.pattern}")
    return [Path(f"/tmp/minisgl_{i}{suffix}") for i in range(5)]


def unlink_run_sockets(suffix: str) -> List[Path]:
    """Remove this run's socket files only (never a glob: other runs share /tmp)."""
    removed = []
    for path in run_socket_paths(suffix):
        try:
            path.unlink()
        except FileNotFoundError:
            continue
        removed.append(path)
    return removed


def rust_endpoints(server_args: ServerArgs) -> Dict[str, str]:
    """Rust's addresses and bind/connect roles, mirroring the Python-mode topology."""
    return {
        "backend_addr": server_args.zmq_backend_addr,
        "backend_role": "connect",
        "detok_addr": server_args.zmq_detokenizer_addr,
        "detok_role": "connect" if server_args.backend_create_detokenizer_link else "bind",
    }


def rust_cli_args(server_args: ServerArgs) -> List[str]:
    """The static rsg-server CLI (D-11). Forwards the upstream --host/--port the
    user asked for (plan 05-08): --abort-timing and --backend-timeout-ms are
    rsg-server's own flags with their own defaults, left for Phase 6 to forward
    deliberately once it decides the benchmark setting."""
    ep = rust_endpoints(server_args)
    return [
        "--backend-addr", ep["backend_addr"],
        "--backend-role", ep["backend_role"],
        "--detok-addr", ep["detok_addr"],
        "--detok-role", ep["detok_role"],
        "--model", server_args.model_path,
        "--run-id", server_args._unique_suffix,
        "--host", server_args.server_host,
        "--port", str(server_args.server_port),
    ]
