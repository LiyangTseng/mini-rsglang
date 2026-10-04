"""The readiness handshake the launcher writes to rsg-server's stdin (D-10, D-11).

One JSON line, sent once TP rank 0 reports ready. The Rust side
(crates/rsg-server/src/handshake.rs) rejects unknown keys, so the keys and
their order are fixed here.
"""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Optional

HANDSHAKE_VERSION = 1
HANDSHAKE_KEYS = (
    "handshake_version",
    "upstream_sha",
    "max_seq_len",
    "eos_token_id",
    "page_size",
    "max_running_req",
    "num_pages",
)

_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def repo_root() -> Path:
    """The checkout root (python/rsglang/handshake.py -> repo)."""
    return Path(__file__).resolve().parents[2]


def read_upstream_sha(path: Optional[Path] = None) -> str:
    """Read the single-source upstream SHA from vendor/UPSTREAM_SHA."""
    if path is None:
        path = repo_root() / "vendor" / "UPSTREAM_SHA"
    sha = Path(path).read_text().strip()
    if not _SHA_RE.match(sha):
        raise ValueError(f"{path}: not a 40-character lowercase hex SHA: {sha!r}")
    return sha


def encode_handshake_line(payload: dict) -> bytes:
    """Encode the handshake as one compact JSON line (keys exactly HANDSHAKE_KEYS, in order)."""
    if tuple(payload) != HANDSHAKE_KEYS:
        raise ValueError(f"handshake keys must be exactly {HANDSHAKE_KEYS}, got {tuple(payload)}")
    return json.dumps(payload, separators=(",", ":")).encode() + b"\n"
