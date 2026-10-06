#!/usr/bin/env python3
"""Capture the API-01 golden fixture set from a live run of upstream's frozen Python frontend
against the mock-scheduler (D-02).

Every fixture is the status code, content-type and (for `compare: "bytes"` cases) the
normalized response body bytes of one of 18 fixed requests, issued one at a time, in a fixed
order, against a single fresh run of `rsglang.testing.python_frontend` (Plan 05-05 Task 1)
backed by `target/debug/mock-scheduler`. The only normalization applied is the `created`
timestamp field, replaced with a fixed `0` after checking it is within one day of capture time.

Usage:
  scripts/gen_api_fixtures.py            write fixtures/api/*.body and manifest.json
  scripts/gen_api_fixtures.py --out DIR  write them to DIR instead
  scripts/gen_api_fixtures.py --check    regenerate into a temp dir and byte-diff with fixtures/api

Exit codes: 0 ok, 1 fixtures differ (--check), 2 environment error.
The case table must stay in step with crates/rsg-server/tests/api_parity.rs (plan 05-09).
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Dict, List, Tuple

REPO = Path(__file__).resolve().parent.parent
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"
FIXTURES_DIR = REPO / "fixtures" / "api"
SHA_FILE = REPO / "vendor" / "UPSTREAM_SHA"

MODEL = "Qwen/Qwen3-0.6B"
MOCK_ARGS = ["--prefill-delay-ms", "0", "--decode-delay-ms", "0", "--max-seq-len", "4096"]
CHECKED_MANIFEST_KEYS = ("model", "mock_args", "upstream_sha", "cases")
# TODO (RED stub, Plan 05-05 Task 2 GREEN): map each normalized case name to its normalized
# field names (only "created" is ever normalized).
NORMALIZED_FIELDS: Dict[str, Tuple[str, ...]] = {}

# TODO (RED stub, Plan 05-05 Task 2 GREEN): the 18-case table, in fixed capture order.
CASES: List[Dict] = []


class EnvError(Exception):
    pass


def normalize_created(body: bytes) -> Tuple[bytes, int]:
    """TODO (RED stub, Plan 05-05 Task 2 GREEN): replace the single "created":<digits>
    occurrence with "created":0, returning the original int; raise ValueError on zero or
    more than one occurrence. This stub is intentionally a no-op passthrough."""
    return body, 0


def generate(out_dir: Path) -> int:
    """TODO (RED stub, Plan 05-05 Task 2 GREEN): capture the live case table into out_dir."""
    raise NotImplementedError("gen_api_fixtures.generate: not implemented yet (RED phase stub)")


def check(committed: Path) -> int:
    """TODO (RED stub, Plan 05-05 Task 2 GREEN): regenerate into a temp dir and byte-diff."""
    return 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=FIXTURES_DIR, help="output directory")
    parser.add_argument(
        "--check", action="store_true", help="regenerate into a temp dir and byte-diff"
    )
    args = parser.parse_args(argv)
    try:
        if args.check:
            return check(args.out)
        count = generate(args.out)
    except EnvError as exc:
        print(f"gen_api_fixtures: error: {exc}", file=sys.stderr)
        return 2
    except NotImplementedError as exc:
        print(f"gen_api_fixtures: error: {exc}", file=sys.stderr)
        return 2
    print(f"gen_api_fixtures: wrote {count} cases to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
