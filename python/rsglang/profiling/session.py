"""Per-scenario measurement orchestration for BENCH-01 (D-11/D-14): wires
hook.py (D-02/D-03), procs.py (D-01 launch/discovery/teardown), analysis.py
(every metric) and scenarios.py (the three workload drivers) into exactly
one sidecar scenario entry per run, matching sidecar.py's schema.

Standard library plus psutil and the sibling profiling modules.
"""

from __future__ import annotations

import asyncio
import os
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Awaitable, Callable, Mapping, Sequence

import psutil

from . import analysis, hook, procs, scenarios, sidecar

ROLES = sidecar.ROLES  # ("api_server", "scheduler", "tokenizer")


class MeasurementError(RuntimeError):
    pass


def _tail_log(path: Path, n: int = 40) -> str:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    lines = text.splitlines()
    return "\n".join(lines[-n:])


class PySpyRecorder:
    """Starts/stops one py-spy record subprocess per (role, kind).

    kind is "active" (plain --nonblocking sampling) or "gil" (adds --gil).
    Every recorder in this instance is stopped by stop_all() before it ever
    raises, so a missing/invalid output from one recorder never leaves a
    sibling recorder running (T-02-16).
    """

    def __init__(self, base: Sequence[str], rate_hz: int, out_dir: Path):
        self._base = list(base)
        self._rate_hz = rate_hz
        self._out_dir = Path(out_dir)
        self._out_dir.mkdir(parents=True, exist_ok=True)
        self._procs: dict[tuple[str, str], subprocess.Popen] = {}
        self._starts: dict[tuple[str, str], float] = {}
        self._log_paths: dict[tuple[str, str], Path] = {}
        self._out_paths: dict[tuple[str, str], Path] = {}

    def start(self, role: str, pid: int, kind: str) -> None:
        out_path = self._out_dir / f"{role}.{kind}.speedscope.json"
        log_path = self._out_dir / f"{role}.{kind}.log"
        argv = [
            *self._base,
            "record",
            "--pid", str(pid),
            "--rate", str(self._rate_hz),
            "--format", "speedscope",
            "--output", str(out_path),
            "--nonblocking",
        ]
        if kind == "gil":
            argv.append("--gil")
        log_file = open(log_path, "ab")
        try:
            proc = subprocess.Popen(argv, stdout=log_file, stderr=log_file)
        finally:
            log_file.close()
        key = (role, kind)
        self._procs[key] = proc
        self._starts[key] = time.perf_counter()
        self._log_paths[key] = log_path
        self._out_paths[key] = out_path

    def stop_all(self, timeout_s: float = 30.0) -> dict:
        stop_t = time.perf_counter()

        for proc in self._procs.values():
            try:
                proc.send_signal(signal.SIGINT)
            except ProcessLookupError:
                pass

        for proc in self._procs.values():
            try:
                proc.wait(timeout=timeout_s)
            except subprocess.TimeoutExpired:
                proc.kill()
                try:
                    proc.wait(timeout=10.0)
                except subprocess.TimeoutExpired:
                    pass

        results: dict[tuple[str, str], tuple[dict, float]] = {}
        errors: list[str] = []
        for key, proc in self._procs.items():
            window_s = stop_t - self._starts[key]
            try:
                doc = analysis.load_speedscope(self._out_paths[key])
            except (OSError, analysis.SpeedscopeError) as exc:
                tail = _tail_log(self._log_paths[key], 40)
                errors.append(f"{key[0]}.{key[1]}: {exc}; log tail:\n{tail}")
                continue
            results[key] = (doc, window_s)

        if errors:
            raise MeasurementError("; ".join(errors))
        return results


class RssSampler:
    """Samples psutil.Process(pid).memory_info().rss per role on a daemon
    thread, skipping a vanished pid rather than raising."""

    def __init__(self, pids_by_role: Mapping[str, int], interval_s: float):
        self._pids_by_role = dict(pids_by_role)
        self._interval_s = interval_s
        self._samples: dict[str, list[tuple[float, int]]] = {
            role: [] for role in self._pids_by_role
        }
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    def _run(self) -> None:
        while not self._stop_event.is_set():
            for role, pid in self._pids_by_role.items():
                try:
                    rss = psutil.Process(pid).memory_info().rss
                except (psutil.NoSuchProcess, psutil.ZombieProcess, psutil.AccessDenied):
                    continue
                self._samples[role].append((time.perf_counter(), rss))
            self._stop_event.wait(self._interval_s)

    def start(self) -> None:
        self._thread = threading.Thread(
            target=self._run, name="rsglang-rss-sampler", daemon=True
        )
        self._thread.start()

    def stop(self) -> dict:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=max(5.0, self._interval_s * 3))
        return self._samples


