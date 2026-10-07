"""Classifies abort-stress outcomes from the backend tap (D-08).

`analyze` turns one run's tap records into abort-class counts, late-token,
double-free and collision evidence (RESEARCH.md "Pitfall 2": the predicted
bug is silent corruption, not a crash, so a stress run that only checks
process liveness can false-PASS). `failure_mode` reduces one run's health
plus that evidence to a single, precedence-ordered outcome. `analyze_probe`
(Task 3) restricts the same classification to a backend window probe's own
uids, so the probe's own record of "did the window get hit" is independent
of the frontend's disconnect-detection latency.

Standard library only, aside from rsglang.parity.tap's kind constants
(imported, not copied).
"""

from __future__ import annotations

from typing import Any, Mapping, Sequence

from . import tap

ABORT_CLASSES = ("pending", "pending_chunked", "prefill_window", "decode", "not_found")
FAILURE_MODES = ("crash", "wedge", "corrupted_requests", "double_free", "none", "setup_failed")


class AbortAnalysisError(ValueError):
    """Raised by analyze() when the input records violate a precondition
    (abort records from more than one pid -- analyze() is scoped to a
    single scheduler process's records)."""


def _classify_abort(abort_record: Mapping[str, Any], detok_by_uid: Mapping[int, list]) -> str:
    uid = abort_record.get("uid")
    seq = abort_record.get("seq")
    in_pending = bool(abort_record.get("in_pending"))
    in_running = bool(abort_record.get("in_running"))
    chunked = bool(abort_record.get("chunked"))

    if in_pending and chunked:
        return "pending_chunked"
    if in_pending:
        return "pending"
    if in_running:
        earlier_detok = any(d["seq"] < seq for d in detok_by_uid.get(uid, []))
        return "decode" if earlier_detok else "prefill_window"
    return "not_found"


def analyze(records: "Sequence[Mapping[str, Any]]") -> "dict[str, Any]":
    """Classifies every abort in `records` (one scheduler pid's tap records)
    and summarizes free/double-free/collision evidence. Raises
    AbortAnalysisError if abort records come from more than one pid."""
    user_records = [r for r in records if r["kind"] == tap.KIND_USER]
    abort_records = [r for r in records if r["kind"] == tap.KIND_ABORT]
    free_records = [r for r in records if r["kind"] == tap.KIND_FREE]
    detok_records = [r for r in records if r["kind"] == tap.KIND_DETOK]
    collision_records = [r for r in records if r["kind"] == tap.KIND_COLLISION]

    pids = {r["pid"] for r in abort_records}
    if len(pids) > 1:
        raise AbortAnalysisError(f"abort records come from multiple pids: {sorted(pids)}")

    detok_by_uid: "dict[int, list]" = {}
    for r in detok_records:
        detok_by_uid.setdefault(r["uid"], []).append(r)

    aborts_by_class = {c: 0 for c in ABORT_CLASSES}
    abort_class_by_uid: "dict[int, str]" = {}
    late_tokens_after_abort = 0
    for abort in abort_records:
        cls = _classify_abort(abort, detok_by_uid)
        aborts_by_class[cls] += 1
        abort_class_by_uid[abort["uid"]] = cls
        uid = abort["uid"]
        seq = abort["seq"]
        late_tokens_after_abort += sum(1 for d in detok_by_uid.get(uid, []) if d["seq"] > seq)

    free_by_uid: "dict[int, list]" = {}
    for r in free_records:
        free_by_uid.setdefault(r["uid"], []).append(r)
    double_free_uids = sorted(uid for uid, recs in free_by_uid.items() if len(recs) >= 2)
    double_free_in_prefill_window = sorted(
        uid for uid in double_free_uids if abort_class_by_uid.get(uid) == "prefill_window"
    )

    dup_free_slot_events = sum(1 for r in free_records if r.get("dup_free_slots"))

    collision_uids = sorted({uid for r in collision_records for uid in r.get("uids", [])})[:50]

    return {
        "requests_total": len(user_records),
        "aborts_total": len(abort_records),
        "aborts_by_class": aborts_by_class,
        "late_tokens_after_abort": late_tokens_after_abort,
        "frees_total": len(free_records),
        "double_free_uids": double_free_uids,
        "double_free_in_prefill_window": double_free_in_prefill_window,
        "dup_free_slot_events": dup_free_slot_events,
        "collisions": len(collision_records),
        "collision_uids": collision_uids,
    }


