"""WIRE-02 (D-15): every message the Rust codec emits decodes through upstream's real decoder.

The Rust side (`cargo test -p rsg-wire --test dump`) writes one file per fixture case into
$DUMP_DIR. Each file must decode via BaseBackendMsg/BaseTokenizerMsg.decoder (the cls(**kwargs)
path the scheduler uses) without error and re-encode with upstream's serialize_type to exactly the
same bytes. Run both steps with scripts/check_wire_decode.sh.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import msgpack
import pytest
import torch
from minisgl.message import BaseBackendMsg, BaseTokenizerMsg
from minisgl.message.utils import serialize_type

REPO = Path(__file__).resolve().parents[2]
FIXTURES = REPO / "fixtures" / "wire"
CASES = json.loads((FIXTURES / "manifest.json").read_text())["cases"]
DECODERS = {"backend": BaseBackendMsg.decoder, "tokenizer": BaseTokenizerMsg.decoder}


@pytest.fixture(scope="module")
def dump_dir() -> Path:
    value = os.environ.get("DUMP_DIR")
    if not value:
        if os.environ.get("RSGLANG_REQUIRE_DUMP") == "1":
            pytest.fail("DUMP_DIR not set but RSGLANG_REQUIRE_DUMP=1")
        pytest.skip("DUMP_DIR not set; run scripts/check_wire_decode.sh")
    return Path(value)


@pytest.mark.parametrize("case", CASES, ids=[c["name"] for c in CASES])
def test_rust_message_decodes_through_upstream(dump_dir: Path, case: dict):
    raw = (dump_dir / f"{case['name']}.msgpack").read_bytes()

    obj = DECODERS[case["decoder"]](msgpack.unpackb(raw, raw=False))

    if case["top_type"] == "Tensor":
        assert isinstance(obj, torch.Tensor)
        assert obj.dtype == torch.int32
    else:
        assert type(obj).__name__ == case["top_type"]
    assert msgpack.packb(serialize_type(obj), use_bin_type=True) == raw
    assert raw == (FIXTURES / case["file"]).read_bytes()


def test_dump_complete(dump_dir: Path):
    assert sorted(p.name for p in dump_dir.iterdir()) == sorted(f"{c['name']}.msgpack" for c in CASES)


def test_batch_decode_keeps_element_order(dump_dir: Path):
    raw = (dump_dir / "batch_tokenizer_n.msgpack").read_bytes()
    batch = BaseTokenizerMsg.decoder(msgpack.unpackb(raw, raw=False))
    entries = msgpack.unpackb(raw, raw=False)["data"]
    assert [m.uid for m in batch.data] == [e["uid"] for e in entries]
    assert len(batch.data) > 1


def test_upstream_decoder_rejects_extra_key():
    # Negative control: the oracle checks the exact key set, so a passing decode is not leniency.
    with pytest.raises(TypeError):
        BaseBackendMsg.decoder({"__type__": "AbortBackendMsg", "uid": 7, "extra": 1})
