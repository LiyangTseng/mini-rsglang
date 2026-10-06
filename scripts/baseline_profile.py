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
import os
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from rsglang.profiling import hook, procs, sidecar  # noqa: E402


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


def main(argv: list[str] | None = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.command == "discover":
        return cmd_discover(ns)
    return 2


if __name__ == "__main__":
    sys.exit(main())
