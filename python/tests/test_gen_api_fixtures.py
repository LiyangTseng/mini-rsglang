"""Tests for scripts/gen_api_fixtures.py (05-05 Task 2): normalization, case-table invariants,
and the committed-fixtures freshness check.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO / "scripts" / "gen_api_fixtures.py"


def _load_module():
    spec = importlib.util.spec_from_file_location("gen_api_fixtures", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.fixture(scope="module")
def gen_api_fixtures():
    return _load_module()


def test_normalize_created_replaces_exactly_one(gen_api_fixtures):
    normalize_created = gen_api_fixtures.normalize_created

    body, original = normalize_created(b'{"a":1,"created":1790000000,"b":2}')
    assert body == b'{"a":1,"created":0,"b":2}'
    assert original == 1790000000

    with pytest.raises(ValueError):
        normalize_created(b'{"a":1,"b":2}')  # zero occurrences

    with pytest.raises(ValueError):
        normalize_created(b'{"created":1,"created":2}')  # two occurrences


def test_case_table_invariants(gen_api_fixtures):
    cases = gen_api_fixtures.CASES
    names = [c["name"] for c in cases]

    assert len(names) == 18, names
    assert len(names) == len(set(names)), names

    for case in cases:
        assert case["compare"] in ("bytes", "status"), case

    uid_cases = [c for c in cases if c["uid"] is not None]
    assert [c["uid"] for c in uid_cases] == list(range(len(uid_cases))), uid_cases

    by_name = {c["name"]: c for c in cases}
    for name, fields in gen_api_fixtures.NORMALIZED_FIELDS.items():
        assert fields == ("created",), (name, fields)
        assert name in by_name, name
        assert by_name[name]["compare"] == "bytes", name

    status_only = {c["name"] for c in cases if c["compare"] == "status"}
    assert status_only == {"generate_missing_max_tokens", "chat_bad_role", "chat_missing_prompt"}


@pytest.mark.slow
def test_committed_fixtures_are_fresh(gen_api_fixtures):
    assert gen_api_fixtures.main(["--check"]) == 0