def _gc_records_for_pid(hook_records, pid: int, *, t0: float, t1: float) -> list:
    pid_recs = hook_records.by_pid.get(pid, [])
    gc_recs = [r for r in pid_recs if r.get("kind") == "gc"]
    return analysis.slice_window(gc_recs, t0, t1)


def _mem_records_for_pid(hook_records, pid: int, *, t0: float, t1: float) -> list:
    pid_recs = hook_records.by_pid.get(pid, [])
    mem_recs = [r for r in pid_recs if r.get("kind") == "mem"]
    return analysis.slice_window(mem_recs, t0, t1)


def _latest_alloc_sites(hook_records, pid: int, *, tag: str) -> "list | None":
    pid_recs = hook_records.by_pid.get(pid, [])
    alloc_recs = [
        r for r in pid_recs if r.get("kind") == "alloc_top" and r.get("tag") == tag
    ]
    if not alloc_recs:
        return None
    return alloc_recs[-1].get("sites")


def build_scenario_entry(
    *,
    name: str,
    handle: procs.ServerHandle,
    role_pid: Mapping[str, int],
    other_pids: Sequence[int],
    records: Sequence[Any],
    t0: float,
    t1: float,
    t_ready: float,
    include_boot: bool,
    hook_records,
    rss_samples_by_role: Mapping[str, Sequence[tuple[float, int]]],
    pyspy_results: Mapping[tuple[str, str], tuple[dict, float]],
    rate_hz: int,
    tree_ready: Mapping[str, Any],
    tree_end: Mapping[str, Any],
    params: Mapping[str, Any],
    session_dir: Path,
    clock_mismatch: bool,
) -> dict:
    """Assemble exactly the 02-04 sidecar scenario-entry schema from this
    session's raw measurements."""
    w0 = handle.t_launch if include_boot else t0
    window_s = t1 - t0

    requests = analysis.summarize_requests(records, window_s)

    gc_by_role: dict[str, dict] = {}
    gc_events_by_role: dict[str, list] = {}
    for role, pid in role_pid.items():
        events = _gc_records_for_pid(hook_records, pid, t0=w0, t1=t1)
        gc_events_by_role[role] = events
        gc_by_role[role] = analysis.gc_stats(events, t0=w0)

    if clock_mismatch:
        gc_ttft_correlation = {"frontend": None, "scheduler": None}
    else:
        frontend_events = gc_events_by_role["api_server"] + gc_events_by_role["tokenizer"]
        gc_ttft_correlation = {
            "frontend": analysis.gc_ttft_correlation(records, frontend_events),
            "scheduler": analysis.gc_ttft_correlation(records, gc_events_by_role["scheduler"]),
        }

    memory_per_role: dict[str, dict] = {}
    for role, pid in role_pid.items():
        rss_samples = list(rss_samples_by_role.get(role, []))
        mem_records = _mem_records_for_pid(hook_records, pid, t0=w0, t1=t1)
        top_alloc_sites = _latest_alloc_sites(hook_records, pid, tag=name)
        memory_per_role[role] = analysis.memory_role_summary(
            rss_samples, mem_records, top_alloc_sites, t0=w0
        )

    memory = {
        "per_role": memory_per_role,
        "rss_tree_bytes": {"ready": tree_ready["rss_bytes"], "end": tree_end["rss_bytes"]},
        "pss_tree_bytes": {"ready": tree_ready["pss_bytes"], "end": tree_end["pss_bytes"]},
    }

    cpu_by_role: dict[str, dict] = {}
    for role in role_pid:
        active_doc, active_window_s = pyspy_results[(role, "active")]
        gil_doc, _gil_window_s = pyspy_results[(role, "gil")]
        cpu_by_role[role] = analysis.cpu_metrics(
            active_doc,
            gil_doc,
            rate_hz=rate_hz,
            window_s=active_window_s,
            requests_completed=requests["completed"],
        )

    scheduler_active_doc, _ = pyspy_results[("scheduler", "active")]
    radix = analysis.radix_share(scheduler_active_doc)

    processes = {
        "api_server": role_pid["api_server"],
        "scheduler": role_pid["scheduler"],
        "tokenizer": role_pid["tokenizer"],
        "other": list(other_pids),
    }

    params_out = dict(params)
    if include_boot:
        params_out["instrumented_ready_s"] = t_ready - handle.t_launch

    return {
        "params": params_out,
        "processes": processes,
        "window_s": window_s,
        "requests": requests,
        "gc": gc_by_role,
        "gc_ttft_correlation": gc_ttft_correlation,
        "memory": memory,
        "cpu": cpu_by_role,
        "radix": radix,
        "artifacts": {"work_dir": str(session_dir)},
    }


