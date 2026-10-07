"""Tap behavior tests against the real vendored Scheduler class (PAR-01/PAR-02).

Proves the env-gated in-scheduler instrumentation added in this plan's
Task 2: the wrapped methods record, then always delegate to the real
vendored logic unchanged, and are off by construction unless
RSGLANG_PARITY_TAP_DIR is set.
"""

from __future__ import annotations

import ast
import os
import subprocess
import sys
import types
from pathlib import Path

import pytest
import torch

import minisgl.scheduler.io as io_module
import minisgl.scheduler.scheduler as scheduler_module
from minisgl.core import SamplingParams
from minisgl.message import AbortBackendMsg, BatchTokenizerMsg, DetokenizeMsg, UserMsg
from minisgl.scheduler.table import TableManager

from rsglang.parity import tap

REPO_ROOT = Path(__file__).resolve().parents[2]
VENDOR_MINISGL = REPO_ROOT / "vendor" / "mini-sglang" / "python" / "minisgl"


# --- test_vendored_tap_targets_exist (AST, no import) -----------------------------


def _parse(path: Path) -> ast.Module:
    return ast.parse(path.read_text(encoding="utf-8"), filename=str(path))


def _find_class(tree: ast.Module, name: str) -> ast.ClassDef:
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef) and node.name == name:
            return node
    raise AssertionError(f"class {name!r} not found")


def _find_method(class_node: ast.ClassDef, name: str) -> ast.FunctionDef:
    for node in class_node.body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return node
    raise AssertionError(f"method {name!r} not found on {class_node.name}")


def test_vendored_tap_targets_exist():
    scheduler_path = VENDOR_MINISGL / "scheduler" / "scheduler.py"
    io_path = VENDOR_MINISGL / "scheduler" / "io.py"
    prefill_path = VENDOR_MINISGL / "scheduler" / "prefill.py"
    decode_path = VENDOR_MINISGL / "scheduler" / "decode.py"
    table_path = VENDOR_MINISGL / "scheduler" / "table.py"

    scheduler_cls = _find_class(_parse(scheduler_path), "Scheduler")
    for name in ("_process_one_msg", "_free_req_resources", "_prepare_batch"):
        method = _find_method(scheduler_cls, name)
        assert method.args.args[0].arg == "self"

    io_cls = _find_class(_parse(io_path), "SchedulerIOMixin")
    reply_method = _find_method(io_cls, "_reply_tokenizer_rank0")
    assert reply_method.args.args[0].arg == "self"

    init_method = _find_method(io_cls, "__init__")
    init_source = ast.get_source_segment(io_path.read_text(encoding="utf-8"), init_method)
    assert init_source is not None and "send = self._reply_tokenizer_rank0" in init_source

    prefill_cls = _find_class(_parse(prefill_path), "PrefillManager")
    assert any(
        isinstance(node, ast.AnnAssign) and getattr(node.target, "id", None) == "pending_list"
        for node in prefill_cls.body
    )

    decode_cls = _find_class(_parse(decode_path), "DecodeManager")
    assert any(
        isinstance(node, ast.AnnAssign) and getattr(node.target, "id", None) == "running_reqs"
        for node in decode_cls.body
    )

    table_cls = _find_class(_parse(table_path), "TableManager")
    table_init = _find_method(table_cls, "__init__")
    assigns_free_slots = any(
        isinstance(node, ast.Assign)
        and any(isinstance(t, ast.Attribute) and t.attr == "_free_slots" for t in node.targets)
        for node in ast.walk(table_init)
    )
    assert assigns_free_slots


# --- test_install_noop_without_env ------------------------------------------------


def test_install_noop_without_env(monkeypatch):
    monkeypatch.delenv(tap.TAP_DIR_ENV, raising=False)
    tap._reset_for_tests()
    before = list(sys.meta_path)

    result = tap.install()

    assert result is False
    assert sys.meta_path == before


# --- test_patch_real_scheduler_methods --------------------------------------------


