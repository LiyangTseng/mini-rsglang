"""The `stress` run part (ROADMAP criterion 4, D-11/D-12): re-runs Phase 5's
128-request cancellation stress tool against a fresh, tapped session for
each configured abort timing, surrounded by D-12's separate process-health
watcher, and reduces the result to a single evidence-backed failure_mode
per run via abort_analysis.

Phase 5's stress tool and the frontends are invoked only through command
templates and never modified (D-11). Bug detection lives here and in the
backend tap, never in the stress tool itself (D-12).
"""

from __future__ import annotations

import asyncio
import json
import os
import re
import shlex
import signal
import subprocess
import time
from pathlib import Path
from typing import Any, Sequence

from .. import handshake
from ..profiling import procs
from . import abort_analysis, sweep, tap

_SCHEDULER_PID_RE = re.compile(r"spawned scheduler rank=0 pid=(\d+)")

_WATCH_RE = re.compile(
    r"WATCH summary pid=(?P<pid>\d+) samples=(?P<samples>\d+) crashed=(?P<crashed>\d+) "
    r"zombie=(?P<zombie>\d+) restarts=(?P<restarts>\d+) gpu_unlisted=(?P<gpu_unlisted>\d+) "
    r"nvsmi_errors=(?P<nvsmi_errors>\d+) verdict=(?P<verdict>\w+)"
)

_UNKNOWN_WATCH: "dict[str, Any]" = {
    "pid": None,
    "samples": 0,
    "crashed": 0,
    "zombie": 0,
    "restarts": 0,
    "gpu_unlisted": 0,
    "nvsmi_errors": 0,
    "verdict": "unknown",
}


class StressSetupError(RuntimeError):
    """Raised when a stress run's session comes up but the scheduler pid
    cannot be found, or the stress/watcher setup otherwise cannot start.
    Caught by run_stress_part and turned into a failure_mode='setup_failed'
    run, rather than aborting the whole stress part."""


def _poll_scheduler_pid(log_path: Path, timeout_s: float = 30.0) -> int:
    deadline = time.monotonic() + timeout_s
    while True:
        try:
            text = Path(log_path).read_text(encoding="utf-8", errors="replace")
        except OSError:
            text = ""
        m = _SCHEDULER_PID_RE.search(text)
        if m:
            return int(m.group(1))
        if time.monotonic() > deadline:
            raise StressSetupError(
                f"no 'spawned scheduler rank=0 pid=' line in {log_path} after {timeout_s:g}s"
            )
        time.sleep(0.2)


def _start_watcher(pid: int, *, interval_s: float, out_path: Path, launcher_log: Path) -> subprocess.Popen:
    script = handshake.repo_root() / "scripts" / "gpu_phase6_watch.sh"
    argv = [
        "bash",
        str(script),
        "--pid",
        str(pid),
        "--interval",
        str(interval_s),
        "--out",
        str(out_path),
        "--launcher-log",
        str(launcher_log),
    ]
    return subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def _parse_watch_summary(line: "str | None") -> "dict[str, Any]":
    if line is None:
        return dict(_UNKNOWN_WATCH)
    m = _WATCH_RE.search(line)
    if not m:
        return dict(_UNKNOWN_WATCH)
    d = m.groupdict()
    return {
        "pid": int(d["pid"]),
        "samples": int(d["samples"]),
        "crashed": int(d["crashed"]),
        "zombie": int(d["zombie"]),
        "restarts": int(d["restarts"]),
        "gpu_unlisted": int(d["gpu_unlisted"]),
        "nvsmi_errors": int(d["nvsmi_errors"]),
        "verdict": d["verdict"],
    }


def _stop_watcher(proc: subprocess.Popen, out_path: Path, *, timeout_s: float = 10.0) -> "dict[str, Any]":
    try:
        proc.send_signal(signal.SIGTERM)
    except ProcessLookupError:
        pass

    stdout_text = ""
    try:
        stdout_text, _ = proc.communicate(timeout=timeout_s)
    except subprocess.TimeoutExpired:
        proc.kill()
        try:
            stdout_text, _ = proc.communicate(timeout=5.0)
        except subprocess.TimeoutExpired:
            stdout_text = ""

    summary_line = None
    for line in (stdout_text or "").splitlines():
        if line.startswith("WATCH summary"):
            summary_line = line
    if summary_line is None and Path(out_path).exists():
        try:
            text = Path(out_path).read_text(encoding="utf-8", errors="replace")
        except OSError:
            text = ""
        for line in text.splitlines():
            if line.startswith("WATCH summary"):
                summary_line = line

    return _parse_watch_summary(summary_line)


def _run_stress_cmd(argv: "list[str]", *, log_path: Path, timeout_s: float) -> "tuple[int | None, bool]":
    log_path = Path(log_path)
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with open(log_path, "ab") as log_file:
        proc = subprocess.Popen(argv, stdout=log_file, stderr=log_file, start_new_session=True)
    try:
        rc = proc.wait(timeout=timeout_s)
        return rc, False
    except subprocess.TimeoutExpired:
        pass

    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        pass
    try:
        proc.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
        try:
            proc.wait(timeout=5.0)
        except subprocess.TimeoutExpired:
            pass
    return proc.returncode, True


def _tail(path: Path, n: int) -> str:
    try:
        text = Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""
    lines = text.splitlines()
    return "\n".join(lines[-n:])


