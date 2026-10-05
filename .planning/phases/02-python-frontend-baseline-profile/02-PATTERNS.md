# Phase 2: Python Frontend Baseline Profile - Pattern Map

**Mapped:** 2026-10-05
**Files analyzed:** 5 (new) + 0 modified
**Analogs found:** 5 / 5 (role-match or better)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `scripts/gpu_phase2_profile.sh` | utility (bash GPU runbook wrapper) | event-driven (launch → poll → sample → report) | `scripts/gpu_phase1_check.sh` | exact (same CLI shape, same Mac/GPU split convention) |
| `scripts/baseline_profile.py` | utility (profiling driver/orchestrator) | request-response + file-I/O (launches server, drives HTTP scenarios, writes JSON) | `scripts/check_upstream.py` | role-match (standalone stdlib-first CLI script with argparse, exit codes, docstring contract) |
| `python/tests/test_baseline_profile.py` | test | transform (parsing/bucketing logic, stubbed subprocess I/O) | `python/tests/test_gpu_check_script.py` | exact (Mac-runnable test that stubs external tools and exercises helper functions in isolation) |
| `docs/benchmarks/baseline-profile.md` | config/doc (narrative report) | batch (hand-written, reads from JSON sidecar) | `docs/mini-sglang-reading-guide.md` | role-match (durable `docs/` reference material, no code analog needed) |
| `docs/benchmarks/baseline-profile.json` | config (machine-readable sidecar) | batch (written once per profiling run) | *(no analog — new concept)* | none — see "No Analog Found" |

## Pattern Assignments

### `scripts/gpu_phase2_profile.sh` (utility, event-driven)

**Analog:** `scripts/gpu_phase1_check.sh` (confirmed tracked: `git ls-files scripts/gpu_phase1_check.sh`)

**Usage/help pattern** (lines 1-32):
```bash
#!/usr/bin/env bash
# Phase 1 GPU verification: ... A human runs it once at the end of the phase and signs off ...
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/gpu_phase1_check.sh [--model Qwen/Qwen3-0.6B] [--port 1919] [--timeout 900] [--help]
...
EOF
}
```
Copy this `usage()` + `set -euo pipefail` + flag-parsing `while [ $# -gt 0 ]; case "$1" in --flag) VAR="${2:?--flag needs a value}"; shift 2 ;; --help|-h) usage; exit 0 ;; esac` shape verbatim for the new script's `--model`/`--port`/`--timeout`/`--help` flags (Discretion area, but the flag *shape* should match).

**PYTHON resolution + PYTHONPATH export** (lines 49-54):
```bash
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [ -z "${PYTHON:-}" ]; then
  if [ -x .venv/bin/python ]; then PYTHON=.venv/bin/python; else PYTHON=python3; fi
fi
export PYTHONPATH="$ROOT/python${PYTHONPATH:+:$PYTHONPATH}"
```
Reuse directly — same project, same venv convention.

**Log directory + PASS/FAIL recording** (lines 56-66, 341-352):
```bash
TMP_BASE="${TMPDIR:-/tmp}"
LOG_DIR="$(mktemp -d "${TMP_BASE%/}/gpu_phase1_check.XXXXXX")"
declare -a RESULTS=()
record() { RESULTS+=("$2 step $1: $3"); echo "$2 step $1: $3"; }
...
for r in "${RESULTS[@]}"; do echo "$r"; case "$r" in FAIL*) failed=1 ;; esac; done
if [ "$failed" = 0 ]; then echo "ALL PASS"; exit 0; fi
```
Adapt for profiling: treat each of the 3 scenarios + radix-sampling pass as a "step" that is PASS (ran, wrote its slice of JSON) or FAIL.

