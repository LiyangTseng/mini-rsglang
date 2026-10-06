"""Report-to-JSON consistency tests for docs/benchmarks/baseline-profile.md
(BENCH-01, plan 02-09, T-02-17).

The narrative report is hand-written from the committed GPU sidecar
(docs/benchmarks/baseline-profile.json); this test is the mechanical proof
that every radix share and P99 TTFT the JSON reports as a number also
appears in the markdown, formatted exactly as the sidecar's own numbers
would render (f"{share * 100:.2f}%" and f"{p99:.1f} ms"), and that it
re-validates as a real GPU run. It never recomputes or second-guesses the
numbers -- only checks that the narrative didn't drift from the sidecar.
"""

from __future__ import annotations

import json
from pathlib import Path

from rsglang.profiling import sidecar

REPO_ROOT = Path(__file__).resolve().parents[2]
JSON_PATH = REPO_ROOT / "docs" / "benchmarks" / "baseline-profile.json"
MD_PATH = REPO_ROOT / "docs" / "benchmarks" / "baseline-profile.md"

REQUIRED_HEADINGS = [
    "# Python Frontend Baseline Profile",
    "## Run",
    "## Topology and GIL framing",
    "## Scenario 1: 128 concurrent agents with cancellations",
    "## Scenario 2: 32-token short-prompt saturation",
    "## Scenario 3: Cold start and host RAM",
    "## Radix cache share (input to RADIX-01)",
    "## Inputs to Phase 7 benchmark design",
    "## Known blind spots",
    "## Reproduce",
]


def _load_doc() -> dict:
    with open(JSON_PATH, encoding="utf-8") as fh:
        return json.load(fh)


def _load_markdown() -> str:
    return MD_PATH.read_text(encoding="utf-8")


def test_sidecar_is_a_valid_gpu_run():
    doc = _load_doc()
    assert sidecar.validate_sidecar(doc, require_scenarios=sidecar.SCENARIOS, require_gpu=True) == []


def test_all_required_headings_present():
    markdown = _load_markdown()
    lines = set(markdown.splitlines())
    missing = [heading for heading in REQUIRED_HEADINGS if heading not in lines]
    assert not missing, f"missing exact heading lines: {missing}"


def test_every_radix_share_appears_formatted():
    doc = _load_doc()
    markdown = _load_markdown()
    for key in sidecar.SCENARIOS:
        share = doc["scenarios"][key]["radix"]["share"]
        if share is None:
            continue
        formatted = f"{share * 100:.2f}%"
        assert formatted in markdown, f"{key}: radix share {formatted!r} not found in markdown"


def test_every_p99_ttft_appears_formatted():
    doc = _load_doc()
    markdown = _load_markdown()
    for key in sidecar.SCENARIOS:
        p99 = doc["scenarios"][key]["requests"]["ttft_ms"]["p99"]
        if p99 is None:
            continue
        formatted = f"{p99:.1f} ms"
        assert formatted in markdown, f"{key}: P99 TTFT {formatted!r} not found in markdown"
