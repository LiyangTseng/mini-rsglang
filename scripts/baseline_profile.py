#!/usr/bin/env python3
"""The only writer of docs/benchmarks/baseline-profile.json (D-14): BENCH-01's
profiling driver.

This plan adds the `discover` subcommand: launch the profiled server, discover
and role-identify its children via a one-shot py-spy dump, confirm the 02-02
profiling hook reached every role, and write a validated discover-mode
sidecar. Later Phase 2 plans add the scenario subcommands that write under
the sidecar's "scenarios" key.

Dependency footprint: standard library plus psutil and aiohttp, from the
project's own venv -- this deliberately departs from scripts/check_upstream.py's
stdlib-only rule, because this script runs only after that venv exists. The
real py-spy and hyperfine binaries are required on the GPU box; the Mac uses
rsglang.testing.fake_profile_env's stand-ins for both.

Never writes under vendor/.

Exit codes: 0 OK, 1 measurement failure (server exited before ready, not ready
within --timeout, role identification failed, the profiling hook did not
activate in some role, or a sidecar failed validation, or a process survived
teardown), 2 environment error (py-spy missing from PATH, or a py-spy
permission error).
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from rsglang.profiling import analysis, hook, procs, scenarios, session, sidecar  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="scripts/baseline_profile.py", allow_abbrev=False)
    sub = parser.add_subparsers(dest="command", required=True)

    discover = sub.add_parser(
        "discover",
        help="Launch the server once, role-identify its children, confirm the "
        "profiling hook activated in every role, and write a discover-mode sidecar",
    )
    discover.add_argument("--model", default="Qwen/Qwen3-0.6B", metavar="MODEL",
                           help="Model path/name forwarded to the server (default: %(default)s)")
    discover.add_argument("--port", type=int, default=1919, metavar="PORT",
                           help="Server port (default: %(default)s)")
    discover.add_argument("--timeout", type=float, default=900.0, metavar="SECONDS",
                           help="Seconds to wait for /v1/models readiness (default: %(default)s)")
    discover.add_argument("--out", type=Path, default=None, metavar="PATH",
                           help="Output sidecar path (default: <work-dir>/discover.json)")
    discover.add_argument("--work-dir", type=Path, default=None, metavar="DIR",
                           help="Scratch directory (default: a fresh tempfile.mkdtemp)")
    discover.add_argument("--py-spy-sudo", action="store_true",
                           help="Attach py-spy via a non-interactive `sudo -n`")
    discover.add_argument("--server-cmd", default=None, metavar="TEMPLATE",
                           help="Override server launch command, with {python}/{model}/{port} "
                           "fields (used by Mac tests)")
    discover.add_argument("--settle-s", type=float, default=3.0, metavar="SECONDS",
                           help="Seconds to wait after role ID so each hook thread flushes "
                           "at least once (default: %(default)s)")
    discover.add_argument("--sample-interval-s", type=float, default=1.0, metavar="SECONDS",
                           help="Profiling hook flush interval (default: %(default)s)")

    run = sub.add_parser(
        "run",
        help="Profile each requested scenario end to end and write a validated sidecar",
    )
    run.add_argument("--model", default="Qwen/Qwen3-0.6B", metavar="MODEL")
    run.add_argument("--port", type=int, default=1919, metavar="PORT")
    run.add_argument("--timeout", type=float, default=900.0, metavar="SECONDS")
    run.add_argument("--out", type=Path, default=None, metavar="PATH",
                      help=f"Output sidecar path (default: {sidecar.CANONICAL_OUT})")
    run.add_argument("--work-dir", type=Path, default=None, metavar="DIR")
    run.add_argument("--py-spy-sudo", action="store_true")
    run.add_argument("--py-spy-rate", type=int, default=100, metavar="HZ")
    run.add_argument("--sample-interval-s", type=float, default=1.0, metavar="SECONDS")
    run.add_argument("--scenarios", default="s1,s2,s3", metavar="LIST",
                      help="Comma-separated subset of s1,s2,s3 (default: %(default)s)")
    run.add_argument("--seed", type=int, default=42)
    run.add_argument("--server-cmd", default=None, metavar="TEMPLATE")
    run.add_argument("--hyperfine", default="hyperfine", metavar="PATH")
    run.add_argument("--s1-agents", type=int, default=128)
    run.add_argument("--s1-duration-s", type=float, default=120.0)
    run.add_argument("--s1-cancel-fraction", type=float, default=0.25)
    run.add_argument("--s1-max-tokens", type=int, default=256)
    run.add_argument("--s1-think-max-s", type=float, default=0.5)
    run.add_argument("--s2-requests", type=int, default=512)
    run.add_argument("--s2-max-input", type=int, default=32)
    run.add_argument("--s2-output-tokens", type=int, default=32)
    run.add_argument("--s3-runs", type=int, default=3)
    run.add_argument("--s3-warmup", type=int, default=1)
    run.add_argument("--s3-max-tokens", type=int, default=16)
    run.add_argument("--s3-sample-s", type=float, default=10.0)

    coldstart_once = sub.add_parser(
        "coldstart-once",
        help="Launch the server once and record its self-timed readiness (hyperfine's timed command)",
    )
    coldstart_once.add_argument("--model", default="Qwen/Qwen3-0.6B", metavar="MODEL")
    coldstart_once.add_argument("--port", type=int, default=1919, metavar="PORT")
    coldstart_once.add_argument("--timeout", type=float, default=900.0, metavar="SECONDS")
    coldstart_once.add_argument("--server-cmd", default=None, metavar="TEMPLATE")
    coldstart_once.add_argument("--pgid-file", type=Path, required=True, metavar="PATH")
    coldstart_once.add_argument("--record-file", type=Path, required=True, metavar="PATH")
    coldstart_once.add_argument("--log", type=Path, required=True, metavar="PATH")

    coldstart_stop = sub.add_parser(
        "coldstart-stop",
        help="Sample whole-tree RSS/PSS and tear down the server (hyperfine's --conclude)",
    )
    coldstart_stop.add_argument("--pgid-file", type=Path, required=True, metavar="PATH")
    coldstart_stop.add_argument("--record-file", type=Path, required=True, metavar="PATH")

    validate = sub.add_parser("validate", help="Validate a baseline-profile.json sidecar")
    validate.add_argument("file", metavar="FILE", type=Path)
    validate.add_argument("--require-gpu", action="store_true")

    return parser


def cmd_discover(ns: argparse.Namespace) -> int:
    work_dir = Path(ns.work_dir) if ns.work_dir else Path(tempfile.mkdtemp(prefix="baseline_profile."))
    work_dir.mkdir(parents=True, exist_ok=True)
    out_path = ns.out or (work_dir / "discover.json")

    try:
        base = procs.py_spy_base(sudo=ns.py_spy_sudo)
    except procs.PySpyMissingError as exc:
        print(
            f"py-spy: {exc}. Install the human-approved py-spy==0.4.2 into the venv.",
            file=sys.stderr,
        )
        return 2

    py_spy_ver = procs.py_spy_version(base)

    shim_dir = work_dir / "shim"
    profile_dir = work_dir / "hook"
    hook.write_shim(shim_dir)
    env = hook.hook_env(
        os.environ, shim_dir=shim_dir, profile_dir=profile_dir, interval_s=ns.sample_interval_s
    )

    log_path = work_dir / "server.log"
    argv = procs.server_argv(ns.server_cmd, python=sys.executable, model=ns.model, port=ns.port)

    handle = None
    children: list[int] = []

    def run_body() -> int:
        nonlocal handle, children
        handle = procs.launch_server(argv, env=env, log_path=log_path)

        try:
            ready_at = procs.wait_ready(handle, port=ns.port, timeout_s=ns.timeout)
        except (procs.ServerExited, TimeoutError) as exc:
            print(str(exc), file=sys.stderr)
            return 1

        children = procs.discover_children(handle.proc.pid)

        dumps: dict[int, str] = {}
        for pid in children:
            try:
                dumps[pid] = procs.py_spy_dump(pid, base=base)
            except procs.PySpyPermissionError as exc:
                print(str(exc), file=sys.stderr)
                return 2

        try:
            roles = procs.identify_roles(handle.proc.pid, dumps)
        except procs.RoleError as exc:
            print(str(exc), file=sys.stderr)
            return 1

        time.sleep(ns.settle_s)

        records = hook.load_hook_records(profile_dir)
        role_pid = {
            "api_server": roles["api_server"],
            "scheduler": roles["scheduler"],
            "tokenizer": roles["tokenizer"],
        }
        hook_active = {
            role: any(r.get("kind") == "start" for r in records.by_pid.get(pid, []))
            for role, pid in role_pid.items()
        }
        gc_count = {
            role: sum(1 for r in records.by_pid.get(pid, []) if r.get("kind") == "gc")
            for role, pid in role_pid.items()
        }

        inactive = [role for role, active in hook_active.items() if not active]
        for role in inactive:
            print(f"hook did not activate in {role}", file=sys.stderr)
        if inactive:
            return 1

        doc = {
            "schema_version": sidecar.SCHEMA_VERSION,
            "generated_by": sidecar.GENERATED_BY,
            "meta": sidecar.build_meta(
                mode="discover", model=ns.model, py_spy_version=py_spy_ver, rate_hz=0, flags=[]
            ),
            "scenarios": {},
            "warnings": [],
            "discovery": {
                "processes": {
                    "api_server": role_pid["api_server"],
                    "scheduler": role_pid["scheduler"],
                    "tokenizer": role_pid["tokenizer"],
                    "other": roles["other"],
                },
                "hook_active": hook_active,
                "gc_count": gc_count,
                "ready_s": ready_at - handle.t_launch,
            },
        }

        try:
            sidecar.write_sidecar(doc, out_path)
        except sidecar.SidecarError as exc:
            for err in exc.errors:
                print(f"sidecar validation: {err}", file=sys.stderr)
            return 1

        print(
            f"api_server={role_pid['api_server']} scheduler={role_pid['scheduler']} "
            f"tokenizer={role_pid['tokenizer']} other={roles['other']}"
        )
        print(f"wrote {out_path}")
        return 0

    try:
        rc = run_body()
    finally:
        if handle is not None:
            alive = procs.teardown(handle, extra_pids=children, grace_s=60.0)
            if alive:
                print(f"process(es) still alive after teardown: {alive}", file=sys.stderr)
                rc = 1

    return rc


_SCENARIO_NAME_MAP = {"s1": "s1_cancel", "s2": "s2_saturation", "s3": "s3_coldstart"}


def _parse_scenarios(spec: str) -> "list[str] | None":
    """Comma-separated s1/s2/s3 tokens -> their sidecar.SCENARIOS names, in the
    fixed order s1_cancel, s2_saturation, s3_coldstart. None on an unknown token."""
    tokens = [t.strip() for t in spec.split(",") if t.strip()]
    for token in tokens:
        if token not in _SCENARIO_NAME_MAP:
            return None
    names = {_SCENARIO_NAME_MAP[t] for t in tokens}
    return [name for name in sidecar.SCENARIOS if name in names]


def cmd_run(ns: argparse.Namespace) -> int:
    requested = _parse_scenarios(ns.scenarios)
    if requested is None:
        print(f"unknown scenario in --scenarios {ns.scenarios!r}; expected s1, s2 and/or s3", file=sys.stderr)
        return 2

    try:
        base = procs.py_spy_base(sudo=ns.py_spy_sudo)
    except procs.PySpyMissingError as exc:
        print(
            f"py-spy: {exc}. Install the human-approved py-spy==0.4.2 into the venv.",
            file=sys.stderr,
        )
        return 2

    out_path = Path(ns.out) if ns.out else (REPO_ROOT / sidecar.CANONICAL_OUT)
    canonical_path = (REPO_ROOT / sidecar.CANONICAL_OUT).resolve()
    is_canonical = out_path.resolve() == canonical_path
    if is_canonical:
        gpu_name = sidecar._gpu_name()
        if not (sys.platform.startswith("linux") and gpu_name):
            print(
                f"refusing to write {sidecar.CANONICAL_OUT} from a non-GPU run; pass --out <path>",
                file=sys.stderr,
            )
            return 2

    if "s3_coldstart" in requested:
        hyperfine_path = shutil.which(ns.hyperfine)
        if not hyperfine_path:
            print(f"hyperfine: {ns.hyperfine!r} not found on PATH", file=sys.stderr)
            return 2
        hyperfine_ok, hyperfine_err = _check_hyperfine_version(hyperfine_path)
        if not hyperfine_ok:
            print(hyperfine_err, file=sys.stderr)
            return 2

    py_spy_ver = procs.py_spy_version(base)

    work_dir = Path(ns.work_dir) if ns.work_dir else Path(tempfile.mkdtemp(prefix="baseline_profile."))
    work_dir.mkdir(parents=True, exist_ok=True)

    argv = procs.server_argv(ns.server_cmd, python=sys.executable, model=ns.model, port=ns.port)

    scenarios_out: dict = {}
    warnings: list[str] = []

    try:
        if "s1_cancel" in requested:
            entry, warns = session.run_session(
                "s1_cancel",
                argv=argv,
                port=ns.port,
                timeout_s=ns.timeout,
                work_dir=work_dir,
                py_spy=base,
                rate_hz=ns.py_spy_rate,
                interval_s=ns.sample_interval_s,
                workload=lambda url: scenarios.run_s1(
                    url,
                    agents=ns.s1_agents,
                    duration_s=ns.s1_duration_s,
                    cancel_fraction=ns.s1_cancel_fraction,
                    max_tokens=ns.s1_max_tokens,
                    think_max_s=ns.s1_think_max_s,
                    seed=ns.seed,
                ),
                include_boot=False,
                params={
                    "agents": ns.s1_agents,
                    "duration_s": ns.s1_duration_s,
                    "cancel_fraction": ns.s1_cancel_fraction,
                    "max_tokens": ns.s1_max_tokens,
                    "think_max_s": ns.s1_think_max_s,
                    "seed": ns.seed,
                },
            )
            scenarios_out["s1_cancel"] = entry
            warnings.extend(warns)

        if "s2_saturation" in requested:
            entry, warns = session.run_session(
                "s2_saturation",
                argv=argv,
                port=ns.port,
                timeout_s=ns.timeout,
                work_dir=work_dir,
                py_spy=base,
                rate_hz=ns.py_spy_rate,
                interval_s=ns.sample_interval_s,
                workload=lambda url: scenarios.run_s2(
                    url,
                    requests=ns.s2_requests,
                    max_input=ns.s2_max_input,
                    output_tokens=ns.s2_output_tokens,
                    seed=ns.seed,
                ),
                include_boot=False,
                params={
                    "requests": ns.s2_requests,
                    "max_input": ns.s2_max_input,
                    "output_tokens": ns.s2_output_tokens,
                    "seed": ns.seed,
                },
            )
            scenarios_out["s2_saturation"] = entry
            warnings.extend(warns)

        if "s3_coldstart" in requested:
            entry, warns = _run_s3(ns, argv=argv, work_dir=work_dir, py_spy=base)
            scenarios_out["s3_coldstart"] = entry
            warnings.extend(warns)
    except (
        procs.ServerExited,
        TimeoutError,
        procs.RoleError,
        session.MeasurementError,
        analysis.SpeedscopeError,
    ) as exc:
        print(str(exc), file=sys.stderr)
        return 1

    meta = sidecar.build_meta(
        mode="run",
        model=ns.model,
        py_spy_version=py_spy_ver,
        rate_hz=ns.py_spy_rate,
        flags=["--nonblocking", "--format", "speedscope"],
    )
    doc = {
        "schema_version": sidecar.SCHEMA_VERSION,
        "generated_by": sidecar.GENERATED_BY,
        "meta": meta,
        "scenarios": scenarios_out,
        "warnings": warnings,
    }

    try:
        sidecar.write_sidecar(
            doc, out_path, require_scenarios=tuple(requested), require_gpu=is_canonical
        )
    except sidecar.SidecarError as exc:
        for err in exc.errors:
            print(f"sidecar validation: {err}", file=sys.stderr)
        return 1

    print(f"wrote {out_path}")
    for key, entry in scenarios_out.items():
        print(f"{key}: radix.share={entry['radix']['share']}")
    return 0


def _check_hyperfine_version(hyperfine_path: str) -> "tuple[bool, str]":
    import subprocess

    try:
        out = subprocess.run([hyperfine_path, "--version"], capture_output=True, text=True, timeout=10)
    except OSError as exc:
        return False, f"hyperfine: could not run --version: {exc}"
    if out.returncode != 0:
        return False, f"hyperfine --version failed: {out.stderr.strip()}"
    text = (out.stdout or out.stderr).strip()
    parts = text.split()
    version = parts[1] if len(parts) > 1 else ""
    try:
        version_fields = tuple(int(p) for p in version.split("."))
    except ValueError:
        return False, f"hyperfine: could not parse version from {text!r}"
    if version_fields < (1, 19, 0):
        return (
            False,
            f"hyperfine {version} is older than the required 1.19.0; "
            "install the human-approved hyperfine==1.20.0 (cargo install hyperfine --version 1.20.0)",
        )
    return True, ""


def _run_s3(ns: argparse.Namespace, *, argv: list[str], work_dir: Path, py_spy: list[str]) -> "tuple[dict, list[str]]":
    return session.run_coldstart(
        model=ns.model,
        port=ns.port,
        timeout_s=ns.timeout,
        work_dir=work_dir,
        py_spy=py_spy,
        rate_hz=ns.py_spy_rate,
        interval_s=ns.sample_interval_s,
        hyperfine=ns.hyperfine,
        runs=ns.s3_runs,
        warmup=ns.s3_warmup,
        max_tokens=ns.s3_max_tokens,
        sample_s=ns.s3_sample_s,
        server_cmd=ns.server_cmd,
        argv=argv,
        params={
            "runs": ns.s3_runs,
            "warmup": ns.s3_warmup,
            "max_tokens": ns.s3_max_tokens,
            "sample_s": ns.s3_sample_s,
        },
    )


def cmd_coldstart_once(ns: argparse.Namespace) -> int:
    argv = procs.server_argv(ns.server_cmd, python=sys.executable, model=ns.model, port=ns.port)
    return scenarios.coldstart_once(
        argv=argv,
        port=ns.port,
        timeout_s=ns.timeout,
        pgid_file=ns.pgid_file,
        record_file=ns.record_file,
        log_path=ns.log,
    )


def cmd_coldstart_stop(ns: argparse.Namespace) -> int:
    return scenarios.coldstart_stop(pgid_file=ns.pgid_file, record_file=ns.record_file)


def cmd_validate(ns: argparse.Namespace) -> int:
    doc = json.loads(Path(ns.file).read_text(encoding="utf-8"))
    require_scenarios = sidecar.SCENARIOS if ns.require_gpu else ()
    errors = sidecar.validate_sidecar(
        doc, require_scenarios=require_scenarios, require_gpu=ns.require_gpu
    )
    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        return 1
    print("valid")
    return 0


def main(argv: list[str] | None = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.command == "discover":
        return cmd_discover(ns)
    if ns.command == "run":
        return cmd_run(ns)
    if ns.command == "coldstart-once":
        return cmd_coldstart_once(ns)
    if ns.command == "coldstart-stop":
        return cmd_coldstart_stop(ns)
    if ns.command == "validate":
        return cmd_validate(ns)
    return 2


if __name__ == "__main__":
    sys.exit(main())
