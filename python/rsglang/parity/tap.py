"""Schema and process-local plumbing for the parity backend tap (PAR-01/PAR-02).

Task 1 adds only the record schema, `tap_env`, and `load_tap_records`: the
pieces the fake-session tracer needs to drive a tap directory end to end.
Task 2 adds the real env-gated in-scheduler instrumentation (`install`,
`patch`, `write_shim`) without changing this module's existing names or
signatures.

Standard library only.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Mapping

TAP_DIR_ENV = "RSGLANG_PARITY_TAP_DIR"

KIND_PATCHED = "patched"
KIND_USER = "user"
KIND_ABORT = "abort"
KIND_FREE = "free"
KIND_COLLISION = "collision"
KIND_DETOK = "detok"

_KNOWN_KINDS = {KIND_PATCHED, KIND_USER, KIND_ABORT, KIND_FREE, KIND_COLLISION, KIND_DETOK}


@dataclass
class TapRecords:
    records: "list[dict[str, Any]]" = field(default_factory=list)
    malformed_lines: int = 0


def tap_env(base_env: Mapping[str, str], *, work_dir: Path, tap_dir: Path) -> "dict[str, str]":
    """base_env plus TAP_DIR_ENV pointed at tap_dir (created here).

    `work_dir` is accepted but unused by Task 1; Task 2 extends this function
    to also write the generated sitecustomize shim under work_dir without
    changing this signature.
    """
    env = dict(base_env)
    tap_dir = Path(tap_dir)
    tap_dir.mkdir(parents=True, exist_ok=True)
    env[TAP_DIR_ENV] = str(tap_dir)
    return env


def load_tap_records(tap_dir: Path) -> TapRecords:
    """Read every tap-*.jsonl file in tap_dir, in sorted (filename) order.

    A line counts as malformed, and is never raised, if it is not JSON, is
    not an object, lacks kind/pid/seq, or carries an unrecognized kind.
    Well-formed records are sorted by (pid, seq).
    """
    tap_dir = Path(tap_dir)
    result = TapRecords(records=[], malformed_lines=0)
    if not tap_dir.is_dir():
        return result

    for path in sorted(tap_dir.glob("tap-*.jsonl")):
        with open(path, "r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    result.malformed_lines += 1
                    continue
                if not isinstance(record, dict):
                    result.malformed_lines += 1
                    continue
                if "kind" not in record or "pid" not in record or "seq" not in record:
                    result.malformed_lines += 1
                    continue
                if record["kind"] not in _KNOWN_KINDS:
                    result.malformed_lines += 1
                    continue
                result.records.append(record)

    result.records.sort(key=lambda r: (r["pid"], r["seq"]))
    return result