**Server-launch + readiness-poll pattern** (step2, lines 190-212): launches `"$PYTHON" -m rsglang.launch --frontend python --model "$MODEL" --port "$PORT"` under `start_session`, then polls `curl -sf "http://127.0.0.1:$PORT/v1/models"` until ready or timeout. This is the exact Pattern 1 from RESEARCH.md ("race-free PID discovery via the server's own ready signal") already implemented in bash — `baseline_profile.py` should do the Python equivalent (`subprocess.Popen` + poll `/v1/models`), and `gpu_phase2_profile.sh` can either delegate entirely to `baseline_profile.py` (simplest) or reuse `start_session`/`stop_session` if it needs to supervise the server itself. **Source-of-truth note:** the server launch command is `python -m rsglang.launch --frontend python --model <M> --port <P>` — confirmed in both this script and `python/rsglang/launch.py`.

**Preflight tool-check pattern** (lines 166-174):
```bash
missing=()
for tool in nvidia-smi cargo curl setsid; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "FAIL preflight: not on PATH: ${missing[*]}"
  exit 1
fi
```
Reuse for `py-spy`, `hyperfine`, `curl` preflight checks (per Common Pitfall 4: py-spy needs elevated privileges — add a `sudo -n py-spy dump --pid $$ 2>&1 | head -1` capability probe alongside the plain `command -v` check).

**Cleanup trap for orphaned processes** (lines 68-75):
```bash
cleanup() {
  local pgid
  for pgid in "${STARTED_PGIDS[@]+"${STARTED_PGIDS[@]}"}"; do
    kill -9 -- "-$pgid" 2>/dev/null || true
  done
}
trap cleanup EXIT
```
Reuse verbatim — the profiling script also launches a real server process that must not be leaked if the script errors mid-run.

---

### `scripts/baseline_profile.py` (utility, request-response + file-I/O)

**Analog:** `scripts/check_upstream.py` (confirmed tracked)

**Module docstring + exit-code contract** (lines 1-21):
```python
#!/usr/bin/env python3
"""Prove vendor/mini-sglang/ equals pristine upstream except for the files UPSTREAM.md lists (D-03),
and that the frozen Python frontend is untouched (D-04).

...

Standard library only, so it runs on the GPU box before any environment exists. Never writes under
the vendored directory. Exit codes: 0 OK, 1 violations, 2 environment error (git, network).
"""

from __future__ import annotations

import argparse
import os
...
```
Copy the shape: a top-of-file docstring stating (a) what the script proves/produces, (b) its dependency footprint, (c) its exit-code contract (for `baseline_profile.py`: 0 = JSON sidecar written successfully, 1 = a scenario failed to complete, 2 = environment error — missing py-spy/psutil/hyperfine, GPU box only). State explicitly that it is the **only** script permitted to write `docs/benchmarks/baseline-profile.json` (D-14), mirroring `check_upstream.py`'s "Never writes under the vendored directory" guarantee statement.

**`from __future__ import annotations` + argparse-first structure**: both scripts should use `argparse.ArgumentParser` with explicit `--help` text, matching `python/rsglang/launch.py`'s `build_parser()` convention (lines 52-66) for flag naming style (`--rust-bin`, `--ready-timeout` style: kebab-case long flags, `metavar=` set, inline help string).

**Readiness-poll + subprocess launch pattern** — copy from `python/rsglang/launch.py`'s own idioms rather than reinventing:
```python
# python/rsglang/launch.py:85-92 — os.execv pattern for "frontend python" mode;
# baseline_profile.py instead uses subprocess.Popen (it must stay alive to supervise/sample),
# but the target command is identical:
argv = [sys.executable, "-m", "rsglang.launch", "--frontend", "python", "--model", MODEL, "--port", str(PORT)]
```
And the HTTP readiness poll used by `scripts/gpu_phase1_check.sh` step2 (`until curl -sf ".../v1/models" ...; do sleep 1; done`) translates directly to `urllib.request.urlopen` in RESEARCH.md's Pattern 1 code example — use that, not a new approach.

