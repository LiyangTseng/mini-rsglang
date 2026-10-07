#!/usr/bin/env python3
"""The only writer of docs/benchmarks/parity-report.json (D-07): PAR-01's
zero-tolerance hard gate and (in later plans) PAR-02's informational
concurrent-load measurement.

Dependency footprint: standard library plus aiohttp, from the project's own
venv -- this deliberately departs from scripts/check_upstream.py's
stdlib-only rule, because this script runs only after that venv exists.

Never writes under vendor/.

Exit codes: 0 OK, 1 measurement failure or any gate mismatch or invalid
sidecar, 2 environment error.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from rsglang.parity import compare, corpus, sidecar, sweep, tap  # noqa: E402
from rsglang.profiling import procs  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]

_SUPPORTED_PARTS = ("sequential", "concurrent", "endpoints")

_DEFAULT_PYTHON_SERVER_CMD = "{python} -m rsglang.launch --frontend python --model {model} --port {port}"
_DEFAULT_RUST_SERVER_CMD = "{python} -m rsglang.launch --frontend rust --model {model} --port {port}"


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="scripts/parity_check.py", allow_abbrev=False)
    sub = parser.add_subparsers(dest="command", required=True)

    run = sub.add_parser(
        "run", help="Run the parity sweep(s), sequential Python-then-Rust, and write a validated sidecar"
    )
    run.add_argument(
        "--models",
        default="Qwen/Qwen3-0.6B,meta-llama/Llama-3.2-1B-Instruct",
        metavar="LIST",
        help="Comma list; the first is the gate model (default: %(default)s)",
    )
    run.add_argument("--corpus", default=corpus.CANONICAL_CORPUS, metavar="PATH")
    run.add_argument(
        "--parts",
        default="endpoints,sequential,concurrent",
        metavar="LIST",
        help="Comma list of sequential, concurrent, endpoints (default: %(default)s)",
    )
    run.add_argument("--port", type=int, default=1919, metavar="PORT")
    run.add_argument("--timeout", type=float, default=900.0, metavar="SECONDS")
    run.add_argument(
        "--concurrency",
        type=int,
        default=128,
        metavar="N",
        help="In-flight request cap for the 'concurrent' part (default: %(default)s)",
    )
    run.add_argument("--out", default=sidecar.CANONICAL_OUT, metavar="PATH")
    run.add_argument("--work-dir", default=None, metavar="DIR")
    run.add_argument(
        "--python-server-cmd",
        default="{python} -m rsglang.launch --frontend python --model {model} --port {port}",
        metavar="TEMPLATE",
    )
    run.add_argument(
        "--rust-server-cmd",
        default="{python} -m rsglang.launch --frontend rust --model {model} --port {port}",
        metavar="TEMPLATE",
    )

    validate = sub.add_parser("validate", help="Validate a parity-report.json sidecar")
    validate.add_argument("file", metavar="FILE", type=Path)
    validate.add_argument("--require-gpu", action="store_true")

    discover = sub.add_parser(
        "discover",
        help="Check every endpoint for one frontend against a fresh session, with the backend tap active",
    )
    discover.add_argument("--frontend", required=True, choices=("python", "rust"))
    discover.add_argument("--model", default="Qwen/Qwen3-0.6B", metavar="MODEL")
    discover.add_argument("--port", type=int, default=1919, metavar="PORT")
    discover.add_argument("--timeout", type=float, default=900.0, metavar="SECONDS")
    discover.add_argument("--server-cmd", default=None, metavar="TEMPLATE")
    discover.add_argument("--work-dir", default=None, metavar="DIR")
    discover.add_argument("--out", default=None, metavar="PATH")
    discover.add_argument("--skip-tap-check", action="store_true")

    verdict = sub.add_parser(
        "verdict", help="Print a PASS/FAIL verdict for one ROADMAP criterion from a sidecar"
    )
    verdict.add_argument("file", metavar="FILE", type=Path)
    verdict.add_argument("--criterion", type=int, required=True, metavar="N")

    return parser


def _slug(model: str) -> str:
    return "".join(c if c.isalnum() else "-" for c in model).strip("-").lower()


_GATED_RE = re.compile(
    r"gated repo|access to model|401 client error|cannot access gated|restricted",
    re.IGNORECASE,
)


def _classify_session_failure(exc: Exception) -> "tuple[str, str]":
    """Returns (status, reason) for a session that raised ServerExited or
    TimeoutError before becoming ready. ServerExited's message embeds the
    last 40 log lines (procs.wait_ready); a line matching the gated-repo
    regex means the checkpoint is simply unavailable, not broken."""
    if isinstance(exc, TimeoutError):
        return "failed", str(exc)
    text = str(exc)
    for line in text.splitlines():
        if _GATED_RE.search(line):
            return "unavailable", line.strip()
    lines = [line for line in text.splitlines() if line.strip()]
    return "failed", (lines[-1] if lines else text)


class _PortInUse(Exception):
    def __init__(self, port: int):
        self.port = port


def _require_port_free(port: int) -> None:
    if not sweep.port_free(port):
        print(f"port {port} in use", file=sys.stderr)
        raise _PortInUse(port)


def _run_concurrent_part(
    *,
    gate_model: str,
    items: "list",
    concurrency: int,
    python_argv: "list[str]",
    rust_argv: "list[str]",
    port: int,
    timeout_s: float,
    work_dir: Path,
    slug: str,
    gate_sides: "dict[str, dict]",
) -> dict:
    """Runs a fresh Python session, then a fresh Rust session, each sending
    the whole corpus at `concurrency` at once, and joins each frontend's
    results against that SAME frontend's own sequential sides for the gate
    model (D-10)."""
    _require_port_free(port)
    try:
        python_session = sweep.run_session(
            f"conc-python-{slug}",
            argv=python_argv,
            port=port,
            timeout_s=timeout_s,
            work_dir=work_dir,
            workload=lambda url: sweep.send_concurrent(url, gate_model, items, concurrency, timeout_s),
        )
    except (procs.ServerExited, TimeoutError) as exc:
        print(f"concurrent {gate_model} python: {exc}", file=sys.stderr)
        return {
            "status": "failed",
            "reason": str(exc),
            "concurrency": concurrency,
            "summary": None,
            "prompts": [],
        }

    _require_port_free(port)
    try:
        rust_session = sweep.run_session(
            f"conc-rust-{slug}",
            argv=rust_argv,
            port=port,
            timeout_s=timeout_s,
            work_dir=work_dir,
            workload=lambda url: sweep.send_concurrent(url, gate_model, items, concurrency, timeout_s),
        )
    except (procs.ServerExited, TimeoutError) as exc:
        print(f"concurrent {gate_model} rust: {exc}", file=sys.stderr)
        return {
            "status": "failed",
            "reason": str(exc),
            "concurrency": concurrency,
            "summary": None,
            "prompts": [],
        }

    python_seq_side = gate_sides["python"]
    rust_seq_side = gate_sides["rust"]
    python_expected = {pid: side["input_ids"] for pid, side in python_seq_side.items() if side.get("input_ids")}
    rust_expected = {pid: side["input_ids"] for pid, side in rust_seq_side.items() if side.get("input_ids")}

    python_conc_side, python_unmatched = sweep.join_by_input_ids(
        items, python_session.value, python_session.tap, python_expected
    )
    rust_conc_side, rust_unmatched = sweep.join_by_input_ids(
        items, rust_session.value, rust_session.tap, rust_expected
    )

    prompts = []
    for item in items:
        record = compare.compare_prompt(item, python_conc_side[item.id], rust_conc_side[item.id])
        record["python_vs_sequential"] = compare.compare_prompt(
            item, python_seq_side[item.id], python_conc_side[item.id]
        )["match"]
        record["rust_vs_sequential"] = compare.compare_prompt(
            item, rust_seq_side[item.id], rust_conc_side[item.id]
        )["match"]
        prompts.append(record)

    summary = compare.summarize(prompts)
    summary["python_vs_sequential_matched"] = sum(1 for r in prompts if r["python_vs_sequential"])
    summary["rust_vs_sequential_matched"] = sum(1 for r in prompts if r["rust_vs_sequential"])
    summary["unmatched_tap"] = python_unmatched + rust_unmatched

    return {
        "status": "ok",
        "reason": None,
        "concurrency": concurrency,
        "summary": summary,
        "prompts": prompts,
    }


def cmd_run(ns: argparse.Namespace) -> int:
    parts = [p.strip() for p in ns.parts.split(",") if p.strip()]
    for part in parts:
        if part not in _SUPPORTED_PARTS:
            print(
                f"unsupported --parts value {part!r}; expected a comma list from {_SUPPORTED_PARTS}",
                file=sys.stderr,
            )
            return 2

    if "concurrent" in parts and "sequential" not in parts:
        print("concurrent needs sequential in the same run", file=sys.stderr)
        return 2

    if ns.concurrency < 1:
        print(f"--concurrency must be >= 1, got {ns.concurrency}", file=sys.stderr)
        return 2

    models = [m.strip() for m in ns.models.split(",") if m.strip()]
    if not models:
        print("--models must list at least one model", file=sys.stderr)
        return 2
    gate_model = models[0]

    try:
        items = corpus.load_corpus(Path(ns.corpus))
    except corpus.CorpusError as exc:
        for err in exc.errors:
            print(f"corpus: {err}", file=sys.stderr)
        return 2

    out_path = Path(ns.out)
    canonical_path = (REPO_ROOT / sidecar.CANONICAL_OUT).resolve()
    if out_path.resolve() == canonical_path:
        gpu_name = sidecar._gpu_name()
        if not (sys.platform.startswith("linux") and gpu_name):
            print(
                f"refusing to write {sidecar.CANONICAL_OUT} from a non-GPU run; pass --out <path>",
                file=sys.stderr,
            )
            return 2

    work_dir = Path(ns.work_dir) if ns.work_dir else Path(tempfile.mkdtemp(prefix="parity_check."))
    work_dir.mkdir(parents=True, exist_ok=True)

    sequential_out: dict = {}
    concurrent_out: "dict | None" = None
    endpoints_out: "dict | None" = None
    any_failure = False
    warnings: "list[str]" = []
    gate_sides: "dict[str, dict] | None" = None
    effective_concurrency: "int | None" = None

    try:
        if "endpoints" in parts:
            python_argv = procs.server_argv(
                ns.python_server_cmd, python=sys.executable, model=gate_model, port=ns.port
            )
            rust_argv = procs.server_argv(
                ns.rust_server_cmd, python=sys.executable, model=gate_model, port=ns.port
            )

            python_entries: "list[dict]" = []
            rust_entries: "list[dict]" = []

            _require_port_free(ns.port)
            try:
                python_ep_session = sweep.run_session(
                    "endpoints-python",
                    argv=python_argv,
                    port=ns.port,
                    timeout_s=ns.timeout,
                    work_dir=work_dir,
                    workload=lambda url: sweep.endpoint_smoke(url, "python", gate_model),
                )
                python_entries = python_ep_session.value
            except (procs.ServerExited, TimeoutError) as exc:
                print(f"endpoints python: {exc}", file=sys.stderr)
                any_failure = True

            _require_port_free(ns.port)
            try:
                rust_ep_session = sweep.run_session(
                    "endpoints-rust",
                    argv=rust_argv,
                    port=ns.port,
                    timeout_s=ns.timeout,
                    work_dir=work_dir,
                    workload=lambda url: sweep.endpoint_smoke(url, "rust", gate_model),
                )
                rust_entries = rust_ep_session.value
            except (procs.ServerExited, TimeoutError) as exc:
                print(f"endpoints rust: {exc}", file=sys.stderr)
                any_failure = True

            endpoints_out = {"python": python_entries, "rust": rust_entries}
            p_ok = sum(1 for e in python_entries if e.get("ok"))
            r_ok = sum(1 for e in rust_entries if e.get("ok"))
            print(f"endpoints python: {p_ok}/{len(python_entries)} ok")
            print(f"endpoints rust: {r_ok}/{len(rust_entries)} ok")

        for model in models:
            slug = _slug(model)
            python_argv = procs.server_argv(
                ns.python_server_cmd, python=sys.executable, model=model, port=ns.port
            )
            rust_argv = procs.server_argv(
                ns.rust_server_cmd, python=sys.executable, model=model, port=ns.port
            )

            try:
                _require_port_free(ns.port)
                python_session = sweep.run_session(
                    f"seq-python-{slug}",
                    argv=python_argv,
                    port=ns.port,
                    timeout_s=ns.timeout,
                    work_dir=work_dir,
                    workload=lambda url, _model=model: sweep.send_sequential(url, _model, items, ns.timeout),
                )
                _require_port_free(ns.port)
                rust_session = sweep.run_session(
                    f"seq-rust-{slug}",
                    argv=rust_argv,
                    port=ns.port,
                    timeout_s=ns.timeout,
                    work_dir=work_dir,
                    workload=lambda url, _model=model: sweep.send_sequential(url, _model, items, ns.timeout),
                )
            except (procs.ServerExited, TimeoutError) as exc:
                status, reason = _classify_session_failure(exc)
                print(f"{model}: {reason}", file=sys.stderr)
                sequential_out[model] = {
                    "status": status,
                    "reason": reason,
                    "summary": None,
                    "prompts": [],
                }
                if status == "unavailable":
                    warnings.append(f"{model} unavailable: {reason}")
                    if model == gate_model:
                        any_failure = True
                else:
                    any_failure = True
                continue

            python_side = sweep.join_sequential(items, python_session.value, python_session.tap)
            rust_side = sweep.join_sequential(items, rust_session.value, rust_session.tap)

            records = [
                compare.compare_prompt(item, python_side[item.id], rust_side[item.id]) for item in items
            ]
            summary = compare.summarize(records)
            sequential_out[model] = {
                "status": "ok",
                "reason": None,
                "summary": summary,
                "prompts": records,
            }

            matched, n = summary["matched"], summary["n"]
            print(f"sequential {model}: {matched}/{n} identical")
            if model == gate_model and matched < n:
                any_failure = True
            if model == gate_model:
                gate_sides = {"python": python_side, "rust": rust_side}

        if "concurrent" in parts:
            effective_concurrency = min(ns.concurrency, len(items)) if items else ns.concurrency
            if effective_concurrency < ns.concurrency:
                warnings.append(
                    f"concurrency {ns.concurrency} clamped to {effective_concurrency} (corpus size)"
                )

            if gate_sides is None:
                concurrent_out = {
                    gate_model: {
                        "status": "failed",
                        "reason": "gate model sequential failed; concurrent skipped",
                        "concurrency": effective_concurrency,
                        "summary": None,
                        "prompts": [],
                    }
                }
                any_failure = True
            else:
                slug = _slug(gate_model)
                python_argv = procs.server_argv(
                    ns.python_server_cmd, python=sys.executable, model=gate_model, port=ns.port
                )
                rust_argv = procs.server_argv(
                    ns.rust_server_cmd, python=sys.executable, model=gate_model, port=ns.port
                )
                conc_block = _run_concurrent_part(
                    gate_model=gate_model,
                    items=items,
                    concurrency=effective_concurrency,
                    python_argv=python_argv,
                    rust_argv=rust_argv,
                    port=ns.port,
                    timeout_s=ns.timeout,
                    work_dir=work_dir,
                    slug=slug,
                    gate_sides=gate_sides,
                )
                concurrent_out = {gate_model: conc_block}
                if conc_block["status"] == "failed":
                    any_failure = True
                s = conc_block.get("summary") or {}
                print(
                    f"concurrent {gate_model} @{conc_block['concurrency']}: "
                    f"{s.get('matched')}/{s.get('n')} identical (informational)"
                )
    except _PortInUse:
        return 2

    doc = {
        "schema_version": sidecar.SCHEMA_VERSION,
        "generated_by": sidecar.GENERATED_BY,
        "meta": sidecar.build_meta(
            mode="run",
            models=models,
            gate_model=gate_model,
            corpus_path=str(ns.corpus),
            corpus_sha256=corpus.corpus_sha256(Path(ns.corpus)),
            corpus_n=len(items),
            concurrency=effective_concurrency,
        ),
        "endpoints": endpoints_out,
        "sequential": sequential_out,
        "concurrent": concurrent_out,
        "abort_stress": None,
        "warnings": warnings,
    }

    try:
        sidecar.write_sidecar(doc, out_path)
    except sidecar.SidecarError as exc:
        for err in exc.errors:
            print(f"sidecar validation: {err}", file=sys.stderr)
        return 1

    print(f"wrote {out_path}")

    if any_failure:
        return 1
    return 0


def cmd_validate(ns: argparse.Namespace) -> int:
    doc = json.loads(Path(ns.file).read_text(encoding="utf-8"))
    errors = sidecar.validate_sidecar(doc, require_gpu=ns.require_gpu)
    if errors:
        for err in errors:
            print(err, file=sys.stderr)
        return 1
    print("valid")
    return 0


def cmd_discover(ns: argparse.Namespace) -> int:
    if not sweep.port_free(ns.port):
        print(f"port {ns.port} in use", file=sys.stderr)
        return 2

    work_dir = Path(ns.work_dir) if ns.work_dir else Path(tempfile.mkdtemp(prefix="parity_check."))
    work_dir.mkdir(parents=True, exist_ok=True)
    out_path = Path(ns.out) if ns.out else (work_dir / f"discover-{ns.frontend}.json")

    default_cmd = _DEFAULT_RUST_SERVER_CMD if ns.frontend == "rust" else _DEFAULT_PYTHON_SERVER_CMD
    server_cmd = ns.server_cmd or default_cmd
    argv = procs.server_argv(server_cmd, python=sys.executable, model=ns.model, port=ns.port)

    try:
        session_result = sweep.run_session(
            f"discover-{ns.frontend}",
            argv=argv,
            port=ns.port,
            timeout_s=ns.timeout,
            work_dir=work_dir,
            workload=lambda url: sweep.endpoint_smoke(url, ns.frontend, ns.model),
        )
    except (procs.ServerExited, TimeoutError) as exc:
        print(str(exc), file=sys.stderr)
        return 1

    endpoints = session_result.value

    if ns.skip_tap_check:
        tap_info = {"checked": False, "patched": False, "user_records": False, "detok_finished": False}
        tap_ok = True
    else:
        records = session_result.tap.records
        patched = any(r["kind"] == tap.KIND_PATCHED for r in records)
        user_records = any(r["kind"] == tap.KIND_USER for r in records)
        detok_finished = any(r["kind"] == tap.KIND_DETOK and r.get("finished") for r in records)
        tap_info = {
            "checked": True,
            "patched": patched,
            "user_records": user_records,
            "detok_finished": detok_finished,
        }
        tap_ok = patched and user_records and detok_finished

    doc = {
        "schema_version": sidecar.SCHEMA_VERSION,
        "generated_by": sidecar.GENERATED_BY,
        "meta": sidecar.build_meta(
            mode="discover",
            models=[ns.model],
            gate_model=ns.model,
            corpus_path="",
            corpus_sha256="",
            corpus_n=0,
            concurrency=None,
        ),
        "frontend": ns.frontend,
        "endpoints": endpoints,
        "tap": tap_info,
    }

    try:
        sidecar.write_discover(doc, out_path)
    except sidecar.SidecarError as exc:
        for err in exc.errors:
            print(f"sidecar validation: {err}", file=sys.stderr)
        return 1

    ok_count = sum(1 for e in endpoints if e.get("ok"))
    print(f"endpoints {ns.frontend}: {ok_count}/{len(endpoints)} ok")
    print(f"wrote {out_path}")

    if ok_count == len(endpoints) and tap_ok:
        return 0
    return 1


def _verdict_criterion_1(doc: dict) -> int:
    endpoints = doc.get("endpoints")
    if not isinstance(endpoints, dict):
        print("criterion 1: FAIL no endpoints recorded", file=sys.stderr)
        return 1

    rust_entries = endpoints.get("rust") or []
    python_entries = endpoints.get("python") or []
    rust_by_name = {e.get("name"): e for e in rust_entries if isinstance(e, dict)}
    failing = [
        name
        for name in sweep.RUST_ENDPOINTS
        if not (name in rust_by_name and rust_by_name[name].get("ok") is True)
    ]
    rust_ok_count = sum(1 for e in rust_entries if isinstance(e, dict) and e.get("ok") is True)
    python_ok_count = sum(1 for e in python_entries if isinstance(e, dict) and e.get("ok") is True)
    k = len(sweep.RUST_ENDPOINTS)

    if not failing:
        print(f"criterion 1: PASS rust {rust_ok_count}/{k} endpoints ok (python {python_ok_count}/5)")
        return 0
    print(f"criterion 1: FAIL {', '.join(failing)}")
    return 1


def _verdict_criterion_2(doc: dict) -> int:
    meta = doc.get("meta") or {}
    models = meta.get("models") or []
    gate_model = meta.get("gate_model")
    sequential = doc.get("sequential") or {}

    gate_block = sequential.get(gate_model)
    gate_summary = gate_block.get("summary") if isinstance(gate_block, dict) else None
    gate_ok = (
        isinstance(gate_block, dict)
        and gate_block.get("status") == "ok"
        and isinstance(gate_summary, dict)
        and gate_summary.get("n", 0) >= 100
        and gate_summary.get("matched") == gate_summary.get("n")
    )

    lines: "list[str]" = []
    ok = gate_ok
    if gate_ok:
        lines.append(f"{gate_model} {gate_summary['matched']}/{gate_summary['n']} identical (hard gate)")
    elif isinstance(gate_block, dict) and gate_block.get("status") == "ok" and isinstance(gate_summary, dict):
        lines.append(
            f"{gate_model} {gate_summary.get('matched')}/{gate_summary.get('n')} identical, "
            "needs n>=100 and matched==n (hard gate)"
        )
    else:
        status = gate_block.get("status") if isinstance(gate_block, dict) else "missing"
        reason = gate_block.get("reason") if isinstance(gate_block, dict) else "no sequential block for gate model"
        lines.append(f"{gate_model} {status}: {reason} (hard gate)")

    for model in models:
        if model == gate_model:
            continue
        block = sequential.get(model)
        if isinstance(block, dict) and block.get("status") == "ok":
            s = block.get("summary") or {}
            lines.append(f"{model} {s.get('matched')}/{s.get('n')} identical (reported, not gated)")
        else:
            ok = False
            status = block.get("status") if isinstance(block, dict) else "missing"
            reason = block.get("reason") if isinstance(block, dict) else "no sequential block"
            lines.append(f"{model} {status}: {reason} (reported, not gated)")

    verdict_word = "PASS" if ok else "FAIL"
    print(f"criterion 2: {verdict_word} " + "; ".join(lines))
    return 0 if ok else 1


def _verdict_criterion_3(doc: dict) -> int:
    meta = doc.get("meta") or {}
    gate_model = meta.get("gate_model")
    concurrency_meta = meta.get("concurrency")
    concurrent = doc.get("concurrent") or {}
    block = concurrent.get(gate_model)
    summary = block.get("summary") if isinstance(block, dict) else None

    ok = (
        isinstance(block, dict)
        and block.get("status") == "ok"
        and block.get("concurrency") == concurrency_meta
        and isinstance(summary, dict)
        and summary.get("n", 0) >= 100
    )

    if ok:
        c = block["concurrency"]
        print(
            f"criterion 3: PASS {gate_model} @{c}: {summary['matched']}/{summary['n']} "
            f"Rust-vs-Python identical, python vs sequential "
            f"{summary.get('python_vs_sequential_matched')}/{summary['n']}, rust vs sequential "
            f"{summary.get('rust_vs_sequential_matched')}/{summary['n']} (informational)"
        )
        return 0

    status = block.get("status") if isinstance(block, dict) else "missing"
    reason = (
        block.get("reason") if isinstance(block, dict) else "no concurrent block for meta.gate_model"
    )
    print(f"criterion 3: FAIL {gate_model} {status}: {reason}")
    return 1


def cmd_verdict(ns: argparse.Namespace) -> int:
    try:
        doc = json.loads(Path(ns.file).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"verdict: could not read {ns.file}: {exc}", file=sys.stderr)
        return 2

    errors = sidecar.validate_sidecar(doc, require_gpu=False)
    if errors:
        for err in errors:
            print(f"sidecar validation: {err}", file=sys.stderr)
        return 2

    if ns.criterion == 1:
        return _verdict_criterion_1(doc)
    if ns.criterion == 2:
        return _verdict_criterion_2(doc)
    if ns.criterion == 3:
        return _verdict_criterion_3(doc)

    print(f"verdict: unknown criterion {ns.criterion}", file=sys.stderr)
    return 2


def main(argv: "list[str] | None" = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.command == "run":
        return cmd_run(ns)
    if ns.command == "validate":
        return cmd_validate(ns)
    if ns.command == "discover":
        return cmd_discover(ns)
    if ns.command == "verdict":
        return cmd_verdict(ns)
    return 2


if __name__ == "__main__":
    sys.exit(main())
