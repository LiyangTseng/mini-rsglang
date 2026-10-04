"""Scheduler-process entry for rust mode (D-09).

A clone of upstream's `_run_scheduler` (minisgl/server/launch.py) that also
reports the readiness handshake on the ready queue. The scheduler class is the
unmodified upstream `minisgl.scheduler:Scheduler`; no vendored file is patched.
"""

from __future__ import annotations

import importlib
import logging
import os
import threading
import time
import traceback
from typing import TYPE_CHECKING, Any, Callable, Dict

from .handshake import HANDSHAKE_VERSION

if TYPE_CHECKING:
    import multiprocessing as mp

    from minisgl.server.args import ServerArgs

SCHEDULER_FACTORY_ENV = "RSGLANG_SCHEDULER_FACTORY"
DEFAULT_SCHEDULER_FACTORY = "minisgl.scheduler:Scheduler"


def resolve_scheduler_factory() -> Callable[[Any], Any]:
    """The scheduler class to construct: upstream's by default, a Mac fake in tests."""
    spec = os.environ.get(SCHEDULER_FACTORY_ENV) or DEFAULT_SCHEDULER_FACTORY
    module_name, sep, attr = spec.partition(":")
    if not sep or not module_name or not attr:
        raise ValueError(f"{SCHEDULER_FACTORY_ENV}={spec!r}: expected 'module:attr'")
    return getattr(importlib.import_module(module_name), attr)


def extract_handshake(scheduler: Any, args: ServerArgs, upstream_sha: str) -> Dict[str, Any]:
    """Read the handshake values from a constructed scheduler (keys in HANDSHAKE_KEYS order)."""
    eos = scheduler.eos_token_id
    return {
        "handshake_version": HANDSHAKE_VERSION,
        "upstream_sha": upstream_sha,
        # The engine's value (min of config and KV capacity), not args.max_seq_len.
        "max_seq_len": int(scheduler.engine.max_seq_len),
        "eos_token_id": None if eos is None else int(eos),
        # Read after construction: the engine may override page_size (TRTLLM -> 64).
        "page_size": int(scheduler.cache_manager.page_size),
        "max_running_req": int(args.max_running_req),
        "num_pages": int(scheduler.engine.num_pages),
    }


def start_parent_watchdog(launcher_pid: int, poll_interval: float = 1.0) -> threading.Thread:
    """Exit this process as soon as the launcher is gone (D-12 backstop).

    The Python twin of rsg-server's stdin-EOF rule: a SIGKILLed launcher cannot
    leak a GPU-holding scheduler. os._exit skips cleanup on purpose; the parent
    that would coordinate a clean exit no longer exists.

    The launcher pid comes from the launcher at spawn time, never from
    os.getppid() read here: under the spawn start method this runs seconds after
    the child started, and a launcher killed in that window has already
    reparented the child, so a value read now can be init or a subreaper.
    """
    if os.getppid() != launcher_pid:  # the launcher died while this child was booting
        os._exit(1)

    def _watch() -> None:
        while True:
            time.sleep(poll_interval)
            if os.getppid() != launcher_pid:
                os._exit(1)

    thread = threading.Thread(target=_watch, name="rsglang-parent-watchdog", daemon=True)
    thread.start()
    return thread


def run_scheduler(
    args: ServerArgs, ready_queue: mp.Queue, upstream_sha: str, launcher_pid: int
) -> None:
    # First: the watchdog compares against the launcher pid passed at spawn time, so a
    # launcher that died during this child's boot is noticed at once.
    start_parent_watchdog(launcher_pid)
    rank = args.tp_info.rank
    passed_ready = False
    try:
        import torch
        from minisgl.utils import init_logger

        factory = resolve_scheduler_factory()
        with torch.inference_mode():
            scheduler = factory(args)
            scheduler.sync_all_ranks()

            if args.tp_info.is_primary():
                handshake = extract_handshake(scheduler, args, upstream_sha)
                ready_queue.put({"kind": "ready", "rank": rank, "handshake": handshake})
            passed_ready = True

            if args.silent_output:
                logging.disable(logging.INFO)

            try:
                scheduler.run_forever()
            except KeyboardInterrupt:
                logger = init_logger(__name__)
                if args.tp_info.is_primary():
                    print()  # for a clean newline after ^C
                    logger.info("Scheduler exiting gracefully...")
                scheduler.shutdown()
    except BaseException as exc:
        if isinstance(exc, KeyboardInterrupt) and passed_ready:
            return
        ready_queue.put({"kind": "error", "rank": rank, "traceback": traceback.format_exc()})
        raise
