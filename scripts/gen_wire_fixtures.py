#!/usr/bin/env python3
"""Generate the golden msgpack wire fixtures from upstream's own encoder (D-13, D-14, D-16).

Every fixture is `msgpack.packb(serialize_type(obj), use_bin_type=True)` -- the same call as
upstream's `utils/mp.py` -- applied to upstream's real message classes from vendor/mini-sglang.

Usage:
  scripts/gen_wire_fixtures.py            write fixtures/wire/*.msgpack and manifest.json
  scripts/gen_wire_fixtures.py --out DIR  write them to DIR instead
  scripts/gen_wire_fixtures.py --check    regenerate into a temp dir and byte-diff with fixtures/wire

Exit codes: 0 ok, 1 fixtures differ (--check), 2 environment error.
The case table must stay in step with crates/rsg-wire/tests/common/mod.rs.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import platform
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
VENDOR_PY = REPO / "vendor" / "mini-sglang" / "python"
FIXTURES_DIR = REPO / "fixtures" / "wire"
SHA_FILE = REPO / "vendor" / "UPSTREAM_SHA"

INTERPRETATION = (
    "7 message types = UserMsg, AbortBackendMsg, ExitMsg, BatchBackendMsg, DetokenizeMsg, "
    "BatchTokenizerMsg, SamplingParams; Tensor is the 8th __type__ tag"
)
TYPE_TAGS = [
    "UserMsg",
    "AbortBackendMsg",
    "ExitMsg",
    "BatchBackendMsg",
    "DetokenizeMsg",
    "BatchTokenizerMsg",
    "SamplingParams",
    "Tensor",
]
CHECKED_MANIFEST_KEYS = ("upstream_sha", "interpretation", "type_tags", "cases")


class EnvError(Exception):
    pass


def _load_upstream():
    """Import upstream's classes from the vendored tree, and nowhere else."""
    sys.path.insert(0, str(VENDOR_PY))
    try:
        import msgpack
        import numpy
        import torch
        import minisgl
        from minisgl.core import SamplingParams
        from minisgl.message import (
            AbortBackendMsg,
            BatchBackendMsg,
            BatchTokenizerMsg,
            DetokenizeMsg,
            ExitMsg,
            UserMsg,
        )
        from minisgl.message.utils import serialize_type
    except ImportError as exc:
        raise EnvError(f"cannot import upstream message classes: {exc}") from exc
    origin = Path(list(minisgl.__path__)[0]).resolve()
    if not origin.is_relative_to(VENDOR_PY.resolve()):
        raise EnvError(f"minisgl resolved to {origin}, not the vendored tree under {VENDOR_PY}")
    return {
        "msgpack": msgpack,
        "numpy": numpy,
        "torch": torch,
        "SamplingParams": SamplingParams,
        "UserMsg": UserMsg,
        "AbortBackendMsg": AbortBackendMsg,
        "ExitMsg": ExitMsg,
        "BatchBackendMsg": BatchBackendMsg,
        "DetokenizeMsg": DetokenizeMsg,
        "BatchTokenizerMsg": BatchTokenizerMsg,
        "serialize_type": serialize_type,
    }


def build_cases(u):
    """The fixture case table, in table order: (name, decoder, value)."""
    torch = u["torch"]
    SP = u["SamplingParams"]
    User, Abort, Exit = u["UserMsg"], u["AbortBackendMsg"], u["ExitMsg"]
    BatchB, Detok, BatchT = u["BatchBackendMsg"], u["DetokenizeMsg"], u["BatchTokenizerMsg"]

    def ids(n):
        return torch.tensor([(i * 7919) % 151936 for i in range(n)], dtype=torch.int32)

    # Float fields always get float literals: an int literal would encode as a msgpack int.
    cases = [
        (
            "base_user_msg",
            "backend",
            User(
                uid=7,
                input_ids=ids(3),
                sampling_params=SP(
                    temperature=0.0, top_k=-1, top_p=1.0, ignore_eos=False, max_tokens=128
                ),
            ),
        ),
        ("base_abort_backend_msg", "backend", Abort(uid=7)),
        ("base_exit_msg", "backend", Exit()),
        (
            "base_batch_backend_msg",
            "backend",
            BatchB(data=[User(uid=1, input_ids=ids(3), sampling_params=SP()), Abort(uid=2)]),
        ),
        ("base_detokenize_msg", "tokenizer", Detok(uid=7, next_token=151645, finished=True)),
        (
            "base_batch_tokenizer_msg",
            "tokenizer",
            BatchT(data=[Detok(uid=1, next_token=5, finished=False)]),
        ),
        (
            "base_sampling_params",
            "backend",
            SP(temperature=0.7, top_k=50, top_p=0.9, ignore_eos=True, max_tokens=256),
        ),
        ("base_tensor", "backend", ids(3)),
        (
            "batch_tokenizer_n",
            "tokenizer",
            BatchT(
                data=[Detok(uid=i, next_token=i + 4, finished=(i % 2 == 0)) for i in range(1, 6)]
            ),
        ),
        (
            "batch_backend_many",
            "backend",
            BatchB(
                data=[
                    User(uid=10, input_ids=ids(5), sampling_params=SP(max_tokens=16)),
                    Abort(uid=11),
                    User(uid=12, input_ids=ids(1), sampling_params=SP()),
                    Abort(uid=13),
                ]
            ),
        ),
    ]
    for n in (127, 128, 255, 256, 65535, 65536, 4294967296):
        cases.append((f"int_uid_{n}", "backend", Abort(uid=n)))
    for n in (1, 32, 33, 128, 129):
        cases.append(
            (
                f"int_top_k_neg{n}",
                "backend",
                User(uid=1, input_ids=ids(1), sampling_params=SP(top_k=-n)),
            )
        )
    cases += [
        (
            "int_max_tokens_65536",
            "backend",
            User(uid=1, input_ids=ids(1), sampling_params=SP(max_tokens=65536)),
        ),
        ("int_next_token_65536", "tokenizer", Detok(uid=1, next_token=65536, finished=False)),
        (
            "float_top_p_0_9",
            "backend",
            User(uid=1, input_ids=ids(1), sampling_params=SP(top_p=0.9)),
        ),
        (
            "float_temperature_1_5",
            "backend",
            User(uid=1, input_ids=ids(1), sampling_params=SP(temperature=1.5)),
        ),
        (
            "float_temperature_0_1",
            "backend",
            User(uid=1, input_ids=ids(1), sampling_params=SP(temperature=0.1)),
        ),
        (
            "bool_ignore_eos_true",
            "backend",
            User(uid=1, input_ids=ids(1), sampling_params=SP(ignore_eos=True)),
        ),
        ("bool_finished_false", "tokenizer", Detok(uid=2, next_token=9, finished=False)),
    ]
    for n in (1, 63, 64, 16383, 16384):
        cases.append(
            (
                f"tensor_len_{n}",
                "backend",
                User(uid=1, input_ids=ids(n), sampling_params=SP()),
            )
        )
    return cases