def test_patch_real_scheduler_methods(tmp_path, monkeypatch):
    monkeypatch.setenv(tap.TAP_DIR_ENV, str(tmp_path))
    tap._reset_for_tests()

    tap.patch(scheduler_module)

    for fn in (
        scheduler_module.Scheduler._process_one_msg,
        scheduler_module.Scheduler._free_req_resources,
        scheduler_module.Scheduler._prepare_batch,
        io_module.SchedulerIOMixin._reply_tokenizer_rank0,
    ):
        assert getattr(fn, "__rsglang_tap__", False) is True

    # UserMsg: a user record is written, and add_one_req still receives msg.
    added: list = []
    self_user = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    self_user.engine = types.SimpleNamespace(max_seq_len=4096)
    self_user.prefill_manager = types.SimpleNamespace(
        pending_list=[], add_one_req=lambda msg: added.append(msg)
    )
    user_msg = UserMsg(
        uid=7,
        input_ids=torch.tensor([1, 2, 3], dtype=torch.int32),
        sampling_params=SamplingParams(temperature=0.0, max_tokens=5),
    )
    scheduler_module.Scheduler._process_one_msg(self_user, user_msg)
    assert added == [user_msg]

    # AbortBackendMsg: uid 7 is only in running_reqs -> in_running True, in_pending False.
    self_abort = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    self_abort.prefill_manager = types.SimpleNamespace(pending_list=[], abort_req=lambda uid: None)
    running_req = types.SimpleNamespace(uid=7)
    self_abort.decode_manager = types.SimpleNamespace(
        running_reqs={running_req}, abort_req=lambda uid: None
    )
    scheduler_module.Scheduler._process_one_msg(self_abort, AbortBackendMsg(uid=7))

    # _free_req_resources called twice on the same (uid, table_idx), real TableManager.
    cache_calls: list = []
    self_free = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    table_manager = TableManager(max_running_reqs=4, page_table=torch.zeros((4, 8), dtype=torch.int32))
    table_manager._free_slots.remove(2)  # simulate table_idx 2 already allocated to this req
    self_free.table_manager = table_manager
    self_free.cache_manager = types.SimpleNamespace(
        cache_req=lambda req, finished: cache_calls.append((req, finished))
    )
    free_req = types.SimpleNamespace(uid=7, table_idx=2)
    scheduler_module.Scheduler._free_req_resources(self_free, free_req)
    scheduler_module.Scheduler._free_req_resources(self_free, free_req)
    assert cache_calls == [(free_req, True), (free_req, True)]

    # _reply_tokenizer_rank0 with two DetokenizeMsg -> one BatchTokenizerMsg still put.
    sent: list = []
    self_reply = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    self_reply._send_into_tokenizer = types.SimpleNamespace(put=lambda msg: sent.append(msg))
    detok_msgs = [
        DetokenizeMsg(uid=1, next_token=10, finished=False),
        DetokenizeMsg(uid=1, next_token=11, finished=True),
    ]
    io_module.SchedulerIOMixin._reply_tokenizer_rank0(self_reply, detok_msgs)
    assert len(sent) == 1
    assert isinstance(sent[0], BatchTokenizerMsg)
    assert sent[0].data == detok_msgs

    # _prepare_batch: two reqs sharing table_idx 3 -> one collision record, and
    # the stand-in pad_batch's sentinel exception propagates unchanged.
    class _Sentinel(Exception):
        pass

    def _raise_sentinel(batch):
        raise _Sentinel("pad_batch sentinel")

    self_batch = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    self_batch.engine = types.SimpleNamespace(
        graph_runner=types.SimpleNamespace(pad_batch=_raise_sentinel)
    )
    self_batch.decode_manager = types.SimpleNamespace(running_reqs=set())
    batch_req_a = types.SimpleNamespace(uid=100, table_idx=3)
    batch_req_b = types.SimpleNamespace(uid=101, table_idx=3)
    batch = types.SimpleNamespace(reqs=[batch_req_a, batch_req_b])

    with pytest.raises(_Sentinel):
        scheduler_module.Scheduler._prepare_batch(self_batch, batch)

    records = tap.load_tap_records(tmp_path)
    assert records.malformed_lines == 0

    kinds = [r["kind"] for r in records.records]
    assert kinds.count(tap.KIND_PATCHED) == 1
    assert kinds.count(tap.KIND_USER) == 1
    assert kinds.count(tap.KIND_ABORT) == 1
    assert kinds.count(tap.KIND_FREE) == 2
    assert kinds.count(tap.KIND_DETOK) == 2
    assert kinds.count(tap.KIND_COLLISION) == 1

    patched_record = next(r for r in records.records if r["kind"] == tap.KIND_PATCHED)
    assert set(patched_record["targets"]) == {
        "Scheduler._process_one_msg",
        "Scheduler._free_req_resources",
        "Scheduler._prepare_batch",
        "SchedulerIOMixin._reply_tokenizer_rank0",
    }

    user_record = next(r for r in records.records if r["kind"] == tap.KIND_USER)
    assert user_record["uid"] == 7
    assert user_record["input_ids"] == [1, 2, 3]
    assert user_record["sampling"]["max_tokens"] == 5

    abort_record = next(r for r in records.records if r["kind"] == tap.KIND_ABORT)
    assert abort_record["in_running"] is True
    assert abort_record["in_pending"] is False

    free_records = [r for r in records.records if r["kind"] == tap.KIND_FREE]
    assert free_records[0]["dup_free_slots"] is False
    assert free_records[1]["dup_free_slots"] is True

    collision_record = next(r for r in records.records if r["kind"] == tap.KIND_COLLISION)
    assert collision_record["table_idx"] == 3
    assert collision_record["uids"] == [100, 101]


