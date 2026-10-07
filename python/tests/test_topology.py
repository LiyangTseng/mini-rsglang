"""D-07 topology and socket-path hygiene, pinned against upstream's real parse_args."""

from __future__ import annotations

import dataclasses
import os
from pathlib import Path

import pytest
from minisgl.distributed import DistributedInfo
from minisgl.server.args import parse_args

from rsglang import sockets

SUFFIX = ".rsg=test"
BASE_ARGS = ["--model", "Qwen/Qwen3-0.6B", "--dtype", "bfloat16"]  # explicit dtype: no network (Pitfall 5)


def _server_args(*extra: str):
    server_args, _ = parse_args([*BASE_ARGS, *extra])
    return dataclasses.replace(server_args, _unique_suffix=SUFFIX)


def test_default_topology_binds_detokenizer_link():
    assert sockets.rust_endpoints(_server_args()) == {
        "backend_addr": "ipc:///tmp/minisgl_0.rsg=test",
        "backend_role": "connect",
        "detok_addr": "ipc:///tmp/minisgl_1.rsg=test",
        "detok_role": "bind",
    }


def test_num_tokenizer_two_connects_detokenizer_link():
    # The scheduler binds _1 when tokenizers are separate, so Rust must connect (D-07).
    assert sockets.rust_endpoints(_server_args("--num-tokenizer", "2")) == {
        "backend_addr": "ipc:///tmp/minisgl_0.rsg=test",
        "backend_role": "connect",
        "detok_addr": "ipc:///tmp/minisgl_1.rsg=test",
        "detok_role": "connect",
    }


def test_endpoints_come_from_server_args_properties():
    server_args = _server_args()
    ep = sockets.rust_endpoints(server_args)
    assert ep["backend_addr"] == server_args.zmq_backend_addr
    assert ep["detok_addr"] == server_args.zmq_detokenizer_addr


def test_suffix_survives_per_rank_replace():
    rank_args = dataclasses.replace(_server_args(), tp_info=DistributedInfo(0, 1))
    assert rank_args._unique_suffix == SUFFIX
    assert rank_args.zmq_backend_addr == "ipc:///tmp/minisgl_0.rsg=test"


def test_rust_cli_args_exact():
    server_args = _server_args()
    assert sockets.rust_cli_args(server_args) == [
        "--backend-addr", "ipc:///tmp/minisgl_0.rsg=test",
        "--backend-role", "connect",
        "--detok-addr", "ipc:///tmp/minisgl_1.rsg=test",
        "--detok-role", "bind",
        "--model", "Qwen/Qwen3-0.6B",
        "--run-id", ".rsg=test",
        "--host", server_args.server_host,
        "--port", str(server_args.server_port),
    ]


def test_run_socket_paths_lists_five_files():
    assert sockets.run_socket_paths(SUFFIX) == [Path(f"/tmp/minisgl_{i}.rsg=test") for i in range(5)]


@pytest.mark.parametrize("bad", [".rsg=../x", "/etc", ".rsg=a/b", ""])
def test_run_socket_paths_rejects_unsafe_suffix(bad):
    with pytest.raises(ValueError):
        sockets.run_socket_paths(bad)


def test_unlink_run_sockets_only_touches_this_run():
    suffix = f".rsg=unit{os.getpid()}"
    mine = sockets.run_socket_paths(suffix)
    decoy = Path(f"/tmp/minisgl_0.rsg=decoy{os.getpid()}")
    try:
        for path in [*mine, decoy]:
            path.touch()
        removed = sockets.unlink_run_sockets(suffix)
        assert sorted(removed) == sorted(mine)
        assert not any(path.exists() for path in mine)
        assert decoy.exists()
        # Idempotent: a second call finds nothing to remove.
        assert sockets.unlink_run_sockets(suffix) == []
    finally:
        for path in [*mine, decoy]:
            path.unlink(missing_ok=True)
