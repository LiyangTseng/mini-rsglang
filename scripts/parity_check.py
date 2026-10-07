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
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from rsglang.parity import compare, corpus, sidecar, sweep  # noqa: E402
from rsglang.profiling import procs  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[1]

_SUPPORTED_PARTS = ("sequential", "concurrent")


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
        default="sequential",
        metavar="LIST",
        help="Comma list of sequential, concurrent (default: %(default)s)",
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

    return parser


def _slug(model: str) -> str:
    return "".join(c if c.isalnum() else "-" for c in model).strip("-").lower()


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

    work_dir = Path(ns.work_dir) if ns.work_dir else Path(tempfile.mkdtemp(prefix="parity_check."))
    work_dir.mkdir(parents=True, exist_ok=True)

    sequential_out: dict = {}
    concurrent_out: "dict | None" = None
    any_failure = False
    warnings: "list[str]" = []
    gate_sides: "dict[str, dict] | None" = None
    effective_concurrency: "int | None" = None

    try:
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
                print(f"{model}: {exc}", file=sys.stderr)
                sequential_out[model] = {
                    "status": "failed",
                    "reason": str(exc),
                    "summary": None,
                    "prompts": [],
                }
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
            if matched < n:
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
        "endpoints": None,
        "sequential": sequential_out,
        "concurrent": concurrent_out,
        "abort_stress": None,
        "warnings": warnings,
    }

    out_path = Path(ns.out)
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


def main(argv: "list[str] | None" = None) -> int:
    ns = _build_parser().parse_args(argv)
    if ns.command == "run":
        return cmd_run(ns)
    if ns.command == "validate":
        return cmd_validate(ns)
    return 2


if __name__ == "__main__":
    sys.exit(main())