def _summarize(value):
    """The serialized dict with every bytes buffer replaced by its length and first ids."""
    if isinstance(value, dict):
        return {k: _summarize(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_summarize(v) for v in value]
    if isinstance(value, bytes):
        n_tokens = len(value) // 4
        first = [
            int.from_bytes(value[4 * i : 4 * i + 4], "little", signed=True)
            for i in range(min(4, n_tokens))
        ]
        return {"len_bytes": len(value), "len_tokens": n_tokens, "first_ids": first}
    return value


def generate(out_dir: Path) -> int:
    """Write every fixture and the manifest into out_dir; return the case count."""
    if sys.byteorder != "little":
        raise EnvError(f"tensor fixtures need a little-endian host, got {sys.byteorder}")
    try:
        upstream_sha = SHA_FILE.read_text().strip()
    except OSError as exc:
        raise EnvError(f"cannot read {SHA_FILE}: {exc}") from exc
    u = _load_upstream()
    msgpack, serialize_type = u["msgpack"], u["serialize_type"]

    def enc(obj) -> bytes:
        return msgpack.packb(serialize_type(obj), use_bin_type=True)

    out_dir.mkdir(parents=True, exist_ok=True)
    entries = []
    for name, decoder, value in build_cases(u):
        serialized = serialize_type(value)
        raw = enc(value)
        file_name = f"{name}.msgpack"
        (out_dir / file_name).write_bytes(raw)
        entries.append(
            {
                "name": name,
                "file": file_name,
                "decoder": decoder,
                "top_type": serialized["__type__"],
                "category": name.split("_", 1)[0],
                "len": len(raw),
                "sha256": hashlib.sha256(raw).hexdigest(),
                "summary": _summarize(serialized),
            }
        )
    manifest = {
        "upstream_sha": upstream_sha,
        "interpretation": INTERPRETATION,
        "type_tags": TYPE_TAGS,
        "generator": {
            "python": platform.python_version(),
            "torch": u["torch"].__version__,
            "msgpack": ".".join(str(p) for p in u["msgpack"].version),
            "numpy": u["numpy"].__version__,
        },
        "cases": entries,
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return len(entries)


def check(committed: Path) -> int:
    """Regenerate into a temp dir and diff against the committed fixtures; return the exit code."""
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp)
        count = generate(fresh)
        diffs = []
        fresh_names = {p.name for p in fresh.glob("*.msgpack")}
        committed_names = {p.name for p in committed.glob("*.msgpack")}
        for name in sorted(committed_names - fresh_names):
            diffs.append((name, "committed fixture has no generated case"))
        for name in sorted(fresh_names - committed_names):
            diffs.append((name, "generated case is missing from the committed fixtures"))
        for name in sorted(fresh_names & committed_names):
            if (fresh / name).read_bytes() != (committed / name).read_bytes():
                diffs.append((name, "bytes differ"))
        try:
            committed_manifest = json.loads((committed / "manifest.json").read_text())
        except (OSError, ValueError) as exc:
            diffs.append(("manifest.json", f"unreadable: {exc}"))
        else:
            fresh_manifest = json.loads((fresh / "manifest.json").read_text())
            for key in CHECKED_MANIFEST_KEYS:
                if committed_manifest.get(key) != fresh_manifest.get(key):
                    diffs.append(("manifest.json", f"key {key!r} differs"))
    for name, reason in diffs:
        print(f"DIFF {name}: {reason}")
    if diffs:
        return 1
    print(f"gen_wire_fixtures: fixtures match ({count} cases)")
    return 0


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
        print(f"gen_wire_fixtures: error: {exc}", file=sys.stderr)
        return 2
    print(f"gen_wire_fixtures: wrote {count} cases to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