def run_session(
    name: str,
    *,
    argv: Sequence[str],
    port: int,
    timeout_s: float,
    work_dir: Path,
    py_spy: Sequence[str],
    rate_hz: int,
    interval_s: float,
    workload: Callable[[str], Awaitable[Sequence[Any]]],
    include_boot: bool = False,
    min_sample_s: float = 0.0,
    params: Mapping[str, Any],
) -> tuple[dict, list[str]]:
    """Launch a fresh server tree, measure one scenario's workload, and
    return (scenario_entry, warnings). Always tears the tree down, even on
    failure; a process surviving teardown raises MeasurementError."""
    work_dir = Path(work_dir)
    session_dir = work_dir / name
    shim_dir = session_dir / "shim"
    profile_dir = session_dir / "hook"
    pyspy_dir = session_dir / "pyspy"

    warnings: list[str] = []

    hook.write_shim(shim_dir)
    env = hook.hook_env(
        os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=interval_s
    )
    log_path = session_dir / "server.log"
    handle = procs.launch_server(list(argv), env=env, log_path=log_path)

    children: list[int] = []
    recorder = PySpyRecorder(py_spy, rate_hz, pyspy_dir)
    role_pid: dict[str, int] = {}
    other_pids: list[int] = []
    tree_ready: dict = {}
    tree_end: dict = {}
    t_ready = handle.t_launch
    t0 = t1 = handle.t_launch
    records: Sequence[Any] = []
    rss_samples_by_role: dict = {}
    pyspy_results: dict = {}

    try:
        t_ready = procs.wait_ready(handle, port=port, timeout_s=timeout_s)
        children = procs.discover_children(handle.proc.pid)

        dumps: dict[int, str] = {}
        for pid in children:
            dumps[pid] = procs.py_spy_dump(pid, base=py_spy)
        roles = procs.identify_roles(handle.proc.pid, dumps)
        role_pid = {
            "api_server": roles["api_server"],
            "scheduler": roles["scheduler"],
            "tokenizer": roles["tokenizer"],
        }
        other_pids = list(roles["other"])

        tree_ready = procs.tree_memory(handle.proc.pid)

        sampler = RssSampler(role_pid, interval_s)
        sampler.start()
        for role, pid in role_pid.items():
            recorder.start(role, pid, "active")
            recorder.start(role, pid, "gil")

        try:
            time.sleep(1.0)

            t0 = time.perf_counter()
            records = asyncio.run(workload(f"http://127.0.0.1:{port}"))
            elapsed = time.perf_counter() - t0
            if elapsed < min_sample_s:
                time.sleep(min_sample_s - elapsed)
            t1 = time.perf_counter()

            hook.request_snapshot(profile_dir, name)
            snapshot_deadline = time.perf_counter() + max(5.0, 3 * interval_s)
            pending_roles = set(role_pid)
            while pending_roles and time.perf_counter() < snapshot_deadline:
                snap_recs = hook.load_hook_records(profile_dir)
                for role in list(pending_roles):
                    pid = role_pid[role]
                    if any(
                        r.get("kind") == "alloc_top" and r.get("tag") == name
                        for r in snap_recs.by_pid.get(pid, [])
                    ):
                        pending_roles.discard(role)
                if pending_roles:
                    time.sleep(0.2)
            for role in pending_roles:
                warnings.append(f"{name}: no allocation snapshot from {role}")
        finally:
            rss_samples_by_role = sampler.stop()
            pyspy_results = recorder.stop_all()

        tree_end = procs.tree_memory(handle.proc.pid)
    finally:
        survivors = procs.teardown(handle, extra_pids=children)
        if survivors:
            raise MeasurementError(
                f"{name}: process(es) survived teardown: {survivors}"
            )

    hook_records = hook.load_hook_records(profile_dir)

    for role, pid in role_pid.items():
        recs = hook_records.by_pid.get(pid, [])
        if not any(r.get("kind") == "start" for r in recs):
            raise MeasurementError(f"{name}: hook did not activate in {role}")

    expected_clock = time.get_clock_info("perf_counter").implementation
    clock_mismatch = False
    for role, pid in role_pid.items():
        recs = hook_records.by_pid.get(pid, [])
        start_rec = next((r for r in recs if r.get("kind") == "start"), None)
        if start_rec is not None and start_rec.get("clock") != expected_clock:
            clock_mismatch = True
            warnings.append(
                f"{name}: {role} reports clock {start_rec.get('clock')!r}, "
                f"expected {expected_clock!r}"
            )

    entry = build_scenario_entry(
        name=name,
        handle=handle,
        role_pid=role_pid,
        other_pids=other_pids,
        records=records,
        t0=t0,
        t1=t1,
        t_ready=t_ready,
        include_boot=include_boot,
        hook_records=hook_records,
        rss_samples_by_role=rss_samples_by_role,
        pyspy_results=pyspy_results,
        rate_hz=rate_hz,
        tree_ready=tree_ready,
        tree_end=tree_end,
        params=params,
        session_dir=session_dir,
        clock_mismatch=clock_mismatch,
    )
    return entry, warnings