# --- test_recording_failure_never_raises ------------------------------------------


def test_recording_failure_never_raises(tmp_path, monkeypatch):
    monkeypatch.setenv(tap.TAP_DIR_ENV, str(tmp_path))
    tap._reset_for_tests()
    tap.patch(scheduler_module)

    def _boom(*args, **kwargs):
        raise RuntimeError("boom")

    monkeypatch.setattr(tap, "_write_record", _boom)

    cache_calls: list = []
    self_free = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    table_manager = TableManager(max_running_reqs=4, page_table=torch.zeros((4, 8), dtype=torch.int32))
    self_free.table_manager = table_manager
    self_free.cache_manager = types.SimpleNamespace(
        cache_req=lambda req, finished: cache_calls.append((req, finished))
    )
    req = types.SimpleNamespace(uid=9, table_idx=1)

    result = scheduler_module.Scheduler._free_req_resources(self_free, req)

    assert result is None
    assert cache_calls == [(req, True)]


# --- test_shim_patches_in_fresh_interpreter (slow) --------------------------------

_CHILD_SCRIPT = (
    "import minisgl.scheduler.scheduler as sched_mod\n"
    "import minisgl.scheduler.io as io_mod\n"
    "flags = [\n"
    "    bool(getattr(sched_mod.Scheduler._process_one_msg, '__rsglang_tap__', False)),\n"
    "    bool(getattr(sched_mod.Scheduler._free_req_resources, '__rsglang_tap__', False)),\n"
    "    bool(getattr(sched_mod.Scheduler._prepare_batch, '__rsglang_tap__', False)),\n"
    "    bool(getattr(io_mod.SchedulerIOMixin._reply_tokenizer_rank0, '__rsglang_tap__', False)),\n"
    "]\n"
    "print('WRAPPED=' + ','.join(str(f) for f in flags))\n"
)


@pytest.mark.slow
def test_shim_patches_in_fresh_interpreter(tmp_path):
    work_dir = tmp_path / "w"
    tap_dir = tmp_path / "tapdir"
    env = tap.tap_env(dict(os.environ), work_dir=work_dir, tap_dir=tap_dir)

    result = subprocess.run(
        [sys.executable, "-c", _CHILD_SCRIPT], env=env, timeout=60, capture_output=True, text=True
    )
    assert result.returncode == 0, result.stderr
    assert "WRAPPED=True,True,True,True" in result.stdout, result.stdout

    records = tap.load_tap_records(tap_dir)
    assert records.malformed_lines == 0
    assert any(r["kind"] == tap.KIND_PATCHED for r in records.records)

    env_off = dict(env)
    env_off.pop(tap.TAP_DIR_ENV, None)
    result_off = subprocess.run(
        [sys.executable, "-c", _CHILD_SCRIPT], env=env_off, timeout=60, capture_output=True, text=True
    )
    assert result_off.returncode == 0, result_off.stderr
    assert "WRAPPED=False,False,False,False" in result_off.stdout, result_off.stdout


# --- test_tap_records_load_cleanly -------------------------------------------------


def test_tap_records_load_cleanly(tmp_path, monkeypatch):
    monkeypatch.setenv(tap.TAP_DIR_ENV, str(tmp_path))
    tap._reset_for_tests()
    tap.patch(scheduler_module)

    self_user = scheduler_module.Scheduler.__new__(scheduler_module.Scheduler)
    self_user.engine = types.SimpleNamespace(max_seq_len=4096)
    self_user.prefill_manager = types.SimpleNamespace(pending_list=[], add_one_req=lambda msg: None)
    user_msg = UserMsg(
        uid=1, input_ids=torch.tensor([9], dtype=torch.int32), sampling_params=SamplingParams()
    )
    scheduler_module.Scheduler._process_one_msg(self_user, user_msg)

    records = tap.load_tap_records(tmp_path)
    assert records.malformed_lines == 0
    assert len(records.records) >= 1
