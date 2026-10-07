"""Backend window probe (D-08(a), RESEARCH "Open Questions" 2): injects
UserMsg/AbortBackendMsg directly onto the scheduler's backend PULL socket
using upstream's own wire encoder, at a sweep of abort delays, so the
abort-during-prefill window is exercised deliberately -- independent of the
frontend's disconnect-detection latency, which may never land an abort
inside that window on its own (05-CONTEXT D-01/D-04).

This module runs only inside a Rust-frontend session: the Rust transport
drops replies for unknown uids (Phase 3 criterion 4), so foreign probe uids
never reach a live client and never corrupt its state. The upstream Python
frontend's detokenizer keeps per-uid state and is not built to receive
foreign uids -- never run this against a `--frontend python` session.

Imports minisgl lazily (inside run_window_probe), so importing this module
never requires the vendored backend's CUDA-only dependencies to be
importable at module load time.
"""

from __future__ import annotations

import time
from typing import Sequence

#: Bounded PUSH send timeout (ms). ZmqPushQueue.put (vendored, frozen -- never
#: edited here) calls socket.send() with no SNDTIMEO, which blocks forever
#: once the PUSH HWM buffer fills with no PULL peer draining it. The caller
#: (stress.py's _run_probe_block) already checks the scheduler is alive
#: before probing; this is defense-in-depth for the TOCTOU window between
#: that check and the actual send (the scheduler can die mid-probe too --
#: this hung a real GPU run once already).
_SEND_TIMEOUT_MS = 5_000


class BackendUnreachable(RuntimeError):
    """Raised when a probe send times out because nothing is draining the
    backend's PULL socket (the scheduler died during the probe itself)."""


def run_window_probe(
    backend_addr: str,
    *,
    delays_ms: "Sequence[float]",
    repeats: int,
    prompt_ids: "Sequence[int]",
    uid_base: int = 1 << 40,
    gap_s: float = 0.2,
) -> "list[dict]":
    """Connects an extra PUSH peer to the scheduler's backend PULL socket
    and, for each repeat and each configured delay, sends UserMsg(uid, ...)
    then AbortBackendMsg(uid) `delay_ms` milliseconds later, for a fresh uid
    starting at `uid_base`. Returns [{uid, delay_ms}, ...] in send order.

    Raises BackendUnreachable if a send times out (the scheduler died mid-probe)
    rather than hanging forever.
    """
    import zmq
    import torch
    from minisgl.core import SamplingParams
    from minisgl.message import AbortBackendMsg, BaseBackendMsg, UserMsg
    from minisgl.utils import ZmqPushQueue

    queue = ZmqPushQueue(backend_addr, create=False, encoder=BaseBackendMsg.encoder)
    queue.socket.setsockopt(zmq.SNDTIMEO, _SEND_TIMEOUT_MS)
    time.sleep(0.2)  # let the PUSH/PULL connect settle before the first send

    def _put(msg: object) -> None:
        try:
            queue.put(msg)
        except zmq.Again as exc:
            raise BackendUnreachable(
                f"probe send timed out after {_SEND_TIMEOUT_MS}ms at {backend_addr} -- "
                "the scheduler likely died mid-probe"
            ) from exc

    trials: "list[dict]" = []
    try:
        trial_idx = 0
        for _repeat in range(repeats):
            for delay_ms in delays_ms:
                uid = uid_base + trial_idx
                trial_idx += 1
                input_ids = torch.tensor(list(prompt_ids), dtype=torch.int32)
                sampling = SamplingParams(temperature=0.0, max_tokens=4)
                _put(UserMsg(uid=uid, input_ids=input_ids, sampling_params=sampling))
                time.sleep(delay_ms / 1000.0)
                _put(AbortBackendMsg(uid=uid))
                time.sleep(gap_s)
                trials.append({"uid": uid, "delay_ms": delay_ms})
    finally:
        queue.stop()

    return trials