def _first_integrity_error(log_path: Path) -> "str | None":
    try:
        text = Path(log_path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return None
    for line in text.splitlines():
        if "RuntimeError" in line or "check_integrity" in line:
            return line
    return None


async def _canary(base_url: str, model: str, timeout_s: float) -> bool:
    import aiohttp

    payload = {
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
        "temperature": 0.0,
        "top_k": -1,
        "top_p": 1.0,
        "max_tokens": 8,
        "stream": False,
    }
    timeout = aiohttp.ClientTimeout(total=timeout_s)
    try:
        async with aiohttp.ClientSession(timeout=timeout) as session:
            async with session.post(f"{base_url}/v1/chat/completions", json=payload) as resp:
                if resp.status != 200:
                    return False
                body = json.loads(await resp.text())
                content = body["choices"][0]["message"]["content"]
                return bool(content)
    except (asyncio.TimeoutError, aiohttp.ClientError, json.JSONDecodeError, KeyError, IndexError, TypeError):
        return False


def _setup_failed_run(timing: str, exc: Exception) -> "dict[str, Any]":
    return {
        "abort_timing": timing,
        "stress_rc": None,
        "stress_timed_out": False,
        "stress_output_tail": str(exc),
        "canary_ok": False,
        "watch": dict(_UNKNOWN_WATCH),
        "integrity_error": None,
        "analysis": abort_analysis.analyze([]),
        "failure_mode": "setup_failed",
    }


def _format_argv(template: str, **kwargs: Any) -> "list[str]":
    return shlex.split(template.format(**kwargs))


async def _run_one_timing(
    base_url: str,
    ctx: sweep.SessionContext,
    *,
    timing: str,
    model: str,
    stress_cmd: str,
    port: int,
    python: str,
    stress_timeout_s: float,
    settle_s: float,
    canary_timeout_s: float,
    watch_interval_s: float,
    work_dir: Path,
) -> "dict[str, Any]":
    scheduler_pid = _poll_scheduler_pid(ctx.log_path)

    watch_out = work_dir / f"watch-{timing}.log"
    watcher = _start_watcher(
        scheduler_pid, interval_s=watch_interval_s, out_path=watch_out, launcher_log=ctx.log_path
    )

    stress_argv = _format_argv(stress_cmd, python=python, base_url=base_url, port=port, model=model)
    stress_log = work_dir / f"stress-{timing}.log"
    stress_rc, stress_timed_out = _run_stress_cmd(stress_argv, log_path=stress_log, timeout_s=stress_timeout_s)

    await asyncio.sleep(settle_s)
    canary_ok = await _canary(base_url, model, canary_timeout_s)

    watch = _stop_watcher(watcher, watch_out)

    tap_records = tap.load_tap_records(ctx.tap_dir).records
    integrity_error = _first_integrity_error(ctx.log_path)
    analysis = abort_analysis.analyze(tap_records)
    stress_output_tail = _tail(stress_log, 40)

    run: "dict[str, Any]" = {
        "abort_timing": timing,
        "stress_rc": stress_rc,
        "stress_timed_out": stress_timed_out,
        "stress_output_tail": stress_output_tail,
        "canary_ok": canary_ok,
        "watch": watch,
        "integrity_error": integrity_error,
        "analysis": analysis,
    }
    run["failure_mode"] = abort_analysis.failure_mode(run)
    return run


def run_stress_part(
    *,
    model: str,
    stress_server_cmd: str,
    stress_cmd: str,
    abort_timings: "Sequence[str]",
    port: int,
    session_timeout_s: float,
    stress_timeout_s: float,
    settle_s: float,
    canary_timeout_s: float,
    watch_interval_s: float,
    work_dir: Path,
    python: str,
) -> "dict[str, Any]":
    """Runs the stress tool once per abort timing, each against a fresh
    tapped session, and reduces the result to the abort_stress sidecar
    block: {model, runs, probe, reproduced, conclusive}. `probe` is always
    None here -- probe.py (Task 3) fills it in."""
    runs: "list[dict[str, Any]]" = []

    for timing in abort_timings:
        if not sweep.port_free(port):
            runs.append(_setup_failed_run(timing, StressSetupError(f"port {port} in use")))
            continue

        argv = _format_argv(stress_server_cmd, python=python, model=model, port=port, abort_timing=timing)

        def _make_workload(_timing: str):
            async def workload(base_url: str, ctx: sweep.SessionContext) -> "dict[str, Any]":
                return await _run_one_timing(
                    base_url,
                    ctx,
                    timing=_timing,
                    model=model,
                    stress_cmd=stress_cmd,
                    port=port,
                    python=python,
                    stress_timeout_s=stress_timeout_s,
                    settle_s=settle_s,
                    canary_timeout_s=canary_timeout_s,
                    watch_interval_s=watch_interval_s,
                    work_dir=work_dir,
                )

            return workload

        try:
            session_result = sweep.run_session(
                f"stress-{timing}",
                argv=argv,
                port=port,
                timeout_s=session_timeout_s,
                work_dir=work_dir,
                workload=_make_workload(timing),
                pass_context=True,
            )
            run = session_result.value
        except (StressSetupError, procs.ServerExited, TimeoutError) as exc:
            run = _setup_failed_run(timing, exc)

        runs.append(run)

    immediate_run = next((r for r in runs if r["abort_timing"] == "immediate"), None)
    reproduced = immediate_run is not None and immediate_run["failure_mode"] != "none"
    conclusive = (
        immediate_run is not None
        and immediate_run["analysis"]["aborts_by_class"]["prefill_window"] > 0
    )

    return {
        "model": model,
        "runs": runs,
        "probe": None,
        "reproduced": reproduced,
        "conclusive": conclusive,
    }