**JSON sidecar writing**: no direct in-repo analog (first JSON-writing script in the project) — follow `scripts/check_upstream.py`'s general "stdlib only, explicit exit codes" discipline, and validate the written JSON against a schema check before returning 0 (per RESEARCH.md's Security Domain V5 note), matching the project's existing emphasis on scripts failing loudly rather than silently producing bad output (see `check_upstream.py`'s STALE_LISTING/UNLISTED_CHANGE violation reporting, lines 8-16, as the "fail loud on malformed state" precedent).

---

### `python/tests/test_baseline_profile.py` (test, transform)

**Analog:** `python/tests/test_gpu_check_script.py` (confirmed tracked)

**Module docstring + slow-marker convention** (lines 1-20):
```python
"""Mac-runnable tests for scripts/gpu_phase1_check.sh's GPU-orphan helpers
(G-01-7-WR07 / WR-08).

Each test sources the script under bash with stub `nvidia-smi` and `setsid`
executables placed first on PATH, so the real preflight, build and GPU steps
never run ...
"""

from __future__ import annotations
import os, re, subprocess, sys, time
from pathlib import Path
import pytest

pytestmark = pytest.mark.slow  # every test spawns bash
```
For `test_baseline_profile.py`: use `pytestmark = pytest.mark.slow` only on the tests that actually spawn a process (the `.pth`-hook propagation pre-flight test, per RESEARCH.md Pitfall 3 / Validation Architecture's `test_pth_hook_propagates_to_spawn_child`); keep the JSON-schema, radix-frame-bucketing, and role-identification tests unmarked (pure functions, no subprocess) so they run in the fast `pytest -m "not slow"` pass per `scripts/check_all.sh` step 2.

**Stub-executable-on-PATH pattern** (lines 32-60):
```python
def _write_stub(tmp_path: Path, name: str, body: str) -> Path:
    """Write an executable `#!/bin/bash` stub named `name` into tmp_path/bin."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir(exist_ok=True)
    stub = bin_dir / name
    stub.write_text(f"#!/bin/bash\n{body}\n")
    stub.chmod(0o755)
    return stub
```
Reuse this exact helper shape to stub `py-spy` (emit canned speedscope JSON or a canned `dump` stack trace containing `_run_scheduler`/`tokenize_worker`) and `hyperfine` (emit canned timing JSON) on `PATH`, so `test_role_identification` and `test_radix_frame_bucketing` never need the real binaries.

**Env + PATH injection for subprocess tests** (lines 63-80):
```python
def _bash(snippet: str, tmp_path: Path, **env_vars: object) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ)
    env["PATH"] = f"{bin_dir}{os.pathsep}{env.get('PATH', '')}"
    env["TMPDIR"] = str(tmp_path)
    env["PYTHON"] = sys.executable
    env.update({k: str(v) for k, v in env_vars.items()})
    ...
```
For the `.pth`-hook pre-flight test, the Python equivalent is injecting a temp `site-packages`-like directory with the `.pth` file onto `sys.path`/`PYTHONPATH` for a `multiprocessing.Process(spawn)` child, then asserting the child's `gc.callbacks` fired (per RESEARCH.md Pitfall 3's recommended pre-flight check) — same "build an isolated env, run a subprocess, assert on its observable side effect" shape as this analog.

**Grouped test sections with `# --- <name> ---` comment banners** (lines 83-271): mirror this exact section-banner convention (`# --- on_gpu ---`, `# --- wait_no_orphans ---`) for `# --- json_sidecar_schema ---`, `# --- radix_frame_bucketing ---`, `# --- role_identification ---`, `# --- pth_hook_propagation ---`.

---

### `docs/benchmarks/baseline-profile.md` (doc, batch)

**Analog:** `docs/mini-sglang-reading-guide.md` (role-match; both are durable `docs/`-tier reference material, distinct from `.planning/` process state)

No code excerpt applies (prose document) — the only pattern to carry over is the **directory convention**: `docs/` holds durable material that outlives a single phase's `.planning/phases/...` artifacts (per CONTEXT.md D-12's explicit rationale), so `docs/benchmarks/` is a peer of the existing `docs/mini-sglang-reading-guide.md` and `docs/agents/`, not a new top-level concept. Write the narrative to explicitly flag the Open Question 1 framing (GIL-held-percentage per process, not cross-process "contention") rather than inventing an unsupported metric.

