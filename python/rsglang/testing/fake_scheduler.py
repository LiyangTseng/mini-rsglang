"""A Mac-only stand-in for upstream's Scheduler (the real one needs CUDA).

It exposes exactly the attributes `rsglang.backend.extract_handshake` reads and
opens the same ZMQ queues the real scheduler opens (minisgl/scheduler/io.py),
with upstream's own ZmqPullQueue/ZmqPushQueue. Select it with
RSGLANG_SCHEDULER_FACTORY=rsglang.testing.fake_scheduler:FakeScheduler.
"""

from __future__ import annotations

import os
import time
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING

import zmq
from minisgl.message import BaseBackendMsg, BaseTokenizerMsg, ExitMsg
from minisgl.utils import ZmqPullQueue, ZmqPushQueue

if TYPE_CHECKING:
    from minisgl.server.args import ServerArgs

FAKE_MAX_SEQ_LEN = 4096
FAKE_EOS_TOKEN_ID = 151645
FAKE_NUM_PAGES = 1024
FACTORY_PATH = "rsglang.testing.fake_scheduler:FakeScheduler"
STATUS_DIR_ENV = "RSGLANG_FAKE_STATUS_DIR"

_PEER_TIMEOUT_MS = 10_000


class FakeScheduler:
    def __init__(self, args: ServerArgs):
        self.args = args
        self.engine = SimpleNamespace(max_seq_len=FAKE_MAX_SEQ_LEN, num_pages=FAKE_NUM_PAGES)
        self.eos_token_id = FAKE_EOS_TOKEN_ID
        self.cache_manager = SimpleNamespace(page_size=args.page_size)
        self._recv = None
        self._send = None
        if args.tp_info.is_primary():
            # Copied from minisgl/scheduler/io.py (SchedulerIOMixin.__init__).
            self._recv = ZmqPullQueue(
                args.zmq_backend_addr,
                create=True,
                decoder=BaseBackendMsg.decoder,
            )
            self._send = ZmqPushQueue(
                args.zmq_detokenizer_addr,
                create=args.backend_create_detokenizer_link,
                encoder=BaseTokenizerMsg.encoder,
            )

    def sync_all_ranks(self) -> None:
        pass

    def _detok_peer_connected(self) -> bool:
        if self.args.backend_create_detokenizer_link:
            # The scheduler binds _1: a bound PUSH has no pipe until a peer connects.
            return self._send.socket.poll(_PEER_TIMEOUT_MS, zmq.POLLOUT) != 0
        # The scheduler connects _1: a connecting PUSH is writable at once unless
        # ZMQ_IMMEDIATE is set, so probe with a separate socket that has it set.
        probe = zmq.Context.instance().socket(zmq.PUSH)
        try:
            probe.setsockopt(zmq.IMMEDIATE, 1)
            probe.setsockopt(zmq.LINGER, 0)
            probe.connect(self.args.zmq_detokenizer_addr)
            return probe.poll(_PEER_TIMEOUT_MS, zmq.POLLOUT) != 0
        finally:
            probe.close()

    def run_forever(self) -> None:
        if not self.args.tp_info.is_primary():
            while True:
                time.sleep(0.05)
        if self._detok_peer_connected():
            status_dir = os.environ.get(STATUS_DIR_ENV)
            if status_dir:
                (Path(status_dir) / "detok_peer_connected").touch()
        while True:
            if not self._recv.empty():
                if isinstance(self._recv.get(), ExitMsg):
                    return
            else:
                time.sleep(0.05)

    def shutdown(self) -> None:
        for queue in (self._recv, self._send):
            if queue is not None:
                queue.stop()
