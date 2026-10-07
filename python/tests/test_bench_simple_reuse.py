"""Tests for rsglang.bench.standard_throughput (BENCH-06): the bench_simple-
shaped standard-inference workload driver (D-12).

test_tracer_main_writes_json proves main() end to end through an injected
runner -- no openai/transformers/minisgl.benchmark needed. The remaining
tests (added in Task 2) pin the module to upstream's bench_simple.py and
client.py shapes with an ast-based drift guard, and exercise summarize()'s
percentile and edge-case formulas directly.
"""

from __future__ import annotations

import ast
import json
from pathlib import Path

import pytest

from rsglang.bench import standard_throughput as st


def _vendor_root() -> Path:
    return Path(__file__).resolve().parents[2] / "vendor" / "mini-sglang"


def _parse(path: Path) -> ast.Module:
    return ast.parse(path.read_text(encoding="utf-8"), filename=str(path))


def _literal_assign_value(tree: ast.Module, name: str):
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name) and target.id == name:
                    return ast.literal_eval(node.value)
    raise AssertionError(f"no assignment to {name} found in {tree}")


def _has_call(tree: ast.Module, func_attr_path: list[str], args: list) -> bool:
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        names: list[str] = []
        cur = node.func
        while isinstance(cur, ast.Attribute):
            names.append(cur.attr)
            cur = cur.value
        if isinstance(cur, ast.Name):
            names.append(cur.id)
        names.reverse()
        if names != func_attr_path:
            continue
        try:
            call_args = [ast.literal_eval(a) for a in node.args]
        except ValueError:
            continue
        if call_args == args:
            return True
    return False


def _top_level_func(tree: ast.Module, name: str):
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == name:
            return node
    raise AssertionError(f"no top-level function {name} found")


def test_tracer_main_writes_json(tmp_path: Path) -> None:
    out_path = tmp_path / "o.json"

    async def fake(port, *, seed, batch_size, max_input):
        return {
            "model": "m",
            "t_start_unix": 1.0,
            "t_end_unix": 3.0,
            "tics": [[0.0, 1.0, 2.0, 3.0], [1.0, 2.0, 4.0]],
        }

    rc = st.main(["--port", "1919", "--out", str(out_path)], runner=fake)

    assert rc == 0
    doc = json.loads(out_path.read_text(encoding="utf-8"))
    assert doc["schema"] == "rsglang.bench.standard_throughput/1"
    assert doc["summary"]["num_tokens"] == 7
    assert doc["summary"]["duration_s"] == 4.0
    assert doc["summary"]["throughput_tok_s"] == 1.75
    assert doc["summary"]["throughput_req_s"] == 0.5
    assert doc["params"]["seed"] == 42


# --- Task 2: drift guard against bench_simple.py and client.py --------------


def test_bench_simple_constants_match() -> None:
    tree = _parse(_vendor_root() / "benchmark" / "online" / "bench_simple.py")

    test_bs = _literal_assign_value(tree, "TEST_BS")
    max_input = _literal_assign_value(tree, "MAX_INPUT")
    assert test_bs == [64]
    assert max_input == 8192
    assert _has_call(tree, ["random", "seed"], [42]), "random.seed(42) not found in bench_simple.py"
    assert _has_call(
        tree, ["random", "randint"], [16, 1024]
    ), "random.randint(16, 1024) not found in bench_simple.py"

    assert st.BATCH_SIZE == test_bs[0]
    assert st.MAX_INPUT == max_input
    assert st.SEED == 42
    assert st.OUTPUT_MIN == 16
    assert st.OUTPUT_MAX == 1024


def test_client_helper_signatures_match() -> None:
    tree = _parse(_vendor_root() / "python" / "minisgl" / "benchmark" / "client.py")

    benchmark_one_batch = _top_level_func(tree, "benchmark_one_batch")
    assert isinstance(benchmark_one_batch, ast.AsyncFunctionDef)
    assert [a.arg for a in benchmark_one_batch.args.args] == [
        "client",
        "prompts",
        "output_lengths",
        "model",
    ]

    generate_prompt = _top_level_func(tree, "generate_prompt")
    assert isinstance(generate_prompt, ast.FunctionDef)
    assert [a.arg for a in generate_prompt.args.args] == ["tokenizer", "n"]

    get_model_name = _top_level_func(tree, "get_model_name")
    assert isinstance(get_model_name, ast.AsyncFunctionDef)
    assert [a.arg for a in get_model_name.args.args] == ["client"]

    benchmark_one = _top_level_func(tree, "benchmark_one")
    assert isinstance(benchmark_one, ast.AsyncFunctionDef)
    assert [a.arg for a in benchmark_one.args.args][:4] == [
        "client",
        "prompt",
        "output_length",
        "model",
    ]


def test_summarize_upstream_indexing() -> None:
    tics_lists = [[0.0, float(i), float(i) + 100.0] for i in range(1, 11)]
    result = st.summarize(tics_lists)

    first_times = sorted(float(i) for i in range(1, 11))
    n = len(first_times)
    assert result["ttft_ms"]["p50"] == first_times[int(n * 0.5)] * 1000.0
    assert result["ttft_ms"]["p90"] == first_times[int(n * 0.9)] * 1000.0
    assert result["ttft_ms"]["p99"] == first_times[int(n * 0.99)] * 1000.0


def test_summarize_rejects_empty_and_zero_duration() -> None:
    with pytest.raises(ValueError):
        st.summarize([])
    with pytest.raises(ValueError):
        st.summarize([[1.0], [1.0]])


def test_main_failure_and_symlink_refusal(tmp_path: Path) -> None:
    async def raising_runner(port, *, seed, batch_size, max_input):
        raise RuntimeError("boom")

    out_path = tmp_path / "o.json"
    rc = st.main(["--port", "1919", "--out", str(out_path)], runner=raising_runner)
    assert rc == 1
    assert not out_path.exists()

    target = tmp_path / "target.json"
    target.write_text("untouched", encoding="utf-8")
    link = tmp_path / "link.json"
    link.symlink_to(target)

    async def fake(port, *, seed, batch_size, max_input):
        return {
            "model": "m",
            "t_start_unix": 1.0,
            "t_end_unix": 2.0,
            "tics": [[0.0, 1.0, 2.0]],
        }

    rc2 = st.main(["--port", "1919", "--out", str(link)], runner=fake)
    assert rc2 == 1
    assert target.read_text(encoding="utf-8") == "untouched"