def failure_mode(run: "Mapping[str, Any]") -> str:
    """Reduces one stress run's health + tap evidence to a single outcome,
    in fixed precedence: crash > wedge > corrupted_requests > double_free >
    none. `run` carries (at least) `watch`, `stress_timed_out`, `canary_ok`
    and `analysis` (analyze()'s return value)."""
    watch = run.get("watch") or {}
    if watch.get("crashed") or watch.get("zombie") or (watch.get("restarts") or 0) > 0:
        return "crash"
    if run.get("stress_timed_out") or not run.get("canary_ok"):
        return "wedge"
    analysis = run.get("analysis") or {}
    if (analysis.get("collisions") or 0) > 0:
        return "corrupted_requests"
    if analysis.get("double_free_uids") or (analysis.get("dup_free_slot_events") or 0) > 0:
        return "double_free"
    return "none"


def analyze_probe(records: "Sequence[Mapping[str, Any]]", trials: "Sequence[Mapping[str, Any]]") -> "dict[str, Any]":
    """Restricts analyze()'s classification to the backend window probe's
    own uids (Task 3). `trials` is run_window_probe()'s return value --
    [{uid, delay_ms}, ...]. Records for non-probe uids are ignored."""
    probe_uids = {t["uid"] for t in trials}
    uid_to_delay = {t["uid"]: t["delay_ms"] for t in trials}

    abort_records = [r for r in records if r["kind"] == tap.KIND_ABORT and r.get("uid") in probe_uids]
    detok_records = [r for r in records if r["kind"] == tap.KIND_DETOK and r.get("uid") in probe_uids]
    free_records = [r for r in records if r["kind"] == tap.KIND_FREE and r.get("uid") in probe_uids]
    collision_records = [
        r
        for r in records
        if r["kind"] == tap.KIND_COLLISION and any(u in probe_uids for u in r.get("uids", []))
    ]

    detok_by_uid: "dict[int, list]" = {}
    for r in detok_records:
        detok_by_uid.setdefault(r["uid"], []).append(r)

    free_by_uid: "dict[int, list]" = {}
    for r in free_records:
        free_by_uid.setdefault(r["uid"], []).append(r)
    double_free_uids = {uid for uid, recs in free_by_uid.items() if len(recs) >= 2}

    delays_present = sorted(set(uid_to_delay.values()))
    by_delay_map = {
        d: {
            "delay_ms": d,
            "trials": 0,
            "by_class": {c: 0 for c in ABORT_CLASSES},
            "double_free": 0,
            "collisions": 0,
        }
        for d in delays_present
    }
    for t in trials:
        by_delay_map[t["delay_ms"]]["trials"] += 1

    prefill_window_hits = 0
    for abort in abort_records:
        uid = abort["uid"]
        delay = uid_to_delay.get(uid)
        if delay not in by_delay_map:
            continue
        cls = _classify_abort(abort, detok_by_uid)
        by_delay_map[delay]["by_class"][cls] += 1
        if cls == "prefill_window":
            prefill_window_hits += 1
        if uid in double_free_uids:
            by_delay_map[delay]["double_free"] += 1

    for r in collision_records:
        matching_delays = {uid_to_delay[u] for u in r.get("uids", []) if u in uid_to_delay}
        for delay in matching_delays:
            by_delay_map[delay]["collisions"] += 1

    by_delay = [by_delay_map[d] for d in delays_present]

    return {
        "by_delay": by_delay,
        "prefill_window_hits": prefill_window_hits,
        "double_free_total": len(double_free_uids),
        "collisions_total": len(collision_records),
    }