## Shared Patterns

### Mac/GPU script split
**Source:** `scripts/gpu_phase1_check.sh` + `python/tests/test_gpu_check_script.py`
**Apply to:** `scripts/gpu_phase2_profile.sh` / `scripts/baseline_profile.py` + `python/tests/test_baseline_profile.py`

The established convention in this repo: the GPU-only script is a thin, `--help`-documented bash wrapper (or stdlib-only Python script) with a bash-sourcing guard —
```bash
# Mac helper tests in python/tests/test_gpu_check_script.py source this file to reach the
# helpers above; nothing below this guard runs when sourced.
if [ "${BASH_SOURCE[0]}" != "${0}" ]; then
  return 0
fi
```
— so Mac-side tests can exercise pure-function helpers (parsing, bucketing, role-ID string matching) without ever touching `nvidia-smi`/GPU/real `py-spy`. Apply the same split: put all parsing/bucketing/schema logic in testable functions inside `baseline_profile.py`, reserve the actual py-spy/hyperfine subprocess calls and GPU-box launch for a thin "main" path, and give `test_baseline_profile.py` stub executables exactly as `test_gpu_check_script.py` does.

### Preflight-then-numbered-steps-then-summary
**Source:** `scripts/gpu_phase1_check.sh` lines 166-352
**Apply to:** `scripts/gpu_phase2_profile.sh`
Preflight tool check → one numbered step per scenario (1: 128-agent cancellation, 2: 32-token RPS, 3: cold-start/RAM, plus a radix-sampling pass piggybacked per D-11) → each step's PASS/FAIL recorded via `record()` → final summary block exits 0 only if everything passed. Reuse the `record()`/`RESULTS` array and the final `ALL PASS` / `SOME STEPS FAILED` summary banner verbatim.

### Stdlib-first, explicit exit codes
**Source:** `scripts/check_upstream.py` docstring (lines 1-21)
**Apply to:** `scripts/baseline_profile.py`
State the dependency footprint and exit-code contract at the top of the file docstring. `baseline_profile.py` differs from `check_upstream.py` in that it does need `py-spy`/`psutil` (not stdlib-only) — document that deviation explicitly in the docstring rather than silently diverging from the "standard library only" precedent, since `check_upstream.py` calls that out as a deliberate design choice (GPU box has no environment yet) whereas this phase's script runs after the venv is set up.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `docs/benchmarks/baseline-profile.json` | config (sidecar) | batch | No prior machine-readable sidecar exists anywhere in the repo (`scripts/check_upstream.py` and `scripts/gpu_phase1_check.sh` only print to stdout/log files, never emit a structured JSON artifact for downstream phases to consume). Use RESEARCH.md's Code Examples section (`radix_share()` speedscope-bucketing function, the `gc.callbacks` self-timestamping snippet, and the `tracemalloc` periodic-snapshot loop) as the schema source instead of a codebase analog. Validate the written JSON against a hand-written schema/dataclass check per RESEARCH.md's Security Domain V5 note (Wave 0 gap: `test_json_sidecar_schema`). |

## Metadata

**Analog search scope:** `scripts/`, `python/tests/`, `python/rsglang/`, `docs/`, repo root configs (`requirements-mac.txt`, `pyproject.toml`)
**Files scanned:** `scripts/gpu_phase1_check.sh`, `scripts/check_upstream.py`, `scripts/check_wire_decode.sh`, `scripts/gen_wire_fixtures.py`, `scripts/bootstrap_mac_env.sh`, `python/tests/test_gpu_check_script.py`, `python/rsglang/launch.py`, `python/rsglang/handshake.py`, `python/rsglang/sockets.py`, `requirements-mac.txt`
**All analog paths verified git-tracked** via `git ls-files` (no gitignored/mirror paths used)
**Pattern extraction date:** 2026-10-05