_REPO_ROOT = Path(__file__).resolve().parents[3]


def run_coldstart(
    *,
    model: str,
    port: int,
    timeout_s: float,
    work_dir: Path,
    py_spy: Sequence[str],
    rate_hz: int,
    interval_s: float,
    hyperfine: str,
    runs: int,
    warmup: int,
    max_tokens: int,
    sample_s: float,
    server_cmd: "str | None",
    argv: Sequence[str],
    params: Mapping[str, Any],
) -> tuple[dict, list[str]]:
    """Scenario 3 (D-08/D-11): hyperfine-timed cold start to readiness (via
    the coldstart-once/coldstart-stop subcommands), then one instrumented
    first-request session whose GC/tracemalloc window starts at launch
    (include_boot=True) so boot-time GC is included and the scheduler is
    py-spy-sampled too. Order matters: hyperfine first, so its warmup run
    absorbs first-touch disk-cache effects before the instrumented session.
    """
    work_dir = Path(work_dir)
    session_dir = work_dir / "s3_coldstart"
    hyperfine_dir = session_dir / "hyperfine"
    hyperfine_dir.mkdir(parents=True, exist_ok=True)

    pgid_file = hyperfine_dir / "pgid"
    record_file = hyperfine_dir / "record.jsonl"
    log_path = hyperfine_dir / "coldstart.log"
    export_json = hyperfine_dir / "hyperfine.json"
    script_path = _REPO_ROOT / "scripts" / "baseline_profile.py"

    once_cmd = [
        sys.executable, str(script_path), "coldstart-once",
        "--model", model,
        "--port", str(port),
        "--timeout", str(timeout_s),
        "--pgid-file", str(pgid_file),
        "--record-file", str(record_file),
        "--log", str(log_path),
    ]
    if server_cmd is not None:
        once_cmd += ["--server-cmd", server_cmd]
    stop_cmd = [
        sys.executable, str(script_path), "coldstart-stop",
        "--pgid-file", str(pgid_file),
        "--record-file", str(record_file),
    ]

    hf_argv = scenarios.hyperfine_argv(
        hyperfine=hyperfine,
        runs=runs,
        warmup=warmup,
        export_json=export_json,
        once_cmd=once_cmd,
        stop_cmd=stop_cmd,
    )

    timeout_total = (runs + warmup) * (timeout_s + 120)
    try:
        result = subprocess.run(hf_argv, capture_output=True, text=True, timeout=timeout_total)
        if result.returncode != 0:
            raise MeasurementError(
                f"s3_coldstart: hyperfine exited {result.returncode}: "
                f"{(result.stderr or result.stdout).strip()}"
            )
    finally:
        if pgid_file.exists():
            scenarios.coldstart_stop(pgid_file=pgid_file, record_file=record_file)

    entry, warnings = run_session(
        "s3_coldstart",
        argv=argv,
        port=port,
        timeout_s=timeout_s,
        work_dir=work_dir,
        py_spy=py_spy,
        rate_hz=rate_hz,
        interval_s=interval_s,
        workload=lambda url: scenarios.run_first_request(url, max_tokens=max_tokens),
        include_boot=True,
        min_sample_s=sample_s,
        params=params,
    )

    entry["coldstart"] = {
        "hyperfine": scenarios.parse_hyperfine_json(export_json),
        **scenarios.read_coldstart_records(record_file, warmup=warmup, runs=runs),
    }
    return entry, warnings
