"""Handshake extraction and encoding (D-09, D-11) and scheduler-factory resolution."""

from __future__ import annotations

import re
from types import SimpleNamespace

import pytest

from rsglang import backend
from rsglang.handshake import HANDSHAKE_KEYS, encode_handshake_line, read_upstream_sha, repo_root
from rsglang.testing.fake_scheduler import FACTORY_PATH, FakeScheduler

SHA = "9a91cfafe754aa85daee49998176275667eb58f2"

CONTRACT_PAYLOAD = {
    "handshake_version": 1,
    "upstream_sha": SHA,
    "max_seq_len": 4096,
    "eos_token_id": 151645,
    "page_size": 16,
    "max_running_req": 8,
    "num_pages": 1024,
}
CONTRACT_LINE = (
    b'{"handshake_version":1,"upstream_sha":"9a91cfafe754aa85daee49998176275667eb58f2",'
    b'"max_seq_len":4096,"eos_token_id":151645,"page_size":16,"max_running_req":8,"num_pages":1024}\n'
)


def _fakes(eos=151645):
    scheduler = SimpleNamespace(
        engine=SimpleNamespace(max_seq_len=40960, num_pages=5000),
        eos_token_id=eos,
        cache_manager=SimpleNamespace(page_size=64),
    )
    # args deliberately disagree with the scheduler: the scheduler's values must win.
    args = SimpleNamespace(max_running_req=256, page_size=1, max_seq_len=99999)
    return scheduler, args


def test_extract_handshake_reads_post_init_scheduler_values():
    scheduler, args = _fakes()
    payload = backend.extract_handshake(scheduler, args, SHA)
    assert tuple(payload) == HANDSHAKE_KEYS
    assert payload["page_size"] == 64  # cache_manager, not args.page_size (Pitfall 2)
    assert payload["max_seq_len"] == 40960  # engine, not args.max_seq_len
    assert payload["num_pages"] == 5000
    assert payload["eos_token_id"] == 151645
    assert payload["max_running_req"] == 256
    assert payload["upstream_sha"] == SHA
    assert payload["handshake_version"] == 1


def test_eos_none_encodes_as_json_null():
    scheduler, args = _fakes(eos=None)
    line = encode_handshake_line(backend.extract_handshake(scheduler, args, SHA))
    assert b'"eos_token_id":null' in line


def test_encode_contract_bytes_exact():
    assert encode_handshake_line(dict(CONTRACT_PAYLOAD)) == CONTRACT_LINE


def test_encode_rejects_extra_key():
    with pytest.raises(ValueError):
        encode_handshake_line({**CONTRACT_PAYLOAD, "extra": 1})


def test_encode_rejects_missing_key():
    payload = dict(CONTRACT_PAYLOAD)
    del payload["num_pages"]
    with pytest.raises(ValueError):
        encode_handshake_line(payload)


def test_encode_rejects_reordered_keys():
    payload = dict(reversed(list(CONTRACT_PAYLOAD.items())))
    with pytest.raises(ValueError):
        encode_handshake_line(payload)


def test_read_upstream_sha_matches_vendor_file():
    sha = read_upstream_sha()
    assert sha == (repo_root() / "vendor" / "UPSTREAM_SHA").read_text().strip()
    assert re.fullmatch(r"[0-9a-f]{40}", sha)


def test_read_upstream_sha_rejects_garbage(tmp_path):
    bad = tmp_path / "UPSTREAM_SHA"
    bad.write_text("xyz\n")
    with pytest.raises(ValueError):
        read_upstream_sha(bad)


def test_default_factory_is_upstream_scheduler(monkeypatch):
    monkeypatch.delenv(backend.SCHEDULER_FACTORY_ENV, raising=False)
    factory = backend.resolve_scheduler_factory()
    assert factory.__name__ == "Scheduler"
    assert factory.__module__.startswith("minisgl.scheduler")


def test_env_factory_selects_fake(monkeypatch):
    monkeypatch.setenv(backend.SCHEDULER_FACTORY_ENV, FACTORY_PATH)
    assert backend.resolve_scheduler_factory() is FakeScheduler
